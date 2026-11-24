//! Keep protocol rectangles until presentation, not a full-screen copy per PDU.
use super::RdpOutputEvent;
use core::num::NonZeroU16;
use ironrdp_pdu::geometry::InclusiveRectangle;
use ironrdp_session::{SessionResult, image::DecodedImage};

/// Advertise only codecs backed by an actual decoder. V10 without AVC_DISABLED
/// also implies AVC444, which this client cannot decode; use V8.1 for AVC420.
pub(super) fn client(enable_h264: bool) -> ironrdp_egfx::client::GraphicsPipelineClient {
    use ironrdp_egfx::{client::GraphicsPipelineClient, decode::H264Decoder};
    let decoder: Option<Box<dyn H264Decoder>> = None;
    #[cfg(feature = "graphics-h264")]
    let decoder = match enable_h264.then(ironrdp_egfx::decode::OpenH264Decoder::new) {
        Some(Ok(decoder)) => Some(Box::new(decoder) as Box<dyn H264Decoder>),
        Some(Err(error)) => {
            tracing::warn!(%error, "RDP H264 initialization failed; retaining lossless codecs");
            decoder
        }
        None => decoder,
    };
    #[cfg(not(feature = "graphics-h264"))]
    let _ = enable_h264;
    GraphicsPipelineClient::new(
        Box::new(Handler {
            avc420: decoder.is_some(),
            last_codec: None,
        }),
        decoder,
    )
}

struct Handler {
    avc420: bool,
    last_codec: Option<ironrdp_egfx::pdu::Codec1Type>,
}
impl ironrdp_egfx::client::GraphicsPipelineHandler for Handler {
    fn on_bitmap_updated(&mut self, update: &ironrdp_egfx::client::BitmapUpdate) {
        if self.last_codec != Some(update.codec_id) {
            tracing::debug!(codec = ?update.codec_id, width = update.width, height = update.height, "RDP graphics codec in use");
            self.last_codec = Some(update.codec_id);
        }
    }
    fn capabilities(&self) -> Vec<ironrdp_egfx::pdu::CapabilitySet> {
        use ironrdp_egfx::pdu::{
            CapabilitiesV8Flags, CapabilitiesV10Flags, CapabilitiesV81Flags, CapabilitySet,
        };
        let preferred = if self.avc420 {
            CapabilitySet::V8_1 {
                flags: CapabilitiesV81Flags::AVC420_ENABLED | CapabilitiesV81Flags::SMALL_CACHE,
            }
        } else {
            CapabilitySet::V10 {
                // Match Java DrdynvcChannel's lossless V10 negotiation. An
                // extra SMALL_CACHE flag changes the server's cache policy.
                // Actual tiles remain demand-allocated and compositor-bounded.
                flags: CapabilitiesV10Flags::AVC_DISABLED,
            }
        };
        vec![
            preferred,
            CapabilitySet::V8 {
                flags: CapabilitiesV8Flags::SMALL_CACHE,
            },
        ]
    }
}
#[derive(Default)]
pub(super) struct Updates {
    canvas: [u16; 2],
    dirty: Vec<InclusiveRectangle>,
}
impl Updates {
    pub(super) fn pending(&self) -> bool {
        !self.dirty.is_empty()
    }
    pub(super) fn mark(&mut self, mut region: InclusiveRectangle, width: u16, height: u16) {
        if width == 0 || height == 0 {
            return;
        }
        if self.canvas != [width, height] {
            self.canvas = [width, height];
            self.dirty.clear();
            region = InclusiveRectangle {
                left: 0,
                top: 0,
                right: width - 1,
                bottom: height - 1,
            };
        }
        region.right = region.right.min(width - 1);
        region.bottom = region.bottom.min(height - 1);
        if region.left > region.right || region.top > region.bottom {
            return;
        }
        let mut index = 0;
        while index < self.dirty.len() {
            let old = self.dirty[index].clone();
            let joined = union(&old, &region);
            // Join adjacent scanlines, but do not turn two small distant changes
            // (clock/caret/scrollbar) into a full-screen upload.
            if area(&joined) <= area(&old) + area(&region) {
                region = joined;
                self.dirty.swap_remove(index);
                index = 0;
            } else {
                index += 1;
            }
        }
        self.dirty.push(region);
        if self.dirty.len() > 32
            || self.dirty.iter().map(area).sum::<usize>() > usize::from(width) * usize::from(height)
        {
            self.dirty = vec![
                self.dirty
                    .iter()
                    .cloned()
                    .reduce(|a, b| union(&a, &b))
                    .unwrap(),
            ];
        }
    }

    pub(super) fn take(&mut self, image: &DecodedImage) -> SessionResult<RdpOutputEvent> {
        if self.dirty.is_empty() {
            return Err(ironrdp_session::general_err!("no dirty RDP pixels"));
        }
        // Reactivation may replace the canvas between its last update and the timer.
        if self.canvas != [image.width(), image.height()] {
            self.dirty = vec![InclusiveRectangle {
                left: 0,
                top: 0,
                right: image.width().saturating_sub(1),
                bottom: image.height().saturating_sub(1),
            }];
            self.canvas = [image.width(), image.height()];
        }
        let width = NonZeroU16::new(image.width())
            .ok_or_else(|| ironrdp_session::general_err!("width is zero"))?;
        let height = NonZeroU16::new(image.height())
            .ok_or_else(|| ironrdp_session::general_err!("height is zero"))?;
        let regions = self
            .dirty
            .drain(..)
            .map(|region| {
                let mut buffer = Vec::with_capacity(area(&region));
                let start = usize::from(region.left) * 4;
                let end = (usize::from(region.right) + 1) * 4;
                for row in image
                    .data()
                    .chunks_exact(usize::from(width.get()) * 4)
                    .skip(usize::from(region.top))
                    .take(usize::from(region.bottom - region.top) + 1)
                {
                    // Preserve native byte order. Unpainted decoded pixels can have
                    // zero alpha; the desktop is opaque, including its initial black.
                    buffer.extend(
                        row[start..end]
                            .chunks_exact(4)
                            .map(|p| u32::from_ne_bytes([p[0], p[1], p[2], 255])),
                    );
                }
                (region, buffer)
            })
            .collect();
        Ok(RdpOutputEvent::RgbaRegions {
            width,
            height,
            regions,
        })
    }
}
fn area(r: &InclusiveRectangle) -> usize {
    (usize::from(r.right - r.left) + 1) * (usize::from(r.bottom - r.top) + 1)
}
fn union(a: &InclusiveRectangle, b: &InclusiveRectangle) -> InclusiveRectangle {
    InclusiveRectangle {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}
