use super::*;
pub(super) fn decode(mode: EntropyAlgorithm, tile: &[u8], mut output: &mut [i16]) -> Result<(), RlgrError> {
    #![expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "u32-to-usize and usize-to-u32 conversions, mostly fine, and hot loop"
    )]

    if tile.is_empty() {
        return Err(RlgrError::EmptyTile);
    }

    let mut k: u32 = 1;
    let mut kr: u32 = 1;
    let mut kp: u32 = k << LS_GR;
    let mut krp: u32 = kr << LS_GR;

    let mut bits = Bits::new(BitSlice::from_slice(tile));

    while !bits.is_empty() && !output.is_empty() {
        match CompressionMode::from(k) {
            CompressionMode::RunLength => {
                let number_of_zeros = truncate_leading_value(&mut bits, false);
                try_split_bits!(bits, 1);
                let run = count_run(number_of_zeros, &mut k, &mut kp) + load_be_u32(try_split_bits!(bits, k as usize));

                let sign_bit = try_split_bits!(bits, 1).load_be::<u8>();

                let number_of_ones = truncate_leading_value(&mut bits, true);
                try_split_bits!(bits, 1);

                let code_remainder = load_be_u32(try_split_bits!(bits, kr as usize)) + ((number_of_ones as u32) << kr);

                update_parameters_according_to_number_of_ones(number_of_ones, &mut kr, &mut krp);
                kp = kp.saturating_sub(DN_GR);
                k = kp >> LS_GR;

                let magnitude = compute_rl_magnitude(sign_bit, code_remainder)?;

                let size = min(run as usize, output.len());
                fill(&mut output[..size], 0);
                output = &mut output[size..];
                write_byte!(output, magnitude);
            }
            CompressionMode::GolombRice => {
                let number_of_ones = truncate_leading_value(&mut bits, true);
                try_split_bits!(bits, 1);

                let code_remainder = load_be_u32(try_split_bits!(bits, kr as usize)) + ((number_of_ones as u32) << kr);

                update_parameters_according_to_number_of_ones(number_of_ones, &mut kr, &mut krp);

                match mode {
                    EntropyAlgorithm::Rlgr1 => {
                        let magnitude = compute_rlgr1_magnitude(code_remainder, &mut k, &mut kp)?;
                        write_byte!(output, magnitude);
                    }
                    EntropyAlgorithm::Rlgr3 => {
                        let n_index = compute_n_index(code_remainder);

                        let val1 = load_be_u32(try_split_bits!(bits, n_index));
                        let val2 = code_remainder - val1;
                        if val1 != 0 && val2 != 0 {
                            kp = kp.saturating_sub(2 * DQ_GR);
                            k = kp >> LS_GR;
                        } else if val1 == 0 && val2 == 0 {
                            kp = min(kp + 2 * UQ_GR, KP_MAX);
                            k = kp >> LS_GR;
                        }

                        let magnitude = compute_rlgr3_magnitude(val1)?;
                        write_byte!(output, magnitude);

                        let magnitude = compute_rlgr3_magnitude(val2)?;
                        write_byte!(output, magnitude);
                    }
                }
            }
        }
    }

    // Fill remaining buffer with zeros.
    fill(output, 0);

    Ok(())
}
