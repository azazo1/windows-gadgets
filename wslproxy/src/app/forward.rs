use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{info, warn};

use super::access;
use super::config::{ForwardRule, ListenSource, Settings};
use super::network_adapter::{AdapterResolver, SystemAdapterResolver};
use super::target::{SystemWslControl, TargetResolver};

type SystemTargetResolver = TargetResolver<SystemWslControl>;

#[derive(Debug, Clone)]
struct ResolvedForward {
    rule: ForwardRule,
    listen: SocketAddrV4,
}

pub(crate) struct PreparedForwards {
    listeners: Vec<(ResolvedForward, TcpListener)>,
}

pub async fn prepare(settings: &Settings) -> Result<PreparedForwards> {
    let forwards = resolve_forwards(settings)?;
    let mut listeners = Vec::with_capacity(forwards.len());

    for forward in forwards {
        let listener = TcpListener::bind(forward.listen)
            .await
            .with_context(|| format!("无法绑定规则 `{}` 的监听端点 {}", forward.rule.name, forward.listen))?;
        info!(rule = %forward.rule.name, listen = %forward.listen, "已绑定转发监听端点");
        listeners.push((forward, listener));
    }

    Ok(PreparedForwards { listeners })
}

pub fn launch(
    prepared: PreparedForwards,
    settings: &Settings,
    shutdown: watch::Receiver<bool>,
    tasks: &mut JoinSet<Result<()>>,
) -> usize {
    let resolver = Arc::new(TargetResolver::new(SystemWslControl));
    let rules = prepared.listeners.len();
    for (forward, listener) in prepared.listeners {
        tasks.spawn(run_listener(
            listener,
            forward,
            Arc::clone(&resolver),
            settings.daemon.connect_timeout,
            settings.daemon.max_connections_per_rule,
            shutdown.clone(),
        ));
    }

    rules
}

pub async fn check(settings: &Settings) -> Result<usize> {
    let forwards = resolve_forwards(settings)?;
    let resolver = Arc::new(TargetResolver::new(SystemWslControl));

    for forward in &forwards {
        let (stream, target) = connect_target(
            Arc::clone(&resolver),
            &forward.rule,
            settings.daemon.connect_timeout,
        )
        .await?;
        drop(stream);
        info!(
            rule = %forward.rule.name,
            listen = %forward.listen,
            target = %target,
            "规则检查通过"
        );
    }

    Ok(forwards.len())
}

fn resolve_forwards(settings: &Settings) -> Result<Vec<ResolvedForward>> {
    let adapter_resolver = SystemAdapterResolver;
    let mut endpoints = HashSet::new();
    let mut forwards = Vec::with_capacity(settings.forwards.len());

    for rule in &settings.forwards {
        let address = match &rule.listen.source {
            ListenSource::Address(address) => *address,
            ListenSource::Interface(interface) => adapter_resolver
                .resolve_ipv4(interface)
                .with_context(|| format!("无法解析规则 `{}` 的网卡", rule.name))?,
        };
        let listen = SocketAddrV4::new(address, rule.listen.port);
        if !endpoints.insert(listen) {
            bail!("解析后存在重复监听端点: {listen}");
        }
        forwards.push(ResolvedForward {
            rule: rule.clone(),
            listen,
        });
    }

    Ok(forwards)
}

async fn run_listener(
    listener: TcpListener,
    forward: ResolvedForward,
    resolver: Arc<SystemTargetResolver>,
    connect_timeout: Duration,
    max_connections: usize,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let limiter = Arc::new(Semaphore::new(max_connections));

    loop {
        let accepted = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
                continue;
            }
            accepted = listener.accept() => accepted,
        }
        .with_context(|| format!("规则 `{}` 接受连接失败", forward.rule.name))?;

        let (stream, peer) = accepted;
        if !allows_peer(&forward.rule, peer) {
            info!(rule = %forward.rule.name, peer = %peer, "连接被来源规则拒绝");
            drop(stream);
            continue;
        }

        let permit = match Arc::clone(&limiter).try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                warn!(
                    rule = %forward.rule.name,
                    peer = %peer,
                    limit = max_connections,
                    "连接数已达到规则上限"
                );
                drop(stream);
                continue;
            }
        };

        let rule = forward.rule.clone();
        let resolver = Arc::clone(&resolver);
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = proxy_connection(stream, peer, resolver, rule, connect_timeout).await {
                warn!(error = ?error, peer = %peer, "转发会话失败");
            }
        });
    }

    info!(rule = %forward.rule.name, "转发监听器已停止");
    Ok(())
}

