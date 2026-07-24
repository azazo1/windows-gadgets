use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::pin::Pin;
use std::process::Output;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::warn;

use super::config::{ForwardRule, TargetMode};

const INITIAL_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait WslControl: Send + Sync {
    fn ensure_started<'a>(&'a self, distro: &'a str) -> BoxFuture<'a, Result<()>>;
    fn resolve_ipv4<'a>(&'a self, distro: &'a str) -> BoxFuture<'a, Result<Ipv4Addr>>;
}

#[derive(Debug, Default)]
pub struct SystemWslControl;

impl WslControl for SystemWslControl {
    fn ensure_started<'a>(&'a self, distro: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let output = Command::new("wsl.exe")
                .args(["--distribution", distro, "--exec", "true"])
                .output()
                .await
                .with_context(|| format!("无法启动 WSL 发行版 `{distro}`"))?;
            check_wsl_output("启动", distro, output).map(|_| ())
        })
    }

    fn resolve_ipv4<'a>(&'a self, distro: &'a str) -> BoxFuture<'a, Result<Ipv4Addr>> {
        Box::pin(async move {
            let output = Command::new("wsl.exe")
                .args([
                    "--distribution",
                    distro,
                    "--exec",
                    "ip",
                    "-j",
                    "-4",
                    "address",
                    "show",
                    "scope",
                    "global",
                ])
                .output()
                .await
                .with_context(|| format!("无法查询 WSL 发行版 `{distro}` 的地址"))?;
            check_wsl_output("查询地址", distro, output)
                .and_then(|output| parse_wsl_ipv4(&output.stdout))
        })
    }
}

pub struct TargetResolver<C> {
    control: Arc<C>,
    state: Mutex<ResolverState>,
}

impl<C> TargetResolver<C>
where
    C: WslControl,
{
    pub fn new(control: C) -> Self {
        Self {
            control: Arc::new(control),
            state: Mutex::new(ResolverState::default()),
        }
    }

    pub async fn endpoint(&self, rule: &ForwardRule) -> Result<SocketAddrV4> {
        let address = match rule.target.mode {
            TargetMode::WslIp => self.wsl_ipv4(&rule.distro).await?,
            TargetMode::Localhost => {
                self.ensure_started(&rule.distro).await?;
                Ipv4Addr::LOCALHOST
            }
        };
        Ok(SocketAddrV4::new(address, rule.target.port))
    }

    pub async fn refresh_endpoint(&self, rule: &ForwardRule) -> Result<SocketAddrV4> {
        self.invalidate(&rule.distro).await;
        self.endpoint(rule).await
    }

    pub async fn invalidate(&self, distro: &str) {
        let mut state = self.state.lock().await;
        state.addresses.remove(distro);
        state.started.remove(distro);
    }

    async fn ensure_started(&self, distro: &str) -> Result<()> {
        let mut state = self.state.lock().await;
        if state.started.contains(distro) {
            return Ok(());
        }
        check_retry_window(distro, &state)?;

        match self.control.ensure_started(distro).await {
            Ok(()) => {
                state.started.insert(distro.to_string());
                state.failures.remove(distro);
                Ok(())
            }
            Err(error) => {
                warn!(
                    distro,
                    error = ?error,
                    "启动 WSL 发行版失败, 将按退避策略重试"
                );
                record_failure(distro, &mut state);
                Err(error)
            }
        }
    }

    async fn wsl_ipv4(&self, distro: &str) -> Result<Ipv4Addr> {
        let mut state = self.state.lock().await;
        if let Some(address) = state.addresses.get(distro) {
            return Ok(*address);
        }
        check_retry_window(distro, &state)?;

        match self.control.resolve_ipv4(distro).await {
            Ok(address) => {
                state.addresses.insert(distro.to_string(), address);
                state.started.insert(distro.to_string());
                state.failures.remove(distro);
                Ok(address)
            }
            Err(error) => {
                warn!(
                    distro,
                    error = ?error,
                    "查询 WSL IPv4 地址失败, 将按退避策略重试"
                );
                record_failure(distro, &mut state);
                Err(error)
            }
        }
    }
}

#[derive(Default)]
struct ResolverState {
    addresses: HashMap<String, Ipv4Addr>,
    started: HashSet<String>,
    failures: HashMap<String, RetryState>,
}

struct RetryState {
    failures: u32,
    retry_after: Instant,
}

fn check_retry_window(distro: &str, state: &ResolverState) -> Result<()> {
    let Some(retry) = state.failures.get(distro) else {
        return Ok(());
    };
    let now = Instant::now();
    if now >= retry.retry_after {
        return Ok(());
    }

    let remaining = retry.retry_after.duration_since(now);
    bail!("WSL 发行版 `{distro}` 暂不可用, 将在 {:.1} 秒后重试", remaining.as_secs_f64());
}

fn record_failure(distro: &str, state: &mut ResolverState) {
    let failures = state
        .failures
        .get(distro)
        .map_or(1, |previous| previous.failures.saturating_add(1));
    let exponent = failures.saturating_sub(1).min(5);
    let delay = INITIAL_RETRY_DELAY
        .checked_mul(1_u32 << exponent)
        .unwrap_or(MAX_RETRY_DELAY)
        .min(MAX_RETRY_DELAY);
    state.failures.insert(
        distro.to_string(),
        RetryState {
            failures,
            retry_after: Instant::now() + delay,
        },
    );
}

