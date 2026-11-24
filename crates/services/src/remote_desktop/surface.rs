//! A single retained canvas combines dirty rectangles while an inactive/busy UI catches up.
use super::{Frame, Region, RgbaFrame, RgbaUpdate};
use anyhow::{Result, ensure};
use std::sync::Arc;
#[derive(Default)]
pub(super) struct Surface {
    size: [u16; 2],
    pixels: Arc<Vec<u32>>,
    dirty: Vec<Region>,
}
impl Surface {
    pub fn apply(
        &mut self,
        width: u16,
        height: u16,
        region: Region,
        pixels: Vec<u32>,
    ) -> Result<()> {
        let pixels = pixels
            .into_iter()
            .map(|p| u32::from_ne_bytes([(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]))
            .collect();
        self.apply_rgba(width, height, region, pixels)
    }
    pub fn apply_rgba(
        &mut self,
        width: u16,
        height: u16,
        region: Region,
        pixels: Vec<u32>,
    ) -> Result<()> {
        let full = Region {
            x: 0,
            y: 0,
            width,
            height,
        };
        let count = usize::from(width) * usize::from(height);
        ensure!(
            width > 0 && height > 0 && count <= 16_777_216,
            "RDP 帧大小无效"
        );
        ensure!(
            region.width > 0
                && region.height > 0
                && u32::from(region.x) + u32::from(region.width) <= u32::from(width)
                && u32::from(region.y) + u32::from(region.height) <= u32::from(height)
                && pixels.len() == usize::from(region.width) * usize::from(region.height),
            "RDP 更新范围或像素数量无效"
        );
        let resized = self.size != [width, height];
        if region == full {
            self.pixels = Arc::new(pixels);
        } else {
            if resized {
                self.pixels = Arc::new(vec![u32::from_ne_bytes([0, 0, 0, 255]); count]);
            }
            let stride = usize::from(width);
            let target = Arc::make_mut(&mut self.pixels);
            for (row, source) in pixels.chunks_exact(usize::from(region.width)).enumerate() {
                let start = (usize::from(region.y) + row) * stride + usize::from(region.x);
                target[start..start + source.len()].copy_from_slice(source);
            }
        }
        self.size = [width, height];
        if resized {
            self.dirty.clear();
            self.dirty.push(full);
        } else {
            let mut region = region;
            let mut index = 0;
            while index < self.dirty.len() {
                let old = self.dirty[index];
                let joined = union(old, region);
                if area(joined) <= area(old) + area(region) {
                    region = joined;
                    self.dirty.swap_remove(index);
                    index = 0;
                } else {
                    index += 1;
                }
            }
            self.dirty.push(region);
            if self.dirty.len() > 32
                || self.dirty.iter().copied().map(area).sum::<usize>()
                    > usize::from(width) * usize::from(height)
            {
                self.dirty = vec![self.dirty.iter().copied().reduce(union).unwrap()];
            }
        }
        Ok(())
    }
    pub(super) fn cursor_patch(&self, x: u16, y: u16) -> Option<(Region, Vec<u32>)> {
        if self.size.contains(&0) {
            return None;
        }
        let left = x.saturating_sub(24).min(self.size[0] - 1);
        let top = y.saturating_sub(24).min(self.size[1] - 1);
        let right = x.saturating_add(24).min(self.size[0] - 1);
        let bottom = y.saturating_add(24).min(self.size[1] - 1);
        let region = Region {
            x: left,
            y: top,
            width: right - left + 1,
            height: bottom - top + 1,
        };
        Some((region, self.copy(region)))
    }
    pub(super) fn cursor_patch_changes(&self, region: Region, before: &[u32]) -> Option<usize> {
        if u32::from(region.x) + u32::from(region.width) > u32::from(self.size[0])
            || u32::from(region.y) + u32::from(region.height) > u32::from(self.size[1])
        {
            return None;
        }
        let mut changed = 0;
        for (row, samples) in before.chunks_exact(usize::from(region.width)).enumerate() {
            let start =
                (usize::from(region.y) + row) * usize::from(self.size[0]) + usize::from(region.x);
            for (previous, current) in samples
                .iter()
                .zip(&self.pixels[start..start + samples.len()])
            {
                if previous.to_ne_bytes()[..3] != current.to_ne_bytes()[..3] {
                    changed += 1;
                }
            }
        }
        Some(changed)
    }
    pub fn take_rgba(&mut self) -> Vec<RgbaUpdate> {
        let [width, height] = self.size;
        std::mem::take(&mut self.dirty)
            .into_iter()
            .map(|dirty| {
                let pixels = self.copy(dirty);
                RgbaUpdate {
                    frame: RgbaFrame {
                        width,
                        height,
                        pixels,
                    },
                    region: (dirty
                        != Region {
                            x: 0,
                            y: 0,
                            width,
                            height,
                        })
                    .then_some(dirty),
                }
            })
            .collect()
    }
    pub(super) fn full_snapshot(&self) -> Option<super::RdpSnapshot> {
        if self.size.contains(&0) {
            return None;
        }
        Some(super::RdpSnapshot {
            width: self.size[0],
            height: self.size[1],
            region: None,
            pixels: self.pixels.clone(),
        })
    }
    pub fn take_snapshots(&mut self) -> Vec<super::RdpSnapshot> {
        let [width, height] = self.size;
        std::mem::take(&mut self.dirty)
            .into_iter()
            .map(|dirty| {
                let full = dirty
                    == Region {
                        x: 0,
                        y: 0,
                        width,
                        height,
                    };
                super::RdpSnapshot {
                    width,
                    height,
                    region: (!full).then_some(dirty),
                    pixels: if full {
                        self.pixels.clone()
                    } else {
                        Arc::new(self.copy(dirty))
                    },
                }
            })
            .collect()
    }
    fn copy(&self, dirty: Region) -> Vec<u32> {
        let mut pixels = Vec::with_capacity(area(dirty));
        for row in usize::from(dirty.y)..usize::from(dirty.y + dirty.height) {
            let start = row * usize::from(self.size[0]) + usize::from(dirty.x);
            pixels.extend_from_slice(&self.pixels[start..start + usize::from(dirty.width)]);
        }
        pixels
    }
    pub fn take(&mut self) -> Option<Frame> {
        let dirty = self.dirty.iter().copied().reduce(union)?;
        self.dirty.clear();
        let [width, height] = self.size;
        let full = dirty
            == Region {
                x: 0,
                y: 0,
                width,
                height,
            };
        let pixels = self
            .copy(dirty)
            .into_iter()
            .map(|p| {
                let [r, g, b, _] = p.to_ne_bytes();
                (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
            })
            .collect();
        Some(Frame {
            width,
            height,
            pixels,
            region: (!full).then_some(dirty),
        })
    }
}
fn area(r: Region) -> usize {
    usize::from(r.width) * usize::from(r.height)
}
fn union(a: Region, b: Region) -> Region {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Region {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renderer_recreation_recovers_all_pixels_without_consuming_new_damage() {
        let mut surface = Surface::default();
        surface
            .apply_rgba(
                8,
                6,
                Region {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 6,
                },
                vec![17; 48],
            )
            .unwrap();
        let _ = surface.take_snapshots();
        surface
            .apply_rgba(
                8,
                6,
                Region {
                    x: 6,
                    y: 4,
                    width: 2,
                    height: 2,
                },
                vec![29; 4],
            )
            .unwrap();
        let full = surface.full_snapshot().unwrap();
        assert!(full.region.is_none());
        assert_eq!(full.pixels.len(), 48);
        assert_eq!(full.pixels[0], 17);
        assert_eq!(full.pixels[4 * 8 + 6], 29);
        let damage = surface.take_snapshots();
        assert_eq!(damage.len(), 1);
        assert_eq!(
            damage[0].region,
            Some(Region {
                x: 6,
                y: 4,
                width: 2,
                height: 2
            })
        );
        surface
            .apply_rgba(
                8,
                6,
                Region {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                vec![45],
            )
            .unwrap();
        assert_eq!(full.pixels[0], 17);
        assert_eq!(surface.full_snapshot().unwrap().pixels[0], 45);
    }
    #[test]
    fn shared_full_canvas_transfers_no_pixels_and_survives_later_partial_updates() -> Result<()> {
        let mut surface = Surface::default();
        let full = Region {
            x: 0,
            y: 0,
            width: 4,
            height: 3,
        };
        let initial = vec![0xff010203; 12];
        let pointer = initial.as_ptr();
        surface.apply_rgba(4, 3, full, initial)?;
        let previous = surface.take_snapshots().pop().unwrap();
        assert!(previous.region.is_none());
        assert_eq!(previous.pixels.as_ptr(), pointer);
        assert!(surface.take_snapshots().is_empty());
        let patch = Region {
            x: 1,
            y: 1,
            width: 2,
            height: 1,
        };
        surface.apply_rgba(4, 3, patch, vec![0xff445566; 2])?;
        assert_eq!(previous.pixels.as_slice(), &[0xff010203; 12]);
        let changed = surface.take_snapshots().pop().unwrap();
        assert_eq!(changed.region, Some(patch));
        assert_eq!(changed.pixels.as_slice(), &[0xff445566; 2]);
        assert_eq!(surface.pixels[0], 0xff010203);
        assert_eq!(surface.pixels[5], 0xff445566);
        let mut last = std::sync::Weak::new();
        for value in 0..100 {
            surface.apply_rgba(4, 3, full, vec![value; 12])?;
            assert!(last.upgrade().is_none());
            last = Arc::downgrade(&surface.pixels);
        }
        assert_eq!(surface.take_snapshots()[0].pixels.as_slice(), &[99; 12]);
        Ok(())
    }
    #[test]
    fn native_sparse_damage_preserves_channels_bounds_and_delayed_updates() -> Result<()> {
        let mut surface = Surface::default();
        let black = u32::from_ne_bytes([0, 0, 0, 255]);
        surface.apply_rgba(
            1920,
            1080,
            Region {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            vec![black; 1920 * 1080],
        )?;
        assert!(surface.take_rgba()[0].region.is_none());
        let color = u32::from_ne_bytes([17, 34, 51, 255]);
        for (x, y) in [(0, 0), (1888, 1048)] {
            surface.apply_rgba(
                1920,
                1080,
                Region {
                    x,
                    y,
                    width: 32,
                    height: 32,
                },
                vec![color; 1024],
            )?;
        }
        let patches = surface.take_rgba();
        assert_eq!(patches.len(), 2);
        assert_eq!(
            patches.iter().map(|p| p.frame.pixels.len()).sum::<usize>(),
            2048
        );
        assert!(
            patches
                .iter()
                .all(|p| p.frame.pixels.iter().all(|v| *v == color))
        );
        assert!(surface.take_rgba().is_empty());
        for i in 0..100 {
            surface.apply_rgba(
                1920,
                1080,
                Region {
                    x: i * 16,
                    y: i * 8,
                    width: 1,
                    height: 1,
                },
                vec![color],
            )?;
        }
        assert!(surface.dirty.len() <= 32);
        assert!(
            surface
                .take_rgba()
                .iter()
                .map(|p| p.frame.pixels.len())
                .sum::<usize>()
                <= 1920 * 1080
        );
        // A partial first update after resize must publish a complete opaque canvas.
        surface.apply_rgba(
            2,
            2,
            Region {
                x: 1,
                y: 1,
                width: 1,
                height: 1,
            },
            vec![color],
        )?;
        let resized = surface.take_rgba().remove(0);
        assert!(resized.region.is_none());
        assert_eq!(resized.frame.pixels, vec![black, black, black, color]);
        Ok(())
    }
    #[test]
    fn delayed_reader_keeps_disjoint_updates_intervening_pixels_and_resize() -> Result<()> {
        let mut surface = Surface::default();
        surface.apply(
            4,
            3,
            Region {
                x: 0,
                y: 0,
                width: 4,
                height: 3,
            },
            vec![1; 12],
        )?;
        let first = surface.take().unwrap();
        assert!(first.region.is_none());
        assert_eq!(first.pixels, vec![1; 12]);
        surface.apply(
            4,
            3,
            Region {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            vec![2],
        )?;
        surface.apply(
            4,
            3,
            Region {
                x: 3,
                y: 2,
                width: 1,
                height: 1,
            },
            vec![3],
        )?;
        let combined = surface.take().unwrap();
        assert!(combined.region.is_none());
        assert_eq!(combined.pixels[0], 2);
        assert_eq!(combined.pixels[11], 3);
        assert!(combined.pixels[1..11].iter().all(|p| *p == 1));
        surface.apply(
            4,
            3,
            Region {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            },
            vec![4, 5],
        )?;
        let patch = surface.take().unwrap();
        assert_eq!(
            patch.region,
            Some(Region {
                x: 1,
                y: 1,
                width: 2,
                height: 1
            })
        );
        assert_eq!(patch.pixels, vec![4, 5]);
        assert!(surface.take().is_none());
        assert!(
            surface
                .apply(
                    4,
                    3,
                    Region {
                        x: 4,
                        y: 0,
                        width: 1,
                        height: 1
                    },
                    vec![99]
                )
                .is_err()
        );
        assert!(surface.take().is_none());
        surface.apply(
            2,
            2,
            Region {
                x: 1,
                y: 1,
                width: 1,
                height: 1,
            },
            vec![7],
        )?;
        let resized = surface.take().unwrap();
        assert!(resized.region.is_none());
        assert_eq!(resized.pixels, vec![0, 0, 0, 7]);
        Ok(())
    }
}
