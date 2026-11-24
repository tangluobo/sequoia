//! Convert negotiated PCM into the device's native mix format without blocking.
use super::RxBuffer;
use cpal::SampleFormat;

pub(super) fn supported(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32
            | SampleFormat::F64
            | SampleFormat::I16
            | SampleFormat::I32
            | SampleFormat::U8
    )
}
pub(super) struct Renderer {
    input: RxBuffer,
    bytes: [u8; 1024],
    offset: usize,
    source_channels: usize,
    bits: u16,
    target_channels: usize,
    ratio: f64,
    phase: f64,
    current: [f32; 2],
    next: [f32; 2],
    initialized: bool,
}
impl Renderer {
    pub(super) fn new(
        input: RxBuffer,
        source_rate: u32,
        source_channels: u16,
        bits: u16,
        target_rate: u32,
        target_channels: u16,
    ) -> Self {
        Self {
            input,
            bytes: [0; 1024],
            offset: 1024,
            source_channels: usize::from(source_channels),
            bits,
            target_channels: usize::from(target_channels),
            ratio: f64::from(source_rate) / f64::from(target_rate.max(1)),
            phase: 0.0,
            current: [0.0; 2],
            next: [0.0; 2],
            initialized: false,
        }
    }
    fn frame(&mut self) -> [f32; 2] {
        let bytes_per_sample = usize::from(self.bits / 8);
        let frame_bytes = bytes_per_sample * self.source_channels;
        if self.offset + frame_bytes > self.bytes.len() {
            self.input.fill(&mut self.bytes);
            self.offset = 0;
        }
        let mut values = [0.0; 2];
        for (channel, value) in values.iter_mut().enumerate() {
            let channel = channel.min(self.source_channels.saturating_sub(1));
            let position = self.offset + channel * bytes_per_sample;
            *value = if self.bits == 8 {
                (f32::from(self.bytes[position]) - 128.0) / 128.0
            } else {
                f32::from(i16::from_le_bytes([
                    self.bytes[position],
                    self.bytes[position + 1],
                ])) / 32768.0
            };
        }
        self.offset += frame_bytes;
        values
    }
    pub(super) fn fill(&mut self, output: &mut [u8], format: SampleFormat) {
        let sample_bytes = format.sample_size();
        if self.target_channels == 0 || sample_bytes == 0 {
            output.fill(0);
            return;
        }
        if !self.initialized {
            self.current = self.frame();
            self.next = self.frame();
            self.initialized = true;
        }
        for frame in output.chunks_exact_mut(sample_bytes * self.target_channels) {
            for (channel, bytes) in frame.chunks_exact_mut(sample_bytes).enumerate() {
                let sample = if channel < 2 {
                    self.current[channel]
                        + (self.next[channel] - self.current[channel]) * self.phase as f32
                } else {
                    0.0
                };
                let sample = sample.clamp(-1.0, 1.0);
                match format {
                    SampleFormat::F32 => bytes.copy_from_slice(&sample.to_ne_bytes()),
                    SampleFormat::F64 => bytes.copy_from_slice(&f64::from(sample).to_ne_bytes()),
                    SampleFormat::I16 => bytes.copy_from_slice(
                        &((sample * 32768.0).clamp(-32768.0, 32767.0) as i16).to_ne_bytes(),
                    ),
                    SampleFormat::I32 => bytes.copy_from_slice(
                        &((f64::from(sample) * 2147483648.0).clamp(-2147483648.0, 2147483647.0)
                            as i32)
                            .to_ne_bytes(),
                    ),
                    SampleFormat::U8 => bytes[0] = (sample * 128.0 + 128.0).clamp(0.0, 255.0) as u8,
                    _ => bytes.fill(0),
                }
            }
            self.phase += self.ratio;
            while self.phase >= 1.0 {
                self.phase -= 1.0;
                self.current = self.next;
                self.next = self.frame();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::{
        HeapRb,
        traits::{Producer, Split},
    };
    use std::sync::{Arc, atomic::AtomicU32};
    #[test]
    fn tomato_pcm_conversion_resamples_mono_to_native_float_stereo_and_zeros_underrun() {
        let (mut producer, consumer) = HeapRb::<u8>::new(2048).split();
        let pcm = (0..256)
            .flat_map(|_| 16384_i16.to_le_bytes())
            .collect::<Vec<_>>();
        assert_eq!(producer.push_slice(&pcm), pcm.len());
        let input = RxBuffer::new(
            consumer,
            Arc::new(AtomicU32::new(super::super::pack_volume(0xffff, 0xffff))),
            16,
            1,
        );
        let mut renderer = Renderer::new(input, 8000, 1, 16, 48000, 2);
        let mut bytes = vec![0; 100 * 2 * 4];
        renderer.fill(&mut bytes, SampleFormat::F32);
        assert!(
            bytes
                .chunks_exact(4)
                .all(|b| f32::from_ne_bytes(b.try_into().unwrap()) == 0.5)
        );
        let mut tail = vec![1; 4096 * 2 * 4];
        renderer.fill(&mut tail, SampleFormat::F32);
        assert!(tail[tail.len() - 64..].iter().all(|b| *b == 0));
    }
}
