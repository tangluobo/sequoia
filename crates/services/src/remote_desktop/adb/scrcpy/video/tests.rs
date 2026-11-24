use super::*;
#[tokio::test]
async fn java_video_sample_decodes_and_backlog_recovers_only_at_keyframe() -> Result<()> {
    let mut sample = include_bytes!("../../../../../tests/fixtures/adb/red64x48.scrcpy").as_slice();
    let mut decoder = Decoder::new()?;
    let queue = Queue::default();
    while !sample.is_empty() {
        queue.offer(packet(&mut sample).await?)?;
    }
    let (encoded, _) = queue.take()?.context("Missing Java sample frame")?;
    validate_sps(&encoded.data)?;
    let image = decoder
        .decode(&encoded.data)?
        .context("Missing decoded Java sample")?;
    assert_eq!(image.dimensions(), (64, 48));
    let mut rgba = vec![0; 64 * 48 * 4];
    image.write_rgba8(&mut rgba);
    assert!(rgba[0] > 240 && rgba[1] < 15 && rgba[2] < 15 && rgba[3] == 255);
    let packed = packed_yuv(&image)?;
    assert_eq!(
        [
            packed.planes[0].len(),
            packed.planes[1].len(),
            packed.planes[2].len()
        ],
        [64 * 48, 32 * 24, 32 * 24]
    );
    for (index, (plane, stride, w, h)) in [
        (image.y(), image.strides().0, 64, 48),
        (image.u(), image.strides().1, 32, 24),
        (image.v(), image.strides().2, 32, 24),
    ]
    .into_iter()
    .enumerate()
    {
        for row in 0..h {
            assert_eq!(
                &packed.planes[index][row * w..row * w + w],
                &plane[row * stride..row * stride + w]
            );
        }
    }
    let p = |pts, key| Packet {
        pts,
        data: vec![0],
        key,
        config: false,
        reset: false,
    };
    queue.offer(p(0, false))?;
    queue.offer(p(300_000, false))?;
    assert!(!queue.pending());
    queue.offer(p(310_000, false))?;
    assert!(!queue.pending());
    queue.offer(p(320_000, true))?;
    let (frame, resets) = queue.take()?.unwrap();
    assert!(frame.reset);
    assert_eq!(resets, 1);
    queue.close();
    assert!(queue.take()?.is_none());
    Ok(())
}
#[tokio::test]
async fn video_rejects_oversized_packet_and_dimensions_before_allocation() {
    let mut bytes = [0u8; 12];
    bytes[8..].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(packet(&mut bytes.as_slice()).await.is_err());
    assert!(dimensions(8192, 8192).is_err());
    assert!(dimensions(0, 1080).is_err());
    assert!(dimensions(1920, 1080).is_ok());
}

#[test]
fn packed_i420_removes_padding_without_mixing_rows_or_chroma() -> Result<()> {
    let mut y = vec![255; 12 * 4];
    let mut u = vec![255; 8 * 2];
    let mut v = vec![255; 8 * 2];
    for row in 0..4 {
        for x in 0..8 {
            y[row * 12 + x] = (row * 8 + x) as u8;
        }
    }
    for row in 0..2 {
        for x in 0..4 {
            u[row * 8 + x] = (64 + row * 4 + x) as u8;
            v[row * 8 + x] = (128 + row * 4 + x) as u8;
        }
    }
    let source = openh264::formats::YUVSlices::new((&y, &u, &v), (8, 4), (12, 8, 8));
    let packed = packed_yuv(&source)?;
    assert_eq!(
        packed.planes,
        [
            (0..32).collect::<Vec<u8>>(),
            (64..72).collect(),
            (128..136).collect()
        ]
    );
    Ok(())
}
