use std::collections::HashSet;
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use futures::io::{AsyncRead, AsyncWrite};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, copy_bidirectional};
use tokio::net::TcpStream;
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{Semaphore, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{sleep, timeout};
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use tracing::{debug, info, warn};
use wslproxy_protocol::{SplitIo, host_handshake};
use yamux::{Config, Connection, Mode, Stream};

use super::config::{ReverseForwardRule, Settings};
use super::wsl;

mod embedded_agent {
    include!(concat!(env!("OUT_DIR"), "/embedded_agent.rs"));
}

const AGENT_FILE_NAME: &str = "wslproxy-agent-v1";
const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);
const PROCESS_STOP_TIMEOUT: Duration = Duration::from_secs(3);
const AGENT_NOT_FOUND_EXIT_CODE: i32 = 42;
const AGENT_PATH_TEST: &str = "cache_root=${XDG_CACHE_HOME:-$HOME/.cache}; if test -x \"$cache_root/wslproxy/wslproxy-agent-v1\"; then exit 0; else exit 42; fi";
const AGENT_DEPLOY: &str = "cache_root=${XDG_CACHE_HOME:-$HOME/.cache}; agent_dir=\"$cache_root/wslproxy\"; agent_path=\"$agent_dir/wslproxy-agent-v1\"; temp_path=\"$agent_path.tmp.$$\"; umask 077; mkdir -p \"$agent_dir\" && chmod 700 \"$agent_dir\" && cat > \"$temp_path\" && chmod 700 \"$temp_path\" && mv -f \"$temp_path\" \"$agent_path\"";
const AGENT_EXEC: &str = "cache_root=${XDG_CACHE_HOME:-$HOME/.cache}; exec \"$cache_root/wslproxy/wslproxy-agent-v1\" \"$@\"";

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type AgentPipe = SplitIo<ChildStdout, ChildStdin>;

trait AgentLauncher: Send + Sync {
    fn start<'a>(
        &'a self,
        rule: &'a ReverseForwardRule,
        max_connections: usize,
        connect_timeout: Duration,
        check_only: bool,
    ) -> BoxFuture<'a, Result<ActiveAgent>>;
}

struct SystemAgentLauncher {
    binary: &'static [u8],
    deployed_distros: tokio::sync::Mutex<HashSet<String>>,
}

struct ActiveAgent {
    child: Child,
    pipe: Option<AgentPipe>,
    stderr_task: JoinHandle<()>,
}

enum SessionEnd {
    Stopped,
    Failed {
        error: anyhow::Error,
        elapsed: Duration,
    },
}

pub(crate) struct PreparedReverse {
    launcher: Option<Arc<dyn AgentLauncher>>,
    initial_agents: Vec<(ReverseForwardRule, ActiveAgent)>,
}

pub async fn prepare(settings: &Settings) -> Result<PreparedReverse> {
    if settings.reverse_forwards.is_empty() {
        return Ok(PreparedReverse {
            launcher: None,
            initial_agents: Vec::new(),
        });
    }

    let launcher: Arc<dyn AgentLauncher> = Arc::new(SystemAgentLauncher::new()?);
    let mut initial_agents = Vec::with_capacity(settings.reverse_forwards.len());

    for rule in &settings.reverse_forwards {
        match launcher
            .start(
                rule,
                settings.daemon.max_connections_per_rule,
                settings.daemon.connect_timeout,
                false,
            )
            .await
        {
            Ok(agent) => initial_agents.push(agent),
            Err(error) => {
                stop_agents(&mut initial_agents).await;
                return Err(error).with_context(|| {
                    format!("无法启动反向规则 `{}` 的 WSL agent", rule.name)
                });
            }
        }
    }

    Ok(PreparedReverse {
        launcher: Some(launcher),
        initial_agents: settings
            .reverse_forwards
            .iter()
            .cloned()
            .zip(initial_agents)
            .collect(),
    })
}

pub fn launch(
    mut prepared: PreparedReverse,
    settings: &Settings,
    shutdown: watch::Receiver<bool>,
    tasks: &mut JoinSet<Result<()>>,
) -> usize {
    let Some(launcher) = prepared.launcher.take() else {
        return 0;
    };
    let rules = prepared.initial_agents.len();

    for (rule, agent) in prepared.initial_agents.drain(..) {
        tasks.spawn(supervise_rule(
            rule,
            agent,
            Arc::clone(&launcher),
            settings.daemon.connect_timeout,
            settings.daemon.max_connections_per_rule,
            shutdown.clone(),
        ));
    }

    rules
}

