use super::*;

#[test]
fn signed_average_matches_i32_division_across_the_i16_domain() {
    for other in [i16::MIN, i16::MIN + 1, -1, 0, 1, i16::MAX - 1, i16::MAX] {
        for base in (i32::from(i16::MIN)..=i32::from(i16::MAX)).step_by(8) {
            let values = core::array::from_fn(|lane| {
                i16::try_from(base + i32::try_from(lane).unwrap()).unwrap()
            });
            let result = trunc_average(i16x8::new(values), i16x8::splat(other)).to_array();
            for (value, actual) in values.into_iter().zip(result) {
                assert_eq!(i32::from(actual), (i32::from(value) + i32::from(other)) / 2);
            }
        }
    }
}

fn corpus() -> Vec<[i16; 4096]> {
    let mut inputs = Vec::new();
    for value in [0, 1, -1, i16::MIN, i16::MAX, -32767, 32766] {
        inputs.push([value; 4096]);
    }
    let mut seed = 0x1234abcd_u32;
    for tile in 0..128 {
        inputs.push(core::array::from_fn(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let value = i16::from_ne_bytes(seed.to_ne_bytes()[..2].try_into().unwrap());
            if tile % 2 == 0 { value } else { value >> 4 }
        }));
    }
    inputs
}

#[test]
fn every_output_matches_the_independent_scalar_transform() {
    for input in corpus() {
        let mut actual = input;
        let mut expected = input;
        decode(&mut actual, &mut [0; 4096]);
        reference::decode(&mut expected, &mut [0; 4096]);
        assert_eq!(actual, expected);
    }
    // Exercise every coefficient position, including all asymmetric subband
    // boundaries, with alternating signs and overflow-sized impulses.
    for position in 0..4096 {
        let mut actual = [0; 4096];
        actual[position] = if position % 2 == 0 {
            i16::MIN
        } else {
            i16::MAX
        };
        let mut expected = actual;
        decode(&mut actual, &mut [0; 4096]);
        reference::decode(&mut expected, &mut [0; 4096]);
        assert_eq!(actual, expected, "coefficient position {position}");
    }
}

#[test]
#[ignore = "explicit paired CPU benchmark, not display FPS"]
fn paired_scalar_and_simd_benchmark() {
    use std::{
        hint::black_box,
        time::{Duration, Instant},
    };
    let inputs = corpus();
    let mut totals = [Duration::ZERO; 2];
    for round in 0..40 {
        for path in [round % 2, 1 - round % 2] {
            let started = Instant::now();
            for input in &inputs {
                let mut output = *black_box(input);
                let mut scratch = [0; 4096];
                if path == 0 {
                    reference::decode(&mut output, &mut scratch);
                } else {
                    decode(&mut output, &mut scratch);
                }
                black_box(output);
            }
            totals[path] += started.elapsed();
        }
    }
    eprintln!(
        "RDP_DWT_BENCH rounds=40 tiles_per_path={} scalar_ms={:.3} simd_ms={:.3} speedup={:.3}",
        inputs.len() * 40,
        totals[0].as_secs_f64() * 1000.,
        totals[1].as_secs_f64() * 1000.,
        totals[0].as_secs_f64() / totals[1].as_secs_f64()
    );
}
