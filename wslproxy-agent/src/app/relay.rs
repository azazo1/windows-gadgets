use std::future::poll_fn;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use futures::io::{AsyncRead, AsyncWrite};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{info, warn};
use yamux::{Config, Connection, Mode, Stream};

pub async fn serve<T>(
    io: T,
    listener: TcpListener,
    rule_name: String,
    max_connections: usize,
) -> Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut connection = Connection::new(io, Config::default(), Mode::Client);
    let limiter = Arc::new(Semaphore::new(max_connections));

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (client, peer) = accepted.context("接受 WSL loopback 连接失败")?;
                let permit = match Arc::clone(&limiter).try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        warn!(rule = %rule_name, peer = %peer, limit = max_connections, "反向规则连接数已达到上限");
                        drop(client);
                        continue;
                    }
                };

                let stream = open_stream(&mut connection).await?;
                let rule_name = rule_name.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Err(error) = relay_client(client, stream, rule_name, peer).await {
                        warn!(error = ?error, peer = %peer, "WSL 反向转发会话失败");
                    }
                });
            }
            inbound = next_stream(&mut connection) => {
                match inbound? {
                    Some(stream) => {
                        warn!(stream = %stream, "agent 收到了不应由 Windows 发起的逻辑流");
                        drop(stream);
                    }
                    None => bail!("Windows 端协议连接已关闭"),
                }
            }
        }
    }
}

async fn open_stream<T>(connection: &mut Connection<T>) -> Result<Stream>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    poll_fn(|context| connection.poll_new_outbound(context))
        .await
        .context("无法创建到 Windows 的逻辑流")
}

async fn next_stream<T>(connection: &mut Connection<T>) -> Result<Option<Stream>>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    poll_fn(|context| connection.poll_next_inbound(context))
        .await
        .transpose()
        .context("WSL agent 协议连接异常")
}

async fn relay_client(
    mut client: TcpStream,
    stream: Stream,
    rule_name: String,
    peer: std::net::SocketAddr,
) -> Result<()> {
    let started = Instant::now();
    let mut stream = stream.compat();
    let (client_to_windows, windows_to_client) = copy_bidirectional(&mut client, &mut stream)
        .await
        .context("WSL 到 Windows 的 TCP 双向转发失败")?;
    info!(
        rule = %rule_name,
        peer = %peer,
        client_to_windows,
        windows_to_client,
        elapsed_ms = started.elapsed().as_millis(),
        "WSL 反向转发会话已结束"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use anyhow::Result;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional, duplex};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::{Barrier, mpsc};
    use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
    use yamux::{Config, Connection, Mode};

    use super::{next_stream, serve};

    #[tokio::test]
    async fn forwards_a_half_closed_loopback_connection_over_yamux() {
        let target_listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let target_address = target_listener.local_addr().unwrap();
        let target_task = tokio::spawn(async move {
            let (mut target, _) = target_listener.accept().await.unwrap();
            let mut request = Vec::new();
            target.read_to_end(&mut request).await.unwrap();
            assert_eq!(request, b"ping");
            target.write_all(b"pong").await.unwrap();
            target.shutdown().await.unwrap();
        });

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let listen_address = listener.local_addr().unwrap();
        let (agent_io, host_io) = duplex(64 * 1024);
        let agent_task = tokio::spawn(serve(
            agent_io.compat(),
            listener,
            "test".to_string(),
            2,
        ));

        let (done_sender, mut done_receiver) = mpsc::channel::<Result<()>>(1);
        let host_task = tokio::spawn(async move {
            let mut connection = Connection::new(host_io.compat(), Config::default(), Mode::Server);
            loop {
                let stream = next_stream(&mut connection).await?.expect("协议连接意外关闭");
                let target = TcpStream::connect(target_address).await?;
                let sender = done_sender.clone();
                tokio::spawn(async move {
                    let mut stream = stream.compat();
                    let mut target = target;
                    let result = copy_bidirectional(&mut stream, &mut target)
                        .await
                        .map(|_| ())
                        .map_err(Into::into);
                    let _ = sender.send(result).await;
                });
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        });

        let mut client = TcpStream::connect(listen_address).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        client.shutdown().await.unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert_eq!(response, b"pong");

        done_receiver.recv().await.unwrap().unwrap();
        target_task.await.unwrap();
        host_task.abort();
        agent_task.abort();
        let _ = host_task.await;
        let _ = agent_task.await;
    }

    #[tokio::test]
    async fn multiplexes_multiple_loopback_connections() {
        let target_listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let target_address = target_listener.local_addr().unwrap();
        let target_task = tokio::spawn(async move {
            let mut sessions = tokio::task::JoinSet::new();
            for _ in 0..2 {
                let (mut target, _) = target_listener.accept().await.unwrap();
                sessions.spawn(async move {
                    let mut request = [0_u8; 4];
                    target.read_exact(&mut request).await.unwrap();
                    target.write_all(&request).await.unwrap();
                    target.shutdown().await.unwrap();
                });
            }
            while let Some(session) = sessions.join_next().await {
                session.unwrap();
            }
        });

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let listen_address = listener.local_addr().unwrap();
        let (agent_io, host_io) = duplex(64 * 1024);
        let agent_task = tokio::spawn(serve(
            agent_io.compat(),
            listener,
            "test".to_string(),
            2,
        ));

        let (session_sender, mut session_receiver) = mpsc::channel::<Result<()>>(2);
        let host_task = tokio::spawn(async move {
            let mut connection = Connection::new(host_io.compat(), Config::default(), Mode::Server);
            loop {
                let stream = next_stream(&mut connection).await?.expect("协议连接意外关闭");
                let target = TcpStream::connect(target_address).await?;
                let sender = session_sender.clone();
                tokio::spawn(async move {
                    let mut stream = stream.compat();
                    let mut target = target;
                    let result = copy_bidirectional(&mut stream, &mut target)
                        .await
                        .map(|_| ())
                        .map_err(anyhow::Error::from);
                    let _ = sender.send(result).await;
                });
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        });

        let barrier = Arc::new(Barrier::new(2));
        let first_barrier = Arc::clone(&barrier);
        let first = tokio::spawn(async move {
            first_barrier.wait().await;
            round_trip(listen_address, *b"one!").await
        });
        let second_barrier = Arc::clone(&barrier);
        let second = tokio::spawn(async move {
            second_barrier.wait().await;
            round_trip(listen_address, *b"two!").await
        });

        first.await.unwrap();
        second.await.unwrap();
        session_receiver.recv().await.unwrap().unwrap();
        session_receiver.recv().await.unwrap().unwrap();
        target_task.await.unwrap();
        host_task.abort();
        agent_task.abort();
        let _ = host_task.await;
        let _ = agent_task.await;
    }

    async fn round_trip(address: std::net::SocketAddr, request: [u8; 4]) {
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(&request).await.unwrap();
        client.shutdown().await.unwrap();
        let mut response = [0_u8; 4];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(response, request);
    }
}