pub async fn check(settings: &Settings) -> Result<usize> {
    if settings.reverse_forwards.is_empty() {
        return Ok(0);
    }

    let launcher = SystemAgentLauncher::new()?;
    for rule in &settings.reverse_forwards {
        let mut agent = launcher
            .start(
                rule,
                settings.daemon.max_connections_per_rule,
                settings.daemon.connect_timeout,
                true,
            )
            .await
            .with_context(|| format!("无法检查反向规则 `{}` 的 WSL agent", rule.name))?;
        agent
            .wait_for_check(settings.daemon.connect_timeout)
            .await
            .with_context(|| format!("反向规则 `{}` 的 WSL agent 检查未完成", rule.name))?;

        let target = connect_windows_target(rule, settings.daemon.connect_timeout).await?;
        drop(target);
        info!(
            rule = %rule.name,
            listen = %format_args!("127.0.0.1:{}", rule.listen.port),
            target = %format_args!("127.0.0.1:{}", rule.target.port),
            "反向规则检查通过"
        );
    }

    Ok(settings.reverse_forwards.len())
}

impl SystemAgentLauncher {
    fn new() -> Result<Self> {
        let binary = embedded_agent::EMBEDDED_AGENT.context(
            "当前 wslproxy 未内嵌 Linux agent. 请使用 just install 或包含 bundled-agent feature 的发布构建. 源码安装仅支持 forwards.",
        )?;
        if binary.is_empty() {
            bail!("当前 wslproxy 内嵌的 Linux agent 无效");
        }

        Ok(Self {
            binary,
            deployed_distros: tokio::sync::Mutex::new(HashSet::new()),
        })
    }

    async fn ensure_deployed(&self, distro: &str) -> Result<()> {
        let mut deployed_distros = self.deployed_distros.lock().await;
        if deployed_distros.contains(distro) {
            return Ok(());
        }

        self.ensure_x86_64(distro).await?;
        if !agent_is_deployed(distro).await? {
            deploy_agent(distro, self.binary).await?;
            info!(distro, file = AGENT_FILE_NAME, "已部署 WSL 反向转发 agent");
        }

        deployed_distros.insert(distro.to_string());
        Ok(())
    }

    async fn ensure_x86_64(&self, distro: &str) -> Result<()> {
        let output = wsl::command()
            .args(["--distribution", distro, "--exec", "uname", "-m"])
            .output()
            .await
            .with_context(|| format!("无法检查 WSL 发行版 `{distro}` 的架构"))?;
        let output = check_wsl_output("检查架构", distro, output)?;
        let architecture = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if architecture != "x86_64" {
            bail!(
                "反向规则仅支持 x86_64 WSL 发行版. `{distro}` 当前架构为 `{architecture}`"
            );
        }
        Ok(())
    }

    async fn start_agent(
        &self,
        rule: &ReverseForwardRule,
        max_connections: usize,
        connect_timeout: Duration,
        check_only: bool,
    ) -> Result<ActiveAgent> {
        self.ensure_deployed(&rule.distro).await?;

        let listen_port = rule.listen.port.to_string();
        let max_connections = max_connections.to_string();
        let mut command = wsl::command();
        command
            .args([
                "--distribution",
                rule.distro.as_str(),
                "--exec",
                "sh",
                "-c",
                AGENT_EXEC,
                "wslproxy-agent",
                "--listen-port",
                listen_port.as_str(),
                "--max-connections",
                max_connections.as_str(),
                "--rule-name",
                rule.name.as_str(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if check_only {
            command.arg("--check");
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("无法启动反向规则 `{}` 的 wsl.exe helper", rule.name))?;
        let stderr = child.stderr.take().context("无法读取 WSL agent 标准错误")?;
        let stderr_task = tokio::spawn(drain_agent_stderr(
            stderr,
            rule.name.clone(),
            rule.distro.clone(),
        ));
        let stdout = child.stdout.take().context("无法读取 WSL agent 标准输出")?;
        let stdin = child.stdin.take().context("无法打开 WSL agent 标准输入")?;
        let mut active = ActiveAgent {
            child,
            pipe: Some(SplitIo::new(stdout, stdin)),
            stderr_task,
        };

        let handshake = timeout(
            connect_timeout,
            host_handshake(
                active.pipe.as_mut().expect("agent pipe must exist"),
                rule.listen.port,
            ),
        )
        .await
        .map_err(|_| anyhow!("等待 WSL agent 握手超时"))?;
        if let Err(error) = handshake {
            active.stop().await;
            return Err(error).context("WSL agent 握手失败");
        }

        info!(
            rule = %rule.name,
            distro = %rule.distro,
            listen = %format_args!("127.0.0.1:{}", rule.listen.port),
            "WSL agent 已就绪"
        );
        Ok(active)
    }
}

impl AgentLauncher for SystemAgentLauncher {
    fn start<'a>(
        &'a self,
        rule: &'a ReverseForwardRule,
        max_connections: usize,
        connect_timeout: Duration,
        check_only: bool,
    ) -> BoxFuture<'a, Result<ActiveAgent>> {
        Box::pin(async move {
            self.start_agent(rule, max_connections, connect_timeout, check_only)
                .await
        })
    }
}

impl ActiveAgent {
    async fn stop(&mut self) {
        self.pipe.take();

        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                if let Err(error) = self.child.kill().await {
                    debug!(error = ?error, "终止 WSL agent helper 失败");
                }
            }
            Err(error) => {
                debug!(error = ?error, "查询 WSL agent helper 状态失败");
            }
        }

