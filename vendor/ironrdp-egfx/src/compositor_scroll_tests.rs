use super::*;

#[test]
fn scrolling_matches_snapshot_for_overlaps_clipping_and_full_width_moves() {
    const WIDTH: u16 = 8;
    const HEIGHT: u16 = 7;
    let original: Vec<u8> = (0..usize::from(WIDTH) * usize::from(HEIGHT) * BYTES_PER_PIXEL)
        .map(|index| u8::try_from((index * 71 + index / 13) % 256).unwrap())
        .collect();
    for sy in 0..=HEIGHT {
        for sx in 0..=WIDTH {
            for dy in 0..=HEIGHT + 1 {
                for dx in 0..=WIDTH + 1 {
                    for (w, h) in [(0, 0), (1, 1), (3, 4), (WIDTH, HEIGHT), (u16::MAX, u16::MAX)] {
                        let w = w.min(WIDTH.saturating_sub(sx));
                        let h = h.min(HEIGHT.saturating_sub(sy));
                        let tile = copy_region(&original, WIDTH, sx, sy, w, h);
                        let mut expected = original.clone();
                        blit_region(
                            CanvasMut {
                                data: &mut expected,
                                width: WIDTH,
                                height: HEIGHT,
                            },
                            dx,
                            dy,
                            w,
                            h,
                            &tile,
                        );
                        let mut actual = original.clone();
                        move_region(
                            CanvasMut {
                                data: &mut actual,
                                width: WIDTH,
                                height: HEIGHT,
                            },
                            (sx, sy),
                            (w, h),
                            &Point { x: dx, y: dy },
                        );
                        assert_eq!(
                            actual, expected,
                            "source=({sx},{sy}) size=({w},{h}) destination=({dx},{dy})"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn multiple_destinations_keep_the_original_source_snapshot() {
    let mut compositor = Compositor::default();
    compositor.reset(8, 4);
    compositor.create_surface(1, 8, 4);
    compositor.map_surface(1, 0, 0);
    let original: Vec<u8> = (0_u8..128).collect();
    compositor.surfaces.get_mut(&1).unwrap().data.clone_from(&original);
    let source = ExclusiveRectangle {
        left: 0,
        top: 0,
        right: 4,
        bottom: 3,
    };
    let points = [Point { x: 1, y: 1 }, Point { x: 4, y: 0 }];
    let tile = copy_region(&original, 8, 0, 0, 4, 3);
    let mut expected = original;
    for point in &points {
        blit_region(
            CanvasMut {
                data: &mut expected,
                width: 8,
                height: 4,
            },
            point.x,
            point.y,
            4,
            3,
            &tile,
        );
    }
    compositor.surface_to_surface(1, 1, &source, &points);
    assert_eq!(compositor.surfaces[&1].data, expected);
    compositor.end_frame();
    assert!(!compositor.drain_output().is_empty());
}

#[test]
fn single_destination_scroll_commits_the_final_destination_pixels() {
    for point in [Point { x: 0, y: 0 }, Point { x: 3, y: 2 }, Point { x: 7, y: 3 }] {
        let mut compositor = Compositor::default();
        compositor.reset(8, 4);
        compositor.create_surface(1, 8, 4);
        compositor.map_surface(1, 0, 0);
        compositor.end_frame();
        let _ = compositor.drain_output();
        let original: Vec<u8> = (0_u8..128).collect();
        compositor.surfaces.get_mut(&1).unwrap().data.clone_from(&original);
        let source = ExclusiveRectangle {
            left: 1,
            top: 1,
            right: 7,
            bottom: 4,
        };
        let tile = copy_region(&original, 8, 1, 1, 6, 3);
        let mut expected = original;
        blit_region(
            CanvasMut {
                data: &mut expected,
                width: 8,
                height: 4,
            },
            point.x,
            point.y,
            6,
            3,
            &tile,
        );
        compositor.surface_to_surface(1, 1, &source, &[point.clone()]);
        assert_eq!(compositor.surfaces[&1].data, expected);
        compositor.end_frame();
        let updates = compositor.drain_output();
        assert_eq!(updates.len(), 1);
        let update = &updates[0];
        assert_eq!(update.region.left, point.x);
        assert_eq!(update.region.top, point.y);
        assert_eq!(
            update.data,
            copy_region(&expected, 8, point.x, point.y, 6.min(8 - point.x), 3.min(4 - point.y))
        );
    }
}