fn check_wsl_output(action: &str, distro: &str, output: Output) -> Result<Output> {
    if output.status.success() {
        return Ok(output);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else {
        "未返回错误信息".to_string()
    };
    bail!("WSL 发行版 `{distro}` {action}失败: {detail}")
}

#[derive(Debug, Deserialize)]
struct WslInterface {
    #[serde(default)]
    operstate: String,
    #[serde(default)]
    addr_info: Vec<WslAddress>,
}

#[derive(Debug, Deserialize)]
struct WslAddress {
    #[serde(default)]
    family: String,
    #[serde(default)]
    local: String,
    #[serde(default)]
    scope: String,
}

fn parse_wsl_ipv4(source: &[u8]) -> Result<Ipv4Addr> {
    let interfaces: Vec<WslInterface> =
        serde_json::from_slice(source).context("无法解析 WSL 地址 JSON")?;

    interfaces
        .iter()
        .filter(|interface| interface.operstate.eq_ignore_ascii_case("up"))
        .flat_map(|interface| interface.addr_info.iter())
        .filter(|address| address.family == "inet" && address.scope == "global")
        .find_map(|address| address.local.parse::<Ipv4Addr>().ok())
        .filter(|address| !address.is_unspecified() && !address.is_loopback())
        .ok_or_else(|| anyhow!("WSL 未返回可用的全局 IPv4 地址"))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::net::Ipv4Addr;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use anyhow::{Result, anyhow};

    use super::{BoxFuture, TargetResolver, WslControl, parse_wsl_ipv4};
    use crate::app::config::{ForwardRule, ListenSource, ListenSpec, TargetMode, TargetSpec};

    struct FakeWslControl {
        starts: AtomicUsize,
        resolutions: AtomicUsize,
        addresses: StdMutex<VecDeque<Ipv4Addr>>,
    }

    impl FakeWslControl {
        fn with_addresses(addresses: impl IntoIterator<Item = Ipv4Addr>) -> Self {
            Self {
                starts: AtomicUsize::new(0),
                resolutions: AtomicUsize::new(0),
                addresses: StdMutex::new(addresses.into_iter().collect()),
            }
        }
    }

    impl WslControl for FakeWslControl {
        fn ensure_started<'a>(&'a self, _distro: &'a str) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.starts.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }

        fn resolve_ipv4<'a>(&'a self, _distro: &'a str) -> BoxFuture<'a, Result<Ipv4Addr>> {
            Box::pin(async move {
                self.resolutions.fetch_add(1, Ordering::SeqCst);
                self.addresses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .ok_or_else(|| anyhow!("缺少测试地址"))
            })
        }
    }

    fn rule(mode: TargetMode) -> ForwardRule {
        ForwardRule {
            name: "test".to_string(),
            distro: "Ubuntu".to_string(),
            allow: Vec::new(),
            listen: ListenSpec {
                source: ListenSource::Address(Ipv4Addr::new(127, 0, 0, 1)),
                port: 3000,
            },
            target: TargetSpec { mode, port: 2222 },
        }
    }

    #[test]
    fn parses_the_first_up_global_ipv4_address() {
        let source = br#"[
            {"operstate":"DOWN","addr_info":[{}, {"family":"inet","local":"10.0.0.2","scope":"global"}]},
            {"operstate":"UP","addr_info":[
                {"family":"inet","local":"127.0.0.1","scope":"host"},
                {"family":"inet","local":"172.22.187.152","scope":"global"}
            ]}
        ]"#;

        assert_eq!(
            parse_wsl_ipv4(source).unwrap(),
            Ipv4Addr::new(172, 22, 187, 152)
        );
    }

    #[tokio::test]
    async fn caches_wsl_ip_and_refreshes_after_invalidation() {
        let control = FakeWslControl::with_addresses([
            Ipv4Addr::new(172, 22, 0, 2),
            Ipv4Addr::new(172, 22, 0, 3),
        ]);
        let resolver = TargetResolver::new(control);
        let rule = rule(TargetMode::WslIp);

        assert_eq!(resolver.endpoint(&rule).await.unwrap().ip(), &Ipv4Addr::new(172, 22, 0, 2));
        assert_eq!(resolver.endpoint(&rule).await.unwrap().ip(), &Ipv4Addr::new(172, 22, 0, 2));
        resolver.invalidate(&rule.distro).await;
        assert_eq!(resolver.endpoint(&rule).await.unwrap().ip(), &Ipv4Addr::new(172, 22, 0, 3));
    }

    #[tokio::test]
    async fn localhost_target_starts_wsl_without_resolving_an_ip() {
        let control = FakeWslControl::with_addresses([]);
        let resolver = TargetResolver::new(control);
        let rule = rule(TargetMode::Localhost);

        assert_eq!(resolver.endpoint(&rule).await.unwrap().ip(), &Ipv4Addr::LOCALHOST);
    }
}
