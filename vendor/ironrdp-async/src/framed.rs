use ironrdp_core::MonotonicInstant;
use std::io;
use std::{collections::VecDeque, time::Instant};

use bytes::{Bytes, BytesMut};
use ironrdp_connector::{ConnectorResult, Sequence, Written};
use ironrdp_core::WriteBuf;
use ironrdp_pdu::PduHint;
use tracing::{debug, trace};

// TODO: investigate if we could use static async fn / return position impl trait in traits when stabilized:
// https://github.com/rust-lang/rust/issues/91611

pub trait FramedRead {
    type ReadFut<'read>: Future<Output = io::Result<usize>> + 'read
    where
        Self: 'read;

    /// Reads from stream and fills internal buffer
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If you use it as the event in a
    /// `tokio::select!` statement and some other branch
    /// completes first, then it is guaranteed that no data was read.
    fn read<'a>(&'a mut self, buf: &'a mut BytesMut) -> Self::ReadFut<'a>;
}

pub trait FramedWrite {
    type WriteAllFut<'write>: Future<Output = io::Result<()>> + 'write
    where
        Self: 'write;

    /// Writes an entire buffer into this stream.
    ///
    /// # Cancel safety
    ///
    /// This method is not cancellation safe. If it is used as the event
    /// in a `tokio::select!` statement and some other
    /// branch completes first, then the provided buffer may have been
    /// partially written, but future calls to `write_all` will start over
    /// from the beginning of the buffer.
    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteAllFut<'a>;
}

pub trait StreamWrapper: Sized {
    type InnerStream;

    fn from_inner(stream: Self::InnerStream) -> Self;

    fn into_inner(self) -> Self::InnerStream;

    fn get_inner(&self) -> &Self::InnerStream;

    fn get_inner_mut(&mut self) -> &mut Self::InnerStream;
}

pub struct Framed<S> {
    stream: S,
    buf: BytesMut,
    clock: Instant,
    arrivals: VecDeque<(usize, MonotonicInstant)>,
    last_read_at: Option<MonotonicInstant>,
}

impl<S> Framed<S> {
    /// Transport arrival of the last returned frame's last byte, excluding decoding time.
    pub fn last_read_at(&self) -> Option<MonotonicInstant> {
        self.last_read_at
    }
    fn consume_arrivals(&mut self, mut count: usize) {
        while count > 0 {
            let Some((bytes, at)) = self.arrivals.front_mut() else {
                break;
            };
            self.last_read_at = Some(*at);
            let consumed = count.min(*bytes);
            *bytes -= consumed;
            count -= consumed;
            if *bytes == 0 {
                self.arrivals.pop_front();
            }
        }
    }
    fn record_arrival(&mut self, len: usize, at: MonotonicInstant) {
        if len == 0 {
            return;
        }
        let full = self.arrivals.len() >= 64;
        if let Some((bytes, last)) = self.arrivals.back_mut() {
            if *last == at || full {
                *bytes += len;
                *last = at;
                return;
            }
        }
        self.arrivals.push_back((len, at));
    }
    pub fn peek(&self) -> &[u8] {
        &self.buf
    }
}

impl<S> Framed<S>
where
    S: StreamWrapper,
{
    pub fn new(stream: S::InnerStream) -> Self {
        Self::new_with_leftover(stream, BytesMut::new())
    }

    pub fn new_with_leftover(stream: S::InnerStream, leftover: BytesMut) -> Self {
        Self {
            stream: S::from_inner(stream),
            arrivals: if leftover.is_empty() {
                VecDeque::new()
            } else {
                VecDeque::from([(leftover.len(), MonotonicInstant::from_millis(0))])
            },
            buf: leftover,
            clock: Instant::now(),
            last_read_at: None,
        }
    }

    pub fn into_inner(self) -> (S::InnerStream, BytesMut) {
        (self.stream.into_inner(), self.buf)
    }

    pub fn into_inner_no_leftover(self) -> S::InnerStream {
        let (stream, leftover) = self.into_inner();
        debug_assert_eq!(leftover.len(), 0, "unexpected leftover");
        stream
    }

    pub fn get_inner(&self) -> (&S::InnerStream, &BytesMut) {
        (self.stream.get_inner(), &self.buf)
    }

    pub fn get_inner_mut(&mut self) -> (&mut S::InnerStream, &mut BytesMut) {
        (self.stream.get_inner_mut(), &mut self.buf)
    }
}