fn allows_peer(rule: &ForwardRule, peer: SocketAddr) -> bool {
    match peer.ip() {
        IpAddr::V4(address) => access::allows(&rule.allow, address),
        IpAddr::V6(_) => false,
    }
}

async fn proxy_connection(
    mut client: TcpStream,
    peer: SocketAddr,
    resolver: Arc<SystemTargetResolver>,
    rule: ForwardRule,
    connect_timeout: Duration,
) -> Result<()> {
    let started = Instant::now();
    let (mut target_stream, target) = connect_target(resolver, &rule, connect_timeout).await?;
    info!(rule = %rule.name, peer = %peer, target = %target, "已建立转发会话");

    let (client_to_target, target_to_client) =
        relay_connection(&mut client, &mut target_stream).await?;
    info!(
        rule = %rule.name,
        peer = %peer,
        target = %target,
        client_to_target,
        target_to_client,
        elapsed_ms = started.elapsed().as_millis(),
        "转发会话已结束"
    );
    Ok(())
}

async fn connect_target(
    resolver: Arc<SystemTargetResolver>,
    rule: &ForwardRule,
    connect_timeout: Duration,
) -> Result<(TcpStream, SocketAddrV4)> {
    let target = resolver.endpoint(rule).await?;
    match connect_once(target, connect_timeout).await {
        Ok(stream) => Ok((stream, target)),
        Err(first_error) => {
            warn!(
                rule = %rule.name,
                target = %target,
                error = ?first_error,
                "首次连接 WSL 目标失败, 将刷新地址后重试"
            );
            let retry_target = resolver.refresh_endpoint(rule).await?;
            let stream = connect_once(retry_target, connect_timeout)
                .await
                .with_context(|| {
                    format!("规则 `{}` 刷新 WSL 地址后的连接仍然失败", rule.name)
                })?;
            Ok((stream, retry_target))
        }
    }
}

async fn connect_once(target: SocketAddrV4, connect_timeout: Duration) -> Result<TcpStream> {
    match timeout(connect_timeout, TcpStream::connect(target)).await {
        Ok(result) => result.with_context(|| format!("无法连接 WSL 目标 {target}")),
        Err(_) => Err(anyhow!("连接 WSL 目标 {target} 超时")),
    }
}

async fn relay_connection(
    client: &mut TcpStream,
    target: &mut TcpStream,
) -> Result<(u64, u64)> {
    copy_bidirectional(client, target)
        .await
        .context("TCP 双向转发失败")
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Semaphore;

    use super::relay_connection;

    #[tokio::test]
    async fn relays_data_after_client_half_close() {
        let target_listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let target_address = target_listener.local_addr().unwrap();
        let target_task = tokio::spawn(async move {
            let (mut stream, _) = target_listener.accept().await.unwrap();
            let mut request = Vec::new();
            stream.read_to_end(&mut request).await.unwrap();
            assert_eq!(request, b"ping");
            stream.write_all(b"pong").await.unwrap();
            stream.shutdown().await.unwrap();
        });

        let proxy_listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let relay_task = tokio::spawn(async move {
            let (mut client, _) = proxy_listener.accept().await.unwrap();
            let mut target = TcpStream::connect(target_address).await.unwrap();
            relay_connection(&mut client, &mut target).await.unwrap();
        });

        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        client.shutdown().await.unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();

        assert_eq!(response, b"pong");
        relay_task.await.unwrap();
        target_task.await.unwrap();
    }

    #[test]
    fn connection_limiter_rejects_connections_over_the_limit() {
        let limiter = Semaphore::new(1);
        let permit = limiter.try_acquire().unwrap();
        assert!(limiter.try_acquire().is_err());
        drop(permit);
        assert!(limiter.try_acquire().is_ok());
    }
}
