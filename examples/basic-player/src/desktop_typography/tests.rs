use super::*;

#[test]
fn bundled_font_keeps_existing_text_widths() {
    // Reference measurements from the bundled font before the parser migration.
    for (text, small, large) in [
        ("Vesper Player", 87, 142),
        ("Café — 01.mp4", 99, 160),
        ("Agjp", 30, 48),
        (" ", 4, 6),
    ] {
        assert_eq!(measure(text, 1), small, "small {text:?}");
        assert_eq!(measure(text, 2), large, "large {text:?}");
    }
    assert_eq!(line_height(1), 18);
    assert_eq!(line_height(2), 30);
}

#[test]
fn glyph_masks_keep_the_baseline_and_display_density() {
    for (scale, density, expected) in [
        (1, 1, [0, 3, 28, 15]),
        (1, 2, [0, 5, 56, 31]),
        (2, 1, [0, 4, 46, 25]),
        (2, 2, [0, 9, 91, 51]),
    ] {
        let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        let mut antialiased = false;
        paint("Agjp", scale, density, |x, y, alpha| {
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x),
                bounds[3].max(y),
            ];
            antialiased |= alpha > 0 && alpha < 255;
        });
        assert_eq!(bounds, expected, "scale {scale}, density {density}");
        assert!(antialiased, "glyph edges must retain coverage antialiasing");
    }
}

#[test]
fn whitespace_advances_and_missing_characters_stay_visible() {
    assert_eq!(measure("", 1), 0);
    assert!(measure("   ", 1) > 0);
    paint("   ", 1, 2, |_, _, _| panic!("spaces must not paint ink"));

    let unsupported = '\u{10ffff}';
    let mut pixels = 0;
    paint(&unsupported.to_string(), 1, 1, |_, _, _| pixels += 1);
    assert!(
        pixels > 0,
        "unsupported characters must draw the missing-glyph box"
    );
    assert!(measure(&unsupported.to_string(), 1) > 0);
}

#[test]
fn closed_contours_preserve_glyph_counters() {
    let mut sink = RasterSink {
        rasterizer: Rasterizer::new(8, 8),
        factor: 1.0,
        offset: point(0.0, 0.0),
        start: point(0.0, 0.0),
        current: point(0.0, 0.0),
    };
    sink.move_to(1.0, -1.0);
    sink.line_to(7.0, -1.0);
    sink.line_to(7.0, -7.0);
    sink.line_to(1.0, -7.0);
    sink.close();
    // A counter winds in the opposite direction to leave the center empty.
    sink.move_to(3.0, -3.0);
    sink.line_to(3.0, -5.0);
    sink.line_to(5.0, -5.0);
    sink.line_to(5.0, -3.0);
    sink.close();

    let mut coverage = [0.0; 64];
    sink.rasterizer
        .for_each_pixel(|index, alpha| coverage[index] = alpha);
    assert_eq!(coverage[2 * 8 + 2], 1.0);
    assert_eq!(coverage[3 * 8 + 3], 0.0);
    assert_eq!(coverage[0], 0.0);
    assert_eq!(coverage.iter().sum::<f32>(), 32.0);
}
