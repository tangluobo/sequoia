//! The sole Java serialization type used by the original custom clipboard MIME.
//! No object graph, class descriptors, handles or executable deserialization.
use std::io::{Error, ErrorKind, Result};
pub const MAX_CLIPBOARD: usize = 1024 * 1024;
fn invalid() -> Error {
    Error::new(ErrorKind::InvalidData, "对象复制剪贴板格式无效或超过1 MiB")
}

pub fn encode(text: &str) -> Result<Vec<u8>> {
    if text.len() > MAX_CLIPBOARD {
        return Err(invalid());
    }
    let length: usize = text
        .encode_utf16()
        .map(|u| match u {
            1..=0x7f => 1,
            0..=0x7ff => 2,
            _ => 3,
        })
        .sum();
    let header = if length <= u16::MAX as usize { 7 } else { 13 };
    if length.saturating_add(header) > MAX_CLIPBOARD {
        return Err(invalid());
    }
    let mut bytes = Vec::with_capacity(header + length);
    bytes.extend_from_slice(&[0xac, 0xed, 0, 5]);
    if header == 7 {
        bytes.push(0x74);
        bytes.extend_from_slice(&(length as u16).to_be_bytes());
    } else {
        bytes.push(0x7c);
        bytes.extend_from_slice(&(length as u64).to_be_bytes());
    }
    for unit in text.encode_utf16() {
        match unit {
            1..=0x7f => bytes.push(unit as u8),
            0..=0x7ff => {
                bytes.push(0xc0 | (unit >> 6) as u8);
                bytes.push(0x80 | (unit & 63) as u8);
            }
            _ => {
                bytes.push(0xe0 | (unit >> 12) as u8);
                bytes.push(0x80 | ((unit >> 6) & 63) as u8);
                bytes.push(0x80 | (unit & 63) as u8);
            }
        }
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<String> {
    if bytes.len() > MAX_CLIPBOARD || bytes.get(..4) != Some(&[0xac, 0xed, 0, 5]) {
        return Err(invalid());
    }
    let (header, length): (usize, u64) = match bytes.get(4) {
        Some(0x74) => (
            7,
            u16::from_be_bytes(
                bytes
                    .get(5..7)
                    .ok_or_else(invalid)?
                    .try_into()
                    .map_err(|_| invalid())?,
            ) as u64,
        ),
        Some(0x7c) => (
            13,
            u64::from_be_bytes(
                bytes
                    .get(5..13)
                    .ok_or_else(invalid)?
                    .try_into()
                    .map_err(|_| invalid())?,
            ),
        ),
        _ => return Err(invalid()),
    };
    let length = usize::try_from(length).map_err(|_| invalid())?;
    let end = header.checked_add(length).ok_or_else(invalid)?;
    let payload = bytes.get(header..end).ok_or_else(invalid)?;
    let mut at = 0;
    let mut text = String::with_capacity(length);
    while at < payload.len() {
        let first = unit(payload, &mut at)?;
        let scalar = if (0xd800..=0xdbff).contains(&first) {
            let second = unit(payload, &mut at)?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err(invalid());
            }
            0x10000 + ((u32::from(first) - 0xd800) << 10) + u32::from(second) - 0xdc00
        } else {
            u32::from(first)
        };
        text.push(char::from_u32(scalar).ok_or_else(invalid)?);
    }
    // Native allocation padding after the declared String is irrelevant.
    Ok(text)
}
fn unit(bytes: &[u8], at: &mut usize) -> Result<u16> {
    let first = *bytes.get(*at).ok_or_else(invalid)?;
    *at += 1;
    let value = match first {
        1..=0x7f => return Ok(u16::from(first)),
        0xc0..=0xdf => u16::from(first & 31),
        0xe0..=0xef => u16::from(first & 15),
        _ => return Err(invalid()),
    };
    let mut result = value;
    for _ in 0..if first < 0xe0 { 1 } else { 2 } {
        let next = *bytes.get(*at).ok_or_else(invalid)?;
        *at += 1;
        if next & 0xc0 != 0x80 {
            return Err(invalid());
        }
        result = (result << 6) | u16::from(next & 63);
    }
    if (first < 0xe0 && result != 0 && result < 0x80) || (first >= 0xe0 && result < 0x800) {
        return Err(invalid());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_java_object_output_stream_bytes_match_for_short_and_long_strings() -> Result<()> {
        for (text, expected) in [
            (
                "fixture\nS3\n目录\nF|🍅.txt\n\0".to_owned(),
                &include_bytes!("fixtures/java-string-short.bin")[..],
            ),
            (
                "x".repeat(70_000),
                &include_bytes!("fixtures/java-string-long.bin")[..],
            ),
        ] {
            assert_eq!(encode(&text)?, expected);
            assert_eq!(decode(expected)?, text);
        }
        Ok(())
    }
    #[test]
    fn modified_utf8_short_and_long_strings_roundtrip_without_loading_objects() -> Result<()> {
        for text in [
            "".to_owned(),
            "fixture\nS3\n目录\nF|🍅.txt\n\0".to_owned(),
            "x".repeat(70_000),
        ] {
            let encoded = encode(&text)?;
            assert_eq!(encoded[4], if text.len() > 65_535 { 0x7c } else { 0x74 });
            assert_eq!(decode(&encoded)?, text);
        }
        for bytes in [
            &b"\xac\xed\0\x05\x73"[..],
            &b"\xac\xed\0\x05\x74\0\x01\0"[..],
            &b"\xac\xed\0\x05\x74\0\x02\xc0A"[..],
            &b"\xac\xed\0\x05\x7c\xff\xff\xff\xff\xff\xff\xff\xff"[..],
        ] {
            assert!(decode(bytes).is_err());
        }
        assert!(encode(&"中".repeat(MAX_CLIPBOARD)).is_err());
        Ok(())
    }
}