impl<S> Framed<S>
where
    S: FramedRead,
{
    /// Accumulates at least `length` bytes and returns exactly `length` bytes, keeping the leftover in the internal buffer.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If you use it as the event in a
    /// `tokio::select!` statement and some other branch
    /// completes first, then it is safe to drop the future and re-create it later.
    /// Data may have been read, but it will be stored in the internal buffer.
    pub(crate) async fn read_exact(&mut self, length: usize) -> io::Result<BytesMut> {
        loop {
            if self.buf.len() >= length {
                self.consume_arrivals(length);
                return Ok(self.buf.split_to(length));
            } else {
                self.buf
                    .reserve(length.checked_sub(self.buf.len()).expect("length > self.buf.len()"));
            }

            let len = self.read().await?;

            // Handle EOF
            if len == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "not enough bytes"));
            }
        }
    }

    /// Reads a standard RDP PDU frame.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If you use it as the event in a
    /// `tokio::select!` statement and some other branch
    /// completes first, then it is safe to drop the future and re-create it later.
    /// Data may have been read, but it will be stored in the internal buffer.
    pub async fn read_pdu(&mut self) -> io::Result<(ironrdp_pdu::Action, BytesMut)> {
        loop {
            // Try decoding and see if a frame has been received already
            match ironrdp_pdu::find_size(self.peek()) {
                Ok(Some(pdu_info)) => {
                    let frame = self.read_exact(pdu_info.length).await?;

                    return Ok((pdu_info.action, frame));
                }
                Ok(None) => {
                    let len = self.read().await?;

                    // Handle EOF
                    if len == 0 {
                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "not enough bytes"));
                    }
                }
                Err(e) => return Err(io::Error::other(e)),
            };
        }
    }

    /// Reads a frame using the provided PduHint.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If you use it as the event in a
    /// `tokio::select!` statement and some other branch
    /// completes first, then it is safe to drop the future and re-create it later.
    /// Data may have been read, but it will be stored in the internal buffer.
    pub async fn read_by_hint(&mut self, hint: &dyn PduHint) -> io::Result<Bytes> {
        loop {
            match hint.find_size(self.peek()).map_err(io::Error::other)? {
                Some((matched, length)) => {
                    let bytes = self.read_exact(length).await?.freeze();
                    if matched {
                        return Ok(bytes);
                    } else {
                        debug!("Received and lost an unexpected PDU");
                    }
                }
                None => {
                    let len = self.read().await?;

                    // Handle EOF
                    if len == 0 {
                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "not enough bytes"));
                    }
                }
            };
        }
    }

    /// Reads from stream and fills internal buffer, returning how many bytes were read.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe. If you use it as the event in a
    /// `tokio::select!` statement and some other branch
    /// completes first, then it is guaranteed that no data was read.
    async fn read(&mut self) -> io::Result<usize> {
        let len = self.stream.read(&mut self.buf).await?;
        let millis = u64::try_from(self.clock.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.record_arrival(len, MonotonicInstant::from_millis(millis));
        Ok(len)
    }
}

impl<S> FramedWrite for Framed<S>
where
    S: FramedWrite,
{
    type WriteAllFut<'write>
        = S::WriteAllFut<'write>
    where
        Self: 'write;

    /// Attempts to write an entire buffer into this `Framed`’s stream.
    ///
    /// # Cancel safety
    ///
    /// This method is not cancellation safe. If it is used as the event
    /// in a `tokio::select!` statement and some other
    /// branch completes first, then the provided buffer may have been
    /// partially written, but future calls to `write_all` will start over
    /// from the beginning of the buffer.
    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> Self::WriteAllFut<'a> {
        self.stream.write_all(buf)
    }
}

pub async fn single_sequence_step<S>(
    framed: &mut Framed<S>,
    sequence: &mut dyn Sequence,
    buf: &mut WriteBuf,
) -> ConnectorResult<()>
where
    S: FramedWrite + FramedRead,
{
    buf.clear();
    let written = single_sequence_step_read(framed, sequence, buf).await?;
    single_sequence_step_write(framed, buf, written).await
}

