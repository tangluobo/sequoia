// Scalar pre-optimization inverse transform, retained independently for differential tests.
pub(super) fn decode(buffer: &mut [i16], temp: &mut [i16]) {
    assert!(buffer.len() >= 4096, "buffer must hold 4096 coefficients");
    assert!(temp.len() >= 4096, "temp must hold 4096 elements");

    // Inner-to-outer: level 3 first (smallest subbands), then 2, then 1
    decode_block(&mut buffer[3807..], temp, 9, 8);
    decode_block(&mut buffer[3007..], temp, 17, 16);
    decode_block(buffer, temp, 33, 31);
}

#[expect(
    clippy::similar_names,
    reason = "hl/lh/hh/ll are standard DWT subband names"
)]
fn decode_block(buffer: &mut [i16], temp: &mut [i16], n_l: usize, n_h: usize) {
    let dst_w = n_l + n_h;

    // Subband offsets within this block's buffer region
    let hl_off = 0;
    let lh_off = n_h * n_l;
    let hh_off = lh_off + n_l * n_h;
    let ll_off = hh_off + n_h * n_h;

    // Temp: L half (n_l rows x dst_w cols) then H half (n_h rows x dst_w cols)
    let l_off = 0;
    let h_off = n_l * dst_w;

    // Horizontal inverse: (LL + HL) -> L rows
    for row in 0..n_l {
        let ll_start = ll_off + row * n_l;
        let hl_start = hl_off + row * n_h;
        let l_start = l_off + row * dst_w;
        idwt_row(
            &buffer[ll_start..ll_start + n_l],
            &buffer[hl_start..hl_start + n_h],
            &mut temp[l_start..l_start + dst_w],
        );
    }

    // Horizontal inverse: (LH + HH) -> H rows
    for row in 0..n_h {
        let lh_start = lh_off + row * n_l;
        let hh_start = hh_off + row * n_h;
        let h_start = h_off + row * dst_w;
        idwt_row(
            &buffer[lh_start..lh_start + n_l],
            &buffer[hh_start..hh_start + n_h],
            &mut temp[h_start..h_start + dst_w],
        );
    }

    // Vertical inverse: (L + H) -> reconstructed columns in buffer
    for col in 0..dst_w {
        idwt_col(
            temp,
            l_off + col,
            h_off + col,
            dst_w,
            buffer,
            col,
            dst_w,
            n_l,
            n_h,
        );
    }
}

/// 1D inverse DWT on a contiguous row.
///
/// Reconstructs `n_l + n_h` output samples from `n_l` low-pass and `n_h`
/// high-pass coefficients.
fn idwt_row(low: &[i16], high: &[i16], dst: &mut [i16]) {
    let n_l = low.len();
    let n_h = high.len();

    let mut h0 = i32::from(high[0]);
    let mut x0 = t(i32::from(low[0]) - h0);
    let mut x2 = x0;

    let mut di = 0;

    for j in 0..n_h - 1 {
        let h1 = i32::from(high[j + 1]);
        let l_val = i32::from(low[j + 1]);
        x2 = t(l_val - (h0 + h1) / 2);
        let x1 = t((i32::from(x0) + i32::from(x2)) / 2 + 2 * h0);
        dst[di] = x0;
        dst[di + 1] = x1;
        di += 2;
        x0 = x2;
        h0 = h1;
    }

    if n_l <= n_h + 1 {
        if n_l <= n_h {
            dst[di] = x2;
            dst[di + 1] = t(i32::from(x2) + 2 * h0);
        } else {
            let x_new = t(i32::from(low[n_h]) - h0);
            dst[di] = x2;
            dst[di + 1] = t((i32::from(x_new) + i32::from(x2)) / 2 + 2 * h0);
            dst[di + 2] = x_new;
        }
    } else {
        let x_new = t(i32::from(low[n_h]) - h0 / 2);
        dst[di] = x2;
        dst[di + 1] = t((i32::from(x_new) + i32::from(x2)) / 2 + 2 * h0);
        dst[di + 2] = x_new;
        dst[di + 3] = t((i32::from(x_new) + i32::from(low[n_h + 1])) / 2);
    }
}

/// 1D inverse DWT on a strided column.
///
/// Reads from `src` with stride, writes to `dst` with stride.
#[expect(clippy::too_many_arguments)]
fn idwt_col(
    src: &[i16],
    l_start: usize,
    h_start: usize,
    src_stride: usize,
    dst: &mut [i16],
    d_start: usize,
    dst_stride: usize,
    n_l: usize,
    n_h: usize,
) {
    let l = |i: usize| i32::from(src[l_start + i * src_stride]);
    let h = |i: usize| i32::from(src[h_start + i * src_stride]);

    let mut h0 = h(0);
    let mut x0 = t(l(0) - h0);
    let mut x2 = x0;

    let mut d = d_start;

    for j in 0..n_h - 1 {
        let h1 = h(j + 1);
        x2 = t(l(j + 1) - (h0 + h1) / 2);
        let x1 = t((i32::from(x0) + i32::from(x2)) / 2 + 2 * h0);
        dst[d] = x0;
        d += dst_stride;
        dst[d] = x1;
        d += dst_stride;
        x0 = x2;
        h0 = h1;
    }

    if n_l <= n_h + 1 {
        if n_l <= n_h {
            dst[d] = x2;
            d += dst_stride;
            dst[d] = t(i32::from(x2) + 2 * h0);
        } else {
            let x_new = t(l(n_h) - h0);
            dst[d] = x2;
            d += dst_stride;
            dst[d] = t((i32::from(x_new) + i32::from(x2)) / 2 + 2 * h0);
            d += dst_stride;
            dst[d] = x_new;
        }
    } else {
        let x_new = t(l(n_h) - h0 / 2);
        dst[d] = x2;
        d += dst_stride;
        dst[d] = t((i32::from(x_new) + i32::from(x2)) / 2 + 2 * h0);
        d += dst_stride;
        dst[d] = x_new;
        d += dst_stride;
        dst[d] = t((i32::from(x_new) + l(n_h + 1)) / 2);
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "intentional truncation matching existing DWT convention"
)]
fn t(value: i32) -> i16 {
    value as i16
}
