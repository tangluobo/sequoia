//! Frozen pre-optimization decoder, compiled only for differential tests.
use super::*;
impl Decompressor {
    pub(super) fn decompress_segment_reference(
        &mut self,
        encoded_data: &[u8],
        output: &mut Vec<u8>,
    ) -> Result<usize, ZgfxError> {
        if encoded_data.is_empty() {
            return Ok(0);
        }

        let mut bits = BitSlice::from_slice(encoded_data);

        // The low 3 bits of the last byte indicate the number of unused bits in the final
        // byte (0-7); MS-RDPEGFX 3.1.9.1.2.4 reserves the five high-order bits. Mask them
        // off rather than reading the whole byte, or a sender that leaves them non-zero
        // (nothing in the spec requires zeroing a reserved field) makes this look like a
        // huge trailing-bit count and silently truncates real token data instead of
        // decoding it.
        // Use checked arithmetic so attacker-controlled trailing-bit counts that exceed the
        // available bit budget surface as a typed error rather than an unsigned underflow panic.
        let trailing_unused =
            usize::from(*encoded_data.last().expect("encoded_data is not empty") & 0x07);
        let bit_count = 8usize
            .checked_mul(encoded_data.len() - 1)
            .and_then(|n| n.checked_sub(trailing_unused))
            .ok_or(ZgfxError::InvalidTrailingBitCount(trailing_unused))?;
        bits = &bits[..bit_count];
        let mut bits = Bits::new(bits);
        let mut bytes_written = 0;

        while !bits.is_empty() {
            // Token prefix lookup uses bit-budget-aware indexing so a truncated
            // segment whose remaining bits are shorter than every prefix surfaces
            // as TokenBitsNotFound rather than an out-of-range slice panic.
            let token = TOKEN_TABLE
                .iter()
                .find(|token| {
                    bits.get(..token.prefix.len())
                        .is_some_and(|s| s == token.prefix)
                })
                .ok_or(ZgfxError::TokenBitsNotFound)?;
            // The prefix length was just validated by the find above; this split
            // cannot exceed the remaining bit budget.
            let _prefix = bits.split_to(token.prefix.len());

            match token.ty {
                TokenType::NullLiteral => {
                    // The prefix value is encoded with a "0" prefix,
                    // then read 8 bits containing the byte to output.
                    let value = bits
                        .try_split_to(8)
                        .ok_or(ZgfxError::IncompleteBitStream {
                            needed: 8,
                            remaining: bits.len(),
                        })?
                        .load_be::<u8>();

                    self.history.write_u8(value)?;
                    output.push(value);
                    bytes_written += 1;
                }
                TokenType::Literal { literal_value } => {
                    self.history
                        .write_u8(literal_value)
                        .expect("circular buffer does not fail");
                    output.push(literal_value);
                    bytes_written += 1;
                }
                TokenType::Match {
                    distance_value_size,
                    distance_base,
                } => {
                    let max_remaining = MAX_DECOMPRESSED_PER_SEGMENT
                        .checked_sub(bytes_written)
                        .ok_or(ZgfxError::SegmentDecompressedSizeExceedsLimit {
                            decompressed: bytes_written,
                            limit: MAX_DECOMPRESSED_PER_SEGMENT,
                        })?;
                    let written = handle_match(
                        &mut bits,
                        distance_value_size,
                        distance_base,
                        &mut self.history,
                        output,
                        max_remaining,
                    )?;
                    bytes_written += written;
                }
            }

            // The per-token check above bounds a single match copy; this bounds their
            // sum, which is what a hostile stream actually exploits. Catches the
            // cumulative NullLiteral / Literal / Match contribution that could
            // otherwise inflate a small input into a multi-gigabyte allocation.
            if bytes_written > MAX_DECOMPRESSED_PER_SEGMENT {
                return Err(ZgfxError::SegmentDecompressedSizeExceedsLimit {
                    decompressed: bytes_written,
                    limit: MAX_DECOMPRESSED_PER_SEGMENT,
                });
            }
        }

        Ok(bytes_written)
    }
}

fn handle_match(
    bits: &mut Bits<'_>,
    distance_value_size: usize,
    distance_base: u32,
    history: &mut FixedCircularBuffer,
    output: &mut Vec<u8>,
    max_remaining: usize,
) -> Result<usize, ZgfxError> {
    // Each token has been assigned a different base distance
    // and number of additional value bits to be added to compute the full distance.

    let distance_value = bits
        .try_split_to(distance_value_size)
        .ok_or(ZgfxError::IncompleteBitStream {
            needed: distance_value_size,
            remaining: bits.len(),
        })?
        .load_be::<u32>();
    let full_distance =
        distance_base
            .checked_add(distance_value)
            .ok_or(ZgfxError::InvalidIntegralConversion(
                "token's full distance",
            ))?;
    let distance = usize::try_from(full_distance)
        .map_err(|_| ZgfxError::InvalidIntegralConversion("token's full distance"))?;

    if distance == 0 {
        read_unencoded_bytes(bits, history, output, max_remaining)
    } else {
        // Bound the match distance against the history size to prevent the
        // circular buffer's position arithmetic from underflowing on an
        // attacker-controlled distance. This is defense-in-depth alongside
        // the bound check inside FixedCircularBuffer::read_with_offset.
        if distance > HISTORY_SIZE {
            return Err(ZgfxError::MatchDistanceOutOfRange {
                distance,
                history_size: HISTORY_SIZE,
            });
        }
        read_encoded_bytes(bits, distance, history, output, max_remaining)
    }
}

