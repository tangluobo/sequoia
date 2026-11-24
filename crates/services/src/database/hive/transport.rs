//! Direct/SSH TCP, SASL PLAIN framing and HTTP use the same bounded Thrift codec.
use super::{
    config::Config,
    wire::{self, Value},
};
use anyhow::{Context as _, Result};
use std::{
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader, ReadBuf};
pub(super) trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
pub(super) type Socket = Box<dyn Io>;
pub(super) enum Transport {
    Tcp(Stream),
    Http(Box<super::http::Transport>),
}
pub(super) struct Stream {
    socket: BufReader<Socket>,
    sasl: bool,
    layer: Option<Box<super::sasl::Layer>>,
    encrypted: Vec<u8>,
    filled: usize,
    plain: Vec<u8>,
    offset: usize,
    header: [u8; 4],
    used: usize,
    remaining: usize,
}
impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if !self.sasl {
            return Pin::new(&mut self.socket).poll_read(cx, output);
        }
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if self.offset < self.plain.len() {
            let count = (self.plain.len() - self.offset).min(output.remaining());
            output.put_slice(&self.plain[self.offset..self.offset + count]);
            self.offset += count;
            if self.offset == self.plain.len() {
                if self.plain.capacity() > 256 * 1024 {
                    self.plain = Vec::new();
                } else {
                    self.plain.clear();
                }
                self.offset = 0;
            }
            return Poll::Ready(Ok(()));
        }
        while self.remaining == 0 {
            let this = &mut *self;
            let mut header = ReadBuf::new(&mut this.header[this.used..]);
            ready!(Pin::new(&mut this.socket).poll_read(cx, &mut header))?;
            let count = header.filled().len();
            if count == 0 {
                return Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()));
            }
            this.used += count;
            if this.used < 4 {
                continue;
            }
            this.remaining = u32::from_be_bytes(this.header) as usize;
            this.used = 0;
            if this.remaining == 0 || this.remaining > 8 * 1024 * 1024 {
                return Poll::Ready(Err(std::io::Error::other("Hive SASL 数据帧长度无效")));
            }
        }
        if self.layer.is_some() {
            let length = self.remaining;
            self.encrypted.resize(length, 0);
            while self.filled < length {
                let this = &mut *self;
                let mut buffer = ReadBuf::new(&mut this.encrypted[this.filled..]);
                ready!(Pin::new(&mut this.socket).poll_read(cx, &mut buffer))?;
                let count = buffer.filled().len();
                if count == 0 {
                    return Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()));
                }
                this.filled += count;
            }
            let this = &mut *self;
            let decoded = this
                .layer
                .as_mut()
                .expect("protected frame")
                .unwrap(&this.encrypted)
                .map_err(std::io::Error::other)?;
            this.remaining = 0;
            this.filled = 0;
            if this.encrypted.capacity() > 256 * 1024 {
                this.encrypted = Vec::new();
            } else {
                this.encrypted.clear();
            }
            let count = decoded.len().min(output.remaining());
            output.put_slice(&decoded[..count]);
            if count < decoded.len() {
                this.plain = decoded;
                this.offset = count;
            }
            return Poll::Ready(Ok(()));
        }
        let count = self.remaining.min(output.remaining());
        let mut buffer = ReadBuf::new(output.initialize_unfilled_to(count));
        ready!(Pin::new(&mut self.socket).poll_read(cx, &mut buffer))?;
        let count = buffer.filled().len();
        if count == 0 {
            return Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()));
        }
        self.remaining -= count;
        output.advance(count);
        Poll::Ready(Ok(()))
    }
}
impl Transport {
    pub async fn connect(config: &Config) -> Result<Self> {
        let sasl = config.sasl()?;
        let socket = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::net::TcpStream::connect((config.endpoint.0.as_str(), config.endpoint.1)),
        )
        .await
        .context("Hive 连接超时")??;
        socket.set_nodelay(true)?;
        let mut socket: Socket = if config.ssl() {
            let name = rustls::pki_types::ServerName::try_from(config.host.clone())?;
            Box::new(
                tokio_rustls::TlsConnector::from(config.tls().await?)
                    .connect(name, socket)
                    .await?,
            )
        } else {
            Box::new(socket)
        };
        if config.http() {
            return Ok(Self::Http(Box::new(
                super::http::Transport::connect(socket, config).await?,
            )));
        }
        let layer = if sasl {
            super::sasl::authenticate(&mut socket, config).await?
        } else {
            None
        };
        Ok(Self::Tcp(Stream {
            socket: BufReader::with_capacity(16 * 1024, socket),
            sasl,
            layer: layer.map(Box::new),
            encrypted: Vec::new(),
            filled: 0,
            plain: Vec::new(),
            offset: 0,
            header: [0; 4],
            used: 0,
            remaining: 0,
        }))
    }
    pub async fn rpc(&mut self, method: &str, sequence: i32, request: Value) -> Result<Value> {
        let bytes = wire::request(method, sequence, request)?;
        match self {
            Self::Tcp(stream) => {
                if let Some(layer) = &mut stream.layer {
                    for chunk in bytes.chunks(layer.payload()) {
                        let token = layer.wrap(chunk)?;
                        stream
                            .socket
                            .get_mut()
                            .write_u32(token.len() as u32)
                            .await?;
                        stream.socket.get_mut().write_all(&token).await?;
                    }
                    stream.socket.get_mut().flush().await?;
                    return wire::response(stream, method, sequence).await;
                }
                if stream.sasl {
                    stream
                        .socket
                        .get_mut()
                        .write_u32(u32::try_from(bytes.len())?)
                        .await?;
                }
                stream.socket.get_mut().write_all(&bytes).await?;
                stream.socket.get_mut().flush().await?;
                wire::response(stream, method, sequence).await
            }
            Self::Http(transport) => {
                let bytes = transport.request(bytes).await?;
                wire::response(&mut bytes.as_slice(), method, sequence).await
            }
        }
    }
}