pub async fn single_sequence_step_read<S>(
    framed: &mut Framed<S>,
    sequence: &mut dyn Sequence,
    buf: &mut WriteBuf,
) -> ConnectorResult<Written>
where
    S: FramedRead,
{
    buf.clear();

    if let Some(next_pdu_hint) = sequence.next_pdu_hint() {
        debug!(
            connector.state = sequence.state().name(),
            hint = ?next_pdu_hint,
            "Wait for PDU"
        );

        let pdu = framed
            .read_by_hint(next_pdu_hint)
            .await
            .map_err(|e| ironrdp_connector::custom_err!("read frame by hint", e))?;

        trace!(length = pdu.len(), "PDU received");

        sequence.step(&pdu, buf)
    } else {
        sequence.step_no_input(buf)
    }
}

async fn single_sequence_step_write<S>(
    framed: &mut Framed<S>,
    buf: &mut WriteBuf,
    written: Written,
) -> ConnectorResult<()>
where
    S: FramedWrite,
{
    if let Some(response_len) = written.size() {
        debug_assert_eq!(buf.filled_len(), response_len);
        let response = buf.filled();
        trace!(response_len, "Send response");
        framed
            .write_all(response)
            .await
            .map_err(|e| ironrdp_connector::custom_err!("write all", e))?;
    }

    Ok(())
}

#[cfg(test)]
mod arrival_tests {
    use super::*;
    struct Reader {
        chunks: VecDeque<Vec<u8>>,
    }
    impl StreamWrapper for Reader {
        type InnerStream = Self;
        fn from_inner(stream: Self) -> Self {
            stream
        }
        fn into_inner(self) -> Self {
            self
        }
        fn get_inner(&self) -> &Self {
            self
        }
        fn get_inner_mut(&mut self) -> &mut Self {
            self
        }
    }
    impl FramedRead for Reader {
        type ReadFut<'a> = std::future::Ready<io::Result<usize>>;
        fn read<'a>(&'a mut self, buf: &'a mut BytesMut) -> Self::ReadFut<'a> {
            std::thread::sleep(std::time::Duration::from_millis(3));
            let bytes = self.chunks.pop_front().unwrap_or_default();
            let len = bytes.len();
            buf.extend_from_slice(&bytes);
            std::future::ready(Ok(len))
        }
    }
    fn ready<T>(future: impl Future<Output = T>) -> T {
        let mut future = std::pin::pin!(future);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => value,
            _ => panic!("owned synchronous transport unexpectedly pending"),
        }
    }
    #[test]
    fn buffered_pdus_share_transport_arrival_despite_slow_decode_between_them() {
        let mut framed = Framed::<Reader>::new(Reader {
            chunks: VecDeque::from([vec![0, 4, 0, 0, 0, 4, 0, 0]]),
        });
        let _ = ready(framed.read_pdu()).unwrap();
        let first = framed.last_read_at().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = ready(framed.read_pdu()).unwrap();
        assert_eq!(framed.last_read_at(), Some(first));
        assert!(framed.arrivals.is_empty());
    }
    #[test]
    fn fragmented_pdu_uses_the_transport_read_that_completes_it() {
        let mut framed = Framed::<Reader>::new(Reader {
            chunks: VecDeque::from([vec![0, 4], vec![0, 0]]),
        });
        let (_, bytes) = ready(framed.read_pdu()).unwrap();
        assert_eq!(&bytes[..], &[0, 4, 0, 0]);
        assert!(framed.last_read_at().unwrap().as_millis() >= 5);
        assert!(framed.arrivals.is_empty());
    }
    #[test]
    fn read_span_metadata_stays_bounded_even_for_byte_at_a_time_peers() {
        let mut framed = Framed::<Reader>::new(Reader {
            chunks: VecDeque::new(),
        });
        for tick in 0..1000 {
            framed.record_arrival(1, MonotonicInstant::from_millis(tick));
        }
        assert!(framed.arrivals.len() <= 64);
        assert_eq!(framed.arrivals.iter().map(|(n, _)| n).sum::<usize>(), 1000);
        framed.consume_arrivals(1000);
        assert_eq!(framed.last_read_at(), Some(MonotonicInstant::from_millis(999)));
        assert!(framed.arrivals.is_empty());
    }
}