fn read_unencoded_bytes(
    bits: &mut Bits<'_>,
    history: &mut FixedCircularBuffer,
    output: &mut Vec<u8>,
    max_remaining: usize,
) -> Result<usize, ZgfxError> {
    // A match distance of zero is a special case,
    // which indicates that an unencoded run of bytes follows.
    // The count of bytes is encoded as a 15-bit value.
    let length = bits
        .try_split_to(15)
        .ok_or(ZgfxError::IncompleteBitStream {
            needed: 15,
            remaining: bits.len(),
        })?
        .load_be::<usize>();

    // Enforce the per-segment cap before any allocation. length is a 15-bit
    // value (max 32,767) so it can never exceed MAX_DECOMPRESSED_PER_SEGMENT,
    // but it may exceed max_remaining if the prior tokens already filled most
    // of the budget.
    if length > max_remaining {
        return Err(ZgfxError::SegmentDecompressedSizeExceedsLimit {
            decompressed: MAX_DECOMPRESSED_PER_SEGMENT
                .saturating_sub(max_remaining)
                .saturating_add(length),
            limit: MAX_DECOMPRESSED_PER_SEGMENT,
        });
    }

    if bits.remaining_bits_of_last_byte() > 0 {
        let pad_to_byte_boundary = 8 - bits.remaining_bits_of_last_byte();
        bits.try_split_to(pad_to_byte_boundary)
            .ok_or(ZgfxError::IncompleteBitStream {
                needed: pad_to_byte_boundary,
                remaining: bits.len(),
            })?;
    }

    // length is a 15-bit value (max 32767); multiplied by 8 cannot overflow usize on
    // any supported platform (max ~262 Kbits). Use checked_mul for defence anyway.
    let unencoded_bit_count = length
        .checked_mul(8)
        .ok_or(ZgfxError::InvalidIntegralConversion(
            "unencoded byte-run bit count",
        ))?;
    let unencoded_bits =
        bits.try_split_to(unencoded_bit_count)
            .ok_or(ZgfxError::IncompleteBitStream {
                needed: unencoded_bit_count,
                remaining: bits.len(),
            })?;

    // FIXME: not very efficient, but we need to rework the `Bits` helper and refactor a bit otherwise
    let unencoded_bits = unencoded_bits.to_bitvec();
    let unencoded_bytes = unencoded_bits.as_raw_slice();
    history.write_all(unencoded_bytes)?;
    output.extend_from_slice(unencoded_bytes);

    Ok(unencoded_bytes.len())
}

fn read_encoded_bytes(
    bits: &mut Bits<'_>,
    distance: usize,
    history: &mut FixedCircularBuffer,
    output: &mut Vec<u8>,
    max_remaining: usize,
) -> Result<usize, ZgfxError> {
    // A match length prefix follows the token and indicates
    // how many additional bits will be needed to get the full length
    // (the number of bytes to be copied).

    let length_token_size = bits.leading_ones();
    // length token + zero bit
    let prefix_len = length_token_size
        .checked_add(1)
        .ok_or(ZgfxError::LengthTokenSizeTooLarge(length_token_size))?;
    bits.try_split_to(prefix_len)
        .ok_or(ZgfxError::IncompleteBitStream {
            needed: prefix_len,
            remaining: bits.len(),
        })?;

    let length = if length_token_size == 0 {
        // special case
        3
    } else {
        // The length-value bit width must fit a usize load_be call. Bound it before
        // attempting any allocation or split so an attacker cannot drive the
        // exponent into a pow overflow or load_be panic.
        let value_bits = length_token_size
            .checked_add(1)
            .ok_or(ZgfxError::LengthTokenSizeTooLarge(length_token_size))?;
        let usize_bits = usize::try_from(usize::BITS)
            .map_err(|_| ZgfxError::LengthTokenSizeTooLarge(length_token_size))?;
        if value_bits >= usize_bits {
            return Err(ZgfxError::LengthTokenSizeTooLarge(length_token_size));
        }
        let value = bits
            .try_split_to(value_bits)
            .ok_or(ZgfxError::IncompleteBitStream {
                needed: value_bits,
                remaining: bits.len(),
            })?
            .load_be::<usize>();

        let exponent = u32::try_from(value_bits)
            .map_err(|_| ZgfxError::LengthTokenSizeTooLarge(length_token_size))?;
        let base = 1usize
            .checked_shl(exponent)
            .ok_or(ZgfxError::LengthTokenSizeTooLarge(length_token_size))?;
        base.checked_add(value)
            .ok_or(ZgfxError::LengthTokenSizeTooLarge(length_token_size))?
    };

    // Enforce the per-segment cap before allocating. A match-copy of length N
    // grows the output by N bytes; rejecting before the circular buffer read
    // prevents an attacker-controlled match length from inflating the output
    // beyond the spec-defined bound.
    if length > max_remaining {
        return Err(ZgfxError::SegmentDecompressedSizeExceedsLimit {
            decompressed: MAX_DECOMPRESSED_PER_SEGMENT
                .saturating_sub(max_remaining)
                .saturating_add(length),
            limit: MAX_DECOMPRESSED_PER_SEGMENT,
        });
    }

    let output_length = output.len();
    history.read_with_offset(distance, length, output)?;
    history
        .write_all(&output[output_length..])
        .expect("circular buffer does not fail");

    Ok(length)
}