        let _ = timeout(PROCESS_STOP_TIMEOUT, self.child.wait()).await;
        self.stderr_task.abort();
        let _ = (&mut self.stderr_task).await;
    }

    async fn wait_for_check(&mut self, connect_timeout: Duration) -> Result<()> {
        self.pipe.take();
        let status = timeout(connect_timeout, self.child.wait())
            .await
            .map_err(|_| anyhow!("等待 WSL agent 检查进程退出超时"))?
            .context("等待 WSL agent 检查进程退出失败")?;
        self.stderr_task.abort();
        let _ = (&mut self.stderr_task).await;

        if !status.success() {
            bail!("WSL agent 检查进程异常退出: {status}");
        }
        Ok(())
    }
}

impl Drop for ActiveAgent {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        self.stderr_task.abort();
    }
}

async fn stop_agents(agents: &mut [ActiveAgent]) {
    for agent in agents {
        agent.stop().await;
    }
}

async fn supervise_rule(
    rule: ReverseForwardRule,
    initial_agent: ActiveAgent,
    launcher: Arc<dyn AgentLauncher>,
    connect_timeout: Duration,
    max_connections: usize,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut agent = Some(initial_agent);
    let mut failures = 0_u32;

    loop {
        let active = match agent.take() {
            Some(agent) => agent,
            None => match wait_for_restart(
                &rule,
                Arc::clone(&launcher),
                connect_timeout,
                max_connections,
                failures,
                &mut shutdown,
            )
            .await?
            {
                Some(agent) => agent,
                None => return Ok(()),
            },
        };

        match serve_agent(active, &rule, connect_timeout, max_connections, &mut shutdown).await {
            SessionEnd::Stopped => return Ok(()),
            SessionEnd::Failed { error, elapsed } => {
                if elapsed >= MAX_RETRY_DELAY {
                    failures = 0;
                }
                failures = failures.saturating_add(1);
                warn!(
                    rule = %rule.name,
                    failures,
                    elapsed_ms = elapsed.as_millis(),
                    error = ?error,
                    "反向 WSL agent 已中断, 将重启"
                );
            }
        }
    }
}

async fn wait_for_restart(
    rule: &ReverseForwardRule,
    launcher: Arc<dyn AgentLauncher>,
    connect_timeout: Duration,
    max_connections: usize,
    mut failures: u32,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<Option<ActiveAgent>> {
    loop {
        let delay = retry_delay(failures);
        info!(
            rule = %rule.name,
            retry_after_seconds = delay.as_secs(),
            "等待重启反向 WSL agent"
        );
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return Ok(None);
                }
            }
            _ = sleep(delay) => {}
        }

        match launcher
            .start(rule, max_connections, connect_timeout, false)
            .await
        {
            Ok(agent) => return Ok(Some(agent)),
            Err(error) => {
                failures = failures.saturating_add(1);
                warn!(
                    rule = %rule.name,
                    failures,
                    error = ?error,
                    "重启反向 WSL agent 失败"
                );
            }
        }
    }
}

async fn serve_agent(
    mut agent: ActiveAgent,
    rule: &ReverseForwardRule,
    connect_timeout: Duration,
    max_connections: usize,
    shutdown: &mut watch::Receiver<bool>,
) -> SessionEnd {
    let started = Instant::now();
    let pipe = agent.pipe.take().expect("agent pipe must exist");
    let mut connection = Connection::new(pipe.compat(), Config::default(), Mode::Server);
    let limiter = Arc::new(Semaphore::new(max_connections));

    let end = loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break SessionEnd::Stopped;
                }
            }
            inbound = next_stream(&mut connection) => {
                match inbound {
                    Ok(Some(stream)) => {
                        let permit = match Arc::clone(&limiter).try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_) => {
                                warn!(rule = %rule.name, limit = max_connections, "反向规则连接数已达到上限");
                                drop(stream);
                                continue;
                            }
                        };
                        let rule = rule.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            if let Err(error) = proxy_stream(stream, &rule, connect_timeout).await {
                                warn!(rule = %rule.name, error = ?error, "反向转发会话失败");
                            }
                        });
                    }
                    Ok(None) => break SessionEnd::Failed {
                        error: anyhow!("WSL agent 协议连接已关闭"),
                        elapsed: started.elapsed(),
                    },
                    Err(error) => break SessionEnd::Failed {
                        error,
                        elapsed: started.elapsed(),
                    },
                }
            }
        }
    };

    agent.stop().await;
    end
}

