use std::io;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

pub const PROTOCOL_VERSION: u16 = 1;

const HELLO_MAGIC: [u8; 8] = *b"WSLPXY01";
const READY_MAGIC: [u8; 8] = *b"WSLRDY01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentHello {
    pub listen_port: u16,
}

pub async fn agent_handshake<T>(io: &mut T, listen_port: u16) -> Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    io.write_all(&HELLO_MAGIC)
        .await
        .context("无法发送 agent 握手标识")?;
    io.write_u16(PROTOCOL_VERSION)
        .await
        .context("无法发送 agent 协议版本")?;
    io.write_u16(listen_port)
        .await
        .context("无法发送 agent 监听端口")?;
    io.flush().await.context("无法刷新 agent 握手数据")?;

    let mut magic = [0_u8; READY_MAGIC.len()];
    io.read_exact(&mut magic)
        .await
        .context("未收到 Windows 握手确认")?;
    if magic != READY_MAGIC {
        bail!("Windows 返回了无效的握手确认");
    }

    let version = io
        .read_u16()
        .await
        .context("无法读取 Windows 协议版本")?;
    if version != PROTOCOL_VERSION {
        bail!(
            "Windows 协议版本不匹配: agent={}, windows={}",
            PROTOCOL_VERSION,
            version
        );
    }

    Ok(())
}

pub async fn host_handshake<T>(io: &mut T, expected_port: u16) -> Result<AgentHello>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut magic = [0_u8; HELLO_MAGIC.len()];
    io.read_exact(&mut magic)
        .await
        .context("未收到 WSL agent 握手标识")?;
    if magic != HELLO_MAGIC {
        bail!("WSL agent 返回了无效的握手标识");
    }

    let version = io
        .read_u16()
        .await
        .context("无法读取 WSL agent 协议版本")?;
    if version != PROTOCOL_VERSION {
        bail!(
            "WSL agent 协议版本不匹配: windows={}, agent={}",
            PROTOCOL_VERSION,
            version
        );
    }

    let listen_port = io
        .read_u16()
        .await
        .context("无法读取 WSL agent 监听端口")?;
    if listen_port != expected_port {
        bail!(
            "WSL agent 监听端口不匹配: expected={}, actual={}",
            expected_port,
            listen_port
        );
    }

    io.write_all(&READY_MAGIC)
        .await
        .context("无法发送 Windows 握手确认")?;
    io.write_u16(PROTOCOL_VERSION)
        .await
        .context("无法发送 Windows 协议版本")?;
    io.flush().await.context("无法刷新 Windows 握手确认")?;

    Ok(AgentHello { listen_port })
}

#[derive(Debug)]
pub struct SplitIo<R, W> {
    reader: R,
    writer: W,
}

impl<R, W> SplitIo<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }

    pub fn into_inner(self) -> (R, W) {
        (self.reader, self.writer)
    }
}

impl<R, W> AsyncRead for SplitIo<R, W>
where
    R: AsyncRead + Unpin,
    W: Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.reader).poll_read(context, buffer)
    }
}

impl<R, W> AsyncWrite for SplitIo<R, W>
where
    R: Unpin,
    W: AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        Pin::new(&mut this.writer).poll_write(context, buffer)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.writer).poll_flush(context)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.writer).poll_shutdown(context)
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::duplex;

    use super::{agent_handshake, host_handshake};

    #[tokio::test]
    async fn completes_a_matching_handshake() {
        let (mut agent, mut host) = duplex(64);
        let (agent_result, host_result) = tokio::join!(
            agent_handshake(&mut agent, 20002),
            host_handshake(&mut host, 20002),
        );

        assert!(agent_result.is_ok());
        assert_eq!(host_result.unwrap().listen_port, 20002);
    }

    #[tokio::test]
    async fn rejects_an_unexpected_listen_port() {
        let (mut agent, mut host) = duplex(64);
        let host_task = tokio::spawn(async move { host_handshake(&mut host, 20003).await });
        let agent_result = agent_handshake(&mut agent, 20002).await;
        let host_result = host_task.await.unwrap();

        assert!(host_result.is_err());
        assert!(agent_result.is_err());
    }
}
