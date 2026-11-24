#![expect(clippy::as_conversions, reason = "buffered bit counts are bounded to 64")]
// Borrowed MSB-first reservoir; four-byte refills avoid generic per-bit views.
pub(super) struct DecodeBits<'a> {
    data: &'a [u8],
    cache: u64,
    buffered: u32,
}
impl<'a> DecodeBits<'a> {
    pub(super) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            cache: 0,
            buffered: 0,
        }
    }
    pub(super) fn len(&self) -> usize {
        self.data.len().saturating_mul(8).saturating_add(self.buffered as usize)
    }
    pub(super) fn is_empty(&self) -> bool {
        self.buffered == 0 && self.data.is_empty()
    }
    #[inline]
    fn refill(&mut self) {
        debug_assert!(self.buffered <= 32);
        if self.data.len() >= 4 {
            let word = u32::from_be_bytes(self.data[..4].try_into().expect("four bytes"));
            self.cache = (self.cache << 32) | u64::from(word);
            self.buffered += 32;
            self.data = &self.data[4..];
        } else {
            for &byte in self.data {
                self.cache = (self.cache << 8) | u64::from(byte);
                self.buffered += 8;
            }
            self.data = &[];
        }
    }
    #[inline]
    pub(super) fn read(&mut self, count: usize) -> Option<u32> {
        if count > 32 {
            return None;
        }
        if count == 0 {
            return Some(0);
        }
        let count = u32::try_from(count).expect("at most 32 bits");
        if self.buffered < count {
            if self.len() < count as usize {
                return None;
            }
            self.refill();
        }
        self.buffered -= count;
        let mask = u64::from(u32::MAX >> (32 - count));
        Some(u32::try_from((self.cache >> self.buffered) & mask).expect("at most 32 bits"))
    }
    #[inline]
    pub(super) fn leading(&mut self, value: bool) -> usize {
        let mut total = 0usize;
        while !self.is_empty() {
            if self.buffered == 0 {
                self.refill();
            }
            let word = self.cache << (64 - self.buffered);
            let count = (if value {
                word.leading_ones()
            } else {
                word.leading_zeros()
            })
            .min(self.buffered);
            let all = count == self.buffered;
            self.buffered -= count;
            total += count as usize;
            if !all {
                break;
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitvec::{field::BitField as _, prelude::*};

    #[test]
    fn reservoir_matches_bit_slices_for_unaligned_reads_runs_and_short_tails() {
        for length in [0, 1, 3, 4, 7, 8, 9, 31, 64, 257] {
            for pattern in 0..3 {
                let mut seed = 7u32;
                let bytes = (0..length)
                    .map(|_| {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        match pattern {
                            0 => 0,
                            1 => 255,
                            _ => seed.to_le_bytes()[3],
                        }
                    })
                    .collect::<Vec<_>>();
                let bits = bytes.view_bits::<Msb0>();
                for offset in 0..=bits.len().min(129) {
                    for count in 0..=32 {
                        let mut reader = DecodeBits::new(&bytes);
                        let mut skip = offset;
                        while skip > 0 {
                            let n = skip.min(32);
                            assert!(reader.read(n).is_some());
                            skip -= n;
                        }
                        let expected = if count > bits.len() - offset {
                            None
                        } else if count == 0 {
                            Some(0)
                        } else {
                            Some(bits[offset..offset + count].load_be::<u32>())
                        };
                        assert_eq!(
                            reader.read(count),
                            expected,
                            "length={length} offset={offset} count={count}"
                        );
                        assert_eq!(
                            reader.len(),
                            bits.len() - offset - if expected.is_some() { count } else { 0 }
                        );
                    }
                    for value in [false, true] {
                        let mut reader = DecodeBits::new(&bytes);
                        let mut skip = offset;
                        while skip > 0 {
                            let n = skip.min(32);
                            reader.read(n).unwrap();
                            skip -= n;
                        }
                        let expected = if value {
                            bits[offset..].leading_ones()
                        } else {
                            bits[offset..].leading_zeros()
                        };
                        assert_eq!(reader.leading(value), expected);
                        assert_eq!(reader.len(), bits.len() - offset - expected);
                        let next = reader.len().min(17);
                        let expected = if next == 0 {
                            0
                        } else {
                            bits[offset + expected..offset + expected + next].load_be::<u32>()
                        };
                        assert_eq!(reader.read(next), Some(expected));
                    }
                }
            }
        }
    }
}