async fn proxy_stream(
    stream: Stream,
    rule: &ReverseForwardRule,
    connect_timeout: Duration,
) -> Result<()> {
    let started = Instant::now();
    let target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, rule.target.port);
    let mut target_stream = timeout(connect_timeout, TcpStream::connect(target))
        .await
        .map_err(|_| anyhow!("连接 Windows loopback 目标 {target} 超时"))?
        .with_context(|| format!("无法连接 Windows loopback 目标 {target}"))?;
    let mut stream = stream.compat();
    info!(rule = %rule.name, target = %target, "已建立反向转发会话");
    let (wsl_to_windows, windows_to_wsl) = copy_bidirectional(&mut stream, &mut target_stream)
        .await
        .context("反向 TCP 双向转发失败")?;
    info!(
        rule = %rule.name,
        target = %target,
        wsl_to_windows,
        windows_to_wsl,
        elapsed_ms = started.elapsed().as_millis(),
        "反向转发会话已结束"
    );
    Ok(())
}

async fn connect_windows_target(
    rule: &ReverseForwardRule,
    connect_timeout: Duration,
) -> Result<TcpStream> {
    let target = SocketAddrV4::new(Ipv4Addr::LOCALHOST, rule.target.port);
    timeout(connect_timeout, TcpStream::connect(target))
        .await
        .map_err(|_| anyhow!("连接 Windows loopback 目标 {target} 超时"))?
        .with_context(|| format!("无法连接 Windows loopback 目标 {target}"))
}

async fn next_stream<T>(connection: &mut Connection<T>) -> Result<Option<Stream>>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    std::future::poll_fn(|context| connection.poll_next_inbound(context))
        .await
        .transpose()
        .context("Windows 端 yamux 协议连接异常")
}

async fn agent_is_deployed(distro: &str) -> Result<bool> {
    let output = wsl::command()
        .args(["--distribution", distro, "--exec", "sh", "-c", AGENT_PATH_TEST])
        .output()
        .await
        .with_context(|| format!("无法检查 WSL 发行版 `{distro}` 中的 agent"))?;
    if output.status.success() {
        return Ok(true);
    }

    if output.status.code() == Some(AGENT_NOT_FOUND_EXIT_CODE) {
        return Ok(false);
    }
    check_wsl_output("检查 agent", distro, output)?;
    unreachable!("成功的 WSL 命令应在前面返回")
}

async fn deploy_agent(distro: &str, binary: &[u8]) -> Result<()> {
    let mut child = wsl::command()
        .args(["--distribution", distro, "--exec", "sh", "-c", AGENT_DEPLOY])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("无法向 WSL 发行版 `{distro}` 部署 agent"))?;
    let mut stdin = child.stdin.take().context("无法写入 WSL agent 文件")?;
    stdin
        .write_all(binary)
        .await
        .context("无法传输 WSL agent 文件")?;
    stdin.shutdown().await.context("无法完成 WSL agent 传输")?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .await
        .context("等待 WSL agent 部署进程失败")?;
    check_wsl_output("部署 agent", distro, output).map(|_| ())
}

fn check_wsl_output(action: &str, distro: &str, output: std::process::Output) -> Result<std::process::Output> {
    if output.status.success() {
        return Ok(output);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if stderr.is_empty() {
        "未返回错误信息".to_string()
    } else {
        stderr
    };
    bail!("WSL 发行版 `{distro}` {action}失败: {detail}")
}

async fn drain_agent_stderr(stderr: ChildStderr, rule: String, distro: String) {
    let mut lines = BufReader::new(stderr).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => debug!(rule, distro, message = %line, "WSL agent 标准错误"),
            Ok(None) => break,
            Err(error) => {
                debug!(rule, distro, error = ?error, "读取 WSL agent 标准错误失败");
                break;
            }
        }
    }
}

fn retry_delay(failures: u32) -> Duration {
    let exponent = failures.saturating_sub(1).min(5);
    INITIAL_RETRY_DELAY
        .checked_mul(1_u32 << exponent)
        .unwrap_or(MAX_RETRY_DELAY)
        .min(MAX_RETRY_DELAY)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::retry_delay;

    #[test]
    fn bounds_agent_restart_backoff() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(2), Duration::from_secs(2));
        assert_eq!(retry_delay(6), Duration::from_secs(30));
        assert_eq!(retry_delay(100), Duration::from_secs(30));
    }
}
