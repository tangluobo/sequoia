use super::*;
#[derive(Default)]
pub(super) struct Encoder {
    pointer_mask: u8,
    surrogate: Option<u16>,
    supplementary: Option<u32>,
}
impl Encoder {
    pub(super) fn encode(&mut self, event: Input) -> Vec<u8> {
        match event {
            Input::AndroidKey(_) | Input::Resize { .. } => Vec::new(), // Original scales locally.
            Input::ScanCode {
                code,
                extended,
                down,
            } => keysym(code, extended)
                .map(|key| key_packet(key, down))
                .unwrap_or_default(),
            Input::Unicode { value, down } => {
                if (0xd800..=0xdbff).contains(&value) {
                    if down {
                        self.surrogate = Some(value);
                    }
                    return Vec::new();
                }
                let value = if (0xdc00..=0xdfff).contains(&value) {
                    if down {
                        let Some(high) = self.surrogate.take() else {
                            return Vec::new();
                        };
                        let scalar =
                            0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(value)
                                - 0xdc00;
                        self.supplementary = Some(scalar);
                        scalar
                    } else {
                        let Some(scalar) = self.supplementary.take() else {
                            return Vec::new();
                        };
                        scalar
                    }
                } else {
                    self.surrogate = None;
                    u32::from(value)
                };
                key_packet(
                    if value <= 255 {
                        value
                    } else {
                        0x1000000 | value
                    },
                    down,
                )
            }
            Input::Pointer {
                x,
                y,
                button,
                down,
                wheel,
            } => {
                if let Some(button) = button {
                    let bit = match button {
                        1 => 1,
                        2 => 4,
                        3 => 2,
                        _ => 0,
                    };
                    if down {
                        self.pointer_mask |= bit;
                    } else {
                        self.pointer_mask &= !bit;
                    }
                }
                let packet = |mask| vec![5, mask, (x >> 8) as u8, x as u8, (y >> 8) as u8, y as u8];
                if wheel != 0 {
                    let mut result = packet(self.pointer_mask | if wheel > 0 { 8 } else { 16 });
                    result.extend(packet(self.pointer_mask));
                    result
                } else {
                    packet(self.pointer_mask)
                }
            }
        }
    }
}
fn key_packet(symbol: u32, down: bool) -> Vec<u8> {
    let mut packet = vec![4, u8::from(down), 0, 0];
    packet.extend_from_slice(&symbol.to_be_bytes());
    packet
}
fn keysym(code: u8, extended: bool) -> Option<u32> {
    Some(match (code, extended) {
        (0x1d, _) => 0xffe3,
        (0x38, _) => 0xffe9,
        (0x2a | 0x36, false) => 0xffe1,
        (0x5b, true) => 0xffe7,
        (0x3a, false) => 0xffe5,
        (0x01, false) => 0xff1b,
        (0x0e, false) => 0xff08,
        (0x0f, false) => 0xff09,
        (0x1c, _) => 0xff0d,
        (0x47, true) => 0xff50,
        (0x48, true) => 0xff52,
        (0x49, true) => 0xff55,
        (0x4b, true) => 0xff51,
        (0x4d, true) => 0xff53,
        (0x4f, true) => 0xff57,
        (0x50, true) => 0xff54,
        (0x51, true) => 0xff56,
        (0x52, true) => 0xff63,
        (0x53, true) => 0xffff,
        (0x3b..=0x44, false) => 0xffbe + u32::from(code - 0x3b),
        (0x57, false) => 0xffc8,
        (0x58, false) => 0xffc9,
        (0x02..=0x0b, false) => u32::from(b"1234567890"[usize::from(code - 2)]),
        (0x10..=0x19, false) => u32::from(b"qwertyuiop"[usize::from(code - 0x10)]),
        (0x1e..=0x26, false) => u32::from(b"asdfghjkl"[usize::from(code - 0x1e)]),
        (0x2c..=0x32, false) => u32::from(b"zxcvbnm"[usize::from(code - 0x2c)]),
        (0x39, false) => 32,
        (0x0c, false) => b'-'.into(),
        (0x0d, false) => b'='.into(),
        (0x1a, false) => b'['.into(),
        (0x1b, false) => b']'.into(),
        (0x27, false) => b';'.into(),
        (0x28, false) => b'\''.into(),
        (0x29, false) => 96,
        (0x2b, false) => b'\\'.into(),
        (0x33, false) => b','.into(),
        (0x34, false) => b'.'.into(),
        (0x35, false) => b'/'.into(),
        _ => return None,
    })
}
