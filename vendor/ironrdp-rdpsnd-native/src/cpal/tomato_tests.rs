use super::*;
use ringbuf::traits::Observer as _;

#[derive(Debug)]
struct Host {
    formats: Vec<AudioFormat>,
    confirmed: Arc<AtomicU32>,
}
impl ironrdp_rdpsnd::server::RdpsndServerHandler for Host {
    fn get_formats(&self) -> &[AudioFormat] {
        &self.formats
    }
    fn choose_format<'a>(
        &mut self,
        formats: &'a [ironrdp_rdpsnd::server::NegotiatedFormat],
    ) -> Option<&'a ironrdp_rdpsnd::server::NegotiatedFormat> {
        formats.first()
    }
    fn start(
        &mut self,
        _: &ironrdp_rdpsnd::server::NegotiatedFormat,
    ) -> Result<(), Box<dyn ironrdp_rdpsnd::server::RdpsndError>> {
        Ok(())
    }
    fn stop(&mut self) {}
    fn wave_confirm(&mut self, _: u8, _: u16) {
        self.confirmed.fetch_add(1, Ordering::Relaxed);
    }
}
#[derive(Debug, Default)]
struct Received {
    waves: Vec<(AudioFormat, Vec<u8>)>,
    volume: Option<(u16, u16)>,
    closed: bool,
}
#[derive(Debug)]
struct Sink {
    formats: Vec<AudioFormat>,
    received: Arc<std::sync::Mutex<Received>>,
}
impl RdpsndClientHandler for Sink {
    fn get_flags(&self) -> AudioFormatFlags {
        AudioFormatFlags::VOLUME
    }
    fn get_formats(&self) -> &[AudioFormat] {
        &self.formats
    }
    fn wave(&mut self, format: &AudioFormat, _: u32, bytes: Cow<'_, [u8]>) {
        self.received
            .lock()
            .unwrap()
            .waves
            .push((format.clone(), bytes.into_owned()));
    }
    fn set_volume(&mut self, volume: VolumePdu) {
        self.received.lock().unwrap().volume = Some((volume.volume_left, volume.volume_right));
    }
    fn set_pitch(&mut self, _: PitchPdu) {}
    fn close(&mut self) {
        self.received.lock().unwrap().closed = true;
    }
}
#[test]
fn tomato_rdpsnd_negotiates_real_pdus_plays_pcm_confirms_and_closes() {
    protocol_roundtrip(ironrdp_rdpsnd::pdu::Version::V8);
}
#[test]
fn tomato_rdpsnd_legacy_wave_info_and_separate_payload_are_reassembled() {
    protocol_roundtrip(ironrdp_rdpsnd::pdu::Version::V6);
}
fn protocol_roundtrip(version: ironrdp_rdpsnd::pdu::Version) {
    use ironrdp_rdpsnd::{client::Rdpsnd, server::RdpsndServer};
    use ironrdp_svc::{SvcMessage, SvcProcessor as _};
    let mut formats: Vec<_> = candidate_output_formats()
        .into_iter()
        .filter(|f| f.format == WaveFormat::PCM)
        .take(2)
        .collect();
    let chosen = formats[0].clone();
    let confirmed = Arc::new(AtomicU32::new(0));
    let received = Arc::new(std::sync::Mutex::new(Received::default()));
    let mut server = RdpsndServer::new(Box::new(Host {
        formats: formats.clone(),
        confirmed: confirmed.clone(),
    }));
    let offered_formats = formats.clone();
    formats.reverse();
    let mut client = Rdpsnd::new(Box::new(Sink {
        formats,
        received: received.clone(),
    }));
    let pump = |server: &mut RdpsndServer, client: &mut Rdpsnd, initial: Vec<SvcMessage>| {
        let mut queue: std::collections::VecDeque<_> =
            initial.into_iter().map(|m| (false, m)).collect();
        let mut steps = 0;
        while let Some((from_client, message)) = queue.pop_front() {
            steps += 1;
            assert!(steps < 100);
            let bytes = message.encode_unframed_pdu().unwrap();
            let responses = if from_client {
                server.process(&bytes)
            } else {
                client.process(&bytes)
            }
            .unwrap();
            queue.extend(responses.into_iter().map(|m| (!from_client, m)));
        }
    };
    let _ = server.start().unwrap();
    let initial = vec![SvcMessage::from(
        ironrdp_rdpsnd::pdu::ServerAudioOutputPdu::AudioFormat(
            ironrdp_rdpsnd::pdu::ServerAudioFormatPdu {
                version,
                formats: offered_formats,
            },
        ),
    )];
    pump(&mut server, &mut client, initial);
    assert!(server.is_ready());
    let samples: Vec<_> = (0..1920_u32)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    let messages = server.wave(samples.clone(), 12345).unwrap().into();
    pump(&mut server, &mut client, messages);
    let messages = server.set_volume(0x1234, 0xabcd).unwrap().into();
    pump(&mut server, &mut client, messages);
    let messages = server.close().unwrap().into();
    pump(&mut server, &mut client, messages);
    let observed = received.lock().unwrap();
    assert_eq!(observed.waves, vec![(chosen, samples)]);
    assert_eq!(observed.volume, Some((0x1234, 0xabcd)));
    assert!(observed.closed);
    assert_eq!(confirmed.load(Ordering::Relaxed), 1);
}

#[test]
fn tomato_audio_buffer_is_bounded_by_decoded_pcm_and_underrun_is_silent() {
    let format = candidate_output_formats()
        .into_iter()
        .find(|f| f.format == WaveFormat::PCM)
        .unwrap();
    assert_eq!(ring_buffer_capacity(&format), 24000);
    let (mut producer, consumer) = HeapRb::<u8>::new(32).split();
    producer.push_slice(&[0xff, 0x7f, 0, 0x80]);
    let mut rx = RxBuffer::new(
        consumer,
        Arc::new(AtomicU32::new(pack_volume(0xffff, 0))),
        16,
        2,
    );
    let mut bytes = [9; 16];
    rx.fill(&mut bytes);
    assert_eq!(&bytes[..4], &[0xff, 0x7f, 0, 0]);
    assert!(bytes[4..].iter().all(|b| *b == 0));
}

#[test]
#[ignore = "requires a native default audio output device; sends silence only"]
fn tomato_native_audio_output_opens_plays_and_releases() {
    let format = candidate_output_formats()
        .into_iter()
        .find(|f| f.format == WaveFormat::PCM)
        .unwrap();
    let (stream, mut producer) = DecodeStream::new(
        &format,
        Arc::new(AtomicU32::new(pack_volume(0xffff, 0xffff))),
    )
    .expect("default output must accept PCM or native-format conversion");
    assert_eq!(producer.push_slice(&[0; 1920]), 1920);
    thread::sleep(std::time::Duration::from_millis(150));
    // A started callback must consume the initial block, rather than leaving
    // the paused stream's ring full. No audible test data is used.
    assert_eq!(
        producer.producer.vacant_len(),
        ring_buffer_capacity(&format)
    );
    drop(stream);
}
