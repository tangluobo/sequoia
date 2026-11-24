//! Bounded packet adapter. It owns no tasks and releases all buffers on drop.
use super::{Shared, packet};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
const LIMIT: usize = 65535;
pub struct Stream<S> {
    inner: S,
    security: Shared,
    input: Vec<u8>,
    decoded: Vec<u8>,
    offset: usize,
    output: Vec<u8>,
    sent: usize,
    pending: Vec<u8>,
}
impl<S> Stream<S> {
    pub fn new(inner: S, security: Shared) -> Self {
        Self {
            inner,
            security,
            input: Vec::new(),
            decoded: Vec::new(),
            offset: 0,
            output: Vec::new(),
            sent: 0,
            pending: Vec::new(),
        }
    }
    fn bad(error: impl std::fmt::Display) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, error.to_string())
    }
}
impl<S: AsyncWrite + Unpin> Stream<S> {
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        loop {
            while self.sent < self.output.len() {
                let count =
                    ready!(Pin::new(&mut self.inner).poll_write(cx, &self.output[self.sent..]))?;
                if count == 0 {
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                self.sent += count;
            }
            self.output.clear();
            self.sent = 0;
            let Some(length) = packet::length(&self.pending).map_err(Self::bad)? else {
                return Poll::Ready(Ok(()));
            };
            if length > self.pending.len() {
                return Poll::Ready(Ok(()));
            }
            self.output = self
                .security
                .lock()
                .map_err(Self::bad)?
                .outgoing(&self.pending[..length])
                .map_err(Self::bad)?;
            self.pending.drain(..length);
        }
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for Stream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        // write_all may have accepted a complete frame while its socket write
        // was pending. The next read must flush it before awaiting a reply.
        // Both endpoints may have full write buffers. Make progress on reads
        // even while a pending write is blocked, and retain its write waker.
        if let Poll::Ready(result) = this.drain(cx) {
            result?;
        }
        loop {
            if this.offset < this.decoded.len() {
                let count = out.remaining().min(this.decoded.len() - this.offset);
                out.put_slice(&this.decoded[this.offset..this.offset + count]);
                this.offset += count;
                if this.offset == this.decoded.len() {
                    this.decoded.clear();
                    this.offset = 0;
                }
                return Poll::Ready(Ok(()));
            }
            let length = packet::length(&this.input).map_err(Self::bad)?;
            if let Some(length) = length
                && length <= this.input.len()
            {
                this.decoded = this
                    .security
                    .lock()
                    .map_err(Self::bad)?
                    .incoming(&this.input[..length])
                    .map_err(Self::bad)?;
                this.input.drain(..length);
                continue;
            }
            if this.input.len() >= LIMIT {
                return Poll::Ready(Err(Self::bad("RDP 接收缓冲超过限制")));
            }
            let mut bytes = [0u8; 8192];
            let mut buffer = ReadBuf::new(&mut bytes[..8192.min(LIMIT - this.input.len())]);
            ready!(Pin::new(&mut this.inner).poll_read(cx, &mut buffer))?;
            if buffer.filled().is_empty() {
                return Poll::Ready(if this.input.is_empty() {
                    Ok(())
                } else {
                    Err(io::ErrorKind::UnexpectedEof.into())
                });
            }
            this.input.extend_from_slice(buffer.filled());
        }
    }
}
impl<S: AsyncWrite + Unpin> AsyncWrite for Stream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        ready!(this.drain(cx))?;
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let count = bytes.len().min(LIMIT - this.pending.len());
        if count == 0 {
            return Poll::Ready(Err(Self::bad("RDP 发送缓冲超过限制")));
        }
        this.pending.extend_from_slice(&bytes[..count]);
        if let Poll::Ready(result) = this.drain(cx) {
            result?;
        }
        Poll::Ready(Ok(count))
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        ready!(this.drain(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}
