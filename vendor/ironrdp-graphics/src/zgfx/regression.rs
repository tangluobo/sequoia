//! Compare the fast lookup with the independently retained decoder.
use super::*;
use bitvec::view::BitView as _;

const WIRES: [&[u8]; 5] = [
    include_bytes!("test_assets/encoded.0.bin"),
    include_bytes!("test_assets/encoded.1.bin"),
    include_bytes!("test_assets/encoded.2.bin"),
    include_bytes!("test_assets/encoded.3.bin"),
    include_bytes!("test_assets/encoded.4.bin"),
];

fn old_fixture(
    decoder: &mut Decompressor,
    wire: &[u8],
    output: &mut Vec<u8>,
) -> Result<usize, ZgfxError> {
    assert_eq!(wire[0], 0xe0);
    if wire[1] & 0x20 != 0 {
        decoder.decompress_segment_reference(&wire[2..], output)
    } else {
        decoder.decompress(wire, output)
    }
}

#[test]
fn token_lookup_matches_the_old_scan_at_every_prefix_and_short_tail() {
    for word in 0u16..512 {
        let bytes = (word << 7).to_be_bytes();
        for count in 1..=9 {
            let bits = Bits::new(&bytes.view_bits::<Msb0>()[..count]);
            let expected = TOKEN_TABLE.iter().position(|token| {
                bits.get(..token.prefix.len())
                    .is_some_and(|s| s == token.prefix)
            });
            let actual = lookup_token(&bits).map(|found| {
                TOKEN_TABLE
                    .iter()
                    .position(|token| std::ptr::eq(token, found))
                    .unwrap()
            });
            assert_eq!(actual, expected, "word={word} count={count}");
        }
    }
}

#[test]
fn independent_decoder_matches_corpus_truncations_history_and_error_outputs() {
    for (index, wire) in WIRES.iter().enumerate() {
        let body = &wire[2..];
        for cut in [
            0,
            1,
            2,
            3,
            5,
            7,
            8,
            body.len() / 2,
            body.len().saturating_sub(1),
            body.len(),
        ] {
            if cut > body.len() || wire[1] & 0x20 == 0 {
                continue;
            }
            let mut fast = Decompressor::new();
            let mut old = Decompressor::new();
            for prior in &WIRES[..index] {
                let mut a = Vec::new();
                let mut b = Vec::new();
                fast.decompress(prior, &mut a).unwrap();
                old_fixture(&mut old, prior, &mut b).unwrap();
                assert_eq!(a, b);
            }
            let mut a = Vec::new();
            let mut b = Vec::new();
            let actual = fast.decompress_segment(&body[..cut], &mut a);
            let expected = old.decompress_segment_reference(&body[..cut], &mut b);
            assert_eq!(
                format!("{actual:?}"),
                format!("{expected:?}"),
                "fixture={index} cut={cut}"
            );
            assert_eq!(a, b, "fixture={index} cut={cut}");
        }
    }
}

#[test]
#[ignore = "explicit paired CPU benchmark; never a physical display FPS claim"]
fn paired_zgfx_corpus_throughput() {
    use std::time::Instant;
    let mut totals = [std::time::Duration::ZERO; 2];
    let mut bytes = [0usize; 2];
    // Alternating order limits warm-cache and clock changes in the comparison.
    for round in 0..40 {
        for mode in [round % 2, 1 - round % 2] {
            let mut decoder = Decompressor::new();
            let mut output = Vec::with_capacity(32768);
            let at = Instant::now();
            for wire in WIRES {
                output.clear();
                let count = if mode == 0 {
                    old_fixture(&mut decoder, wire, &mut output).unwrap()
                } else {
                    decoder.decompress(wire, &mut output).unwrap()
                };
                bytes[mode] += count;
                std::hint::black_box(&output);
            }
            totals[mode] += at.elapsed();
        }
    }
    assert_eq!(bytes[0], bytes[1]);
    println!(
        "zgfx paired bytes={} old_ms={:.3} new_ms={:.3} ratio={:.3}; same compiler profile, CPU corpus only",
        bytes[0],
        totals[0].as_secs_f64() * 1000.,
        totals[1].as_secs_f64() * 1000.,
        totals[0].as_secs_f64() / totals[1].as_secs_f64()
    );
}
