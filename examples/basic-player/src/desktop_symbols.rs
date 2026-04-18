use crate::desktop_ui::DesktopUiRect;

const SYMBOL_VIEWBOX: f32 = 24.0;
const SYMBOL_SAMPLES: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSymbol {
    Play,
    Pause,
    Stop,
    SeekBack,
    SeekForward,
    SeekStart,
    SeekEnd,
    FolderOpen,
    Stream,
    DashGrid,
    Playlist,
    Download,
    Export,
    Remove,
    Magic,
    CheckCircle,
    AlertTriangle,
    VideoStack,
    Waveform,
    LocalLibrary,
}

#[derive(Clone, Copy)]
struct Point {
    x: f32,
    y: f32,
}

#[derive(Clone, Copy)]
enum SymbolPrimitive {
    FillTriangle {
        a: Point,
        b: Point,
        c: Point,
    },
    FillRoundedRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
    },
    FillCircle {
        center: Point,
        radius: f32,
    },
    StrokeLine {
        from: Point,
        to: Point,
        width: f32,
    },
    StrokeRoundedRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        stroke: f32,
    },
    StrokeCircle {
        center: Point,
        radius: f32,
        stroke: f32,
    },
}

pub fn draw_symbol(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    symbol: DesktopSymbol,
    color: [u8; 4],
) {
    if rect.width == 0 || rect.height == 0 || color[3] == 0 {
        return;
    }

    match symbol {
        DesktopSymbol::Play => {
            let primitives = [SymbolPrimitive::FillTriangle {
                a: point(8.0, 5.0),
                b: point(18.0, 12.0),
                c: point(8.0, 19.0),
            }];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::Pause => {
            let primitives = [
                SymbolPrimitive::FillRoundedRect {
                    x: 7.0,
                    y: 5.0,
                    width: 3.5,
                    height: 14.0,
                    radius: 1.4,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 13.5,
                    y: 5.0,
                    width: 3.5,
                    height: 14.0,
                    radius: 1.4,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::Stop => {
            let primitives = [SymbolPrimitive::FillRoundedRect {
                x: 7.0,
                y: 7.0,
                width: 10.0,
                height: 10.0,
                radius: 2.0,
            }];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::SeekBack => {
            let primitives = [
                SymbolPrimitive::FillTriangle {
                    a: point(16.0, 5.0),
                    b: point(7.0, 12.0),
                    c: point(16.0, 19.0),
                },
                SymbolPrimitive::FillTriangle {
                    a: point(21.0, 5.0),
                    b: point(12.0, 12.0),
                    c: point(21.0, 19.0),
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::SeekForward => {
            let primitives = [
                SymbolPrimitive::FillTriangle {
                    a: point(8.0, 5.0),
                    b: point(17.0, 12.0),
                    c: point(8.0, 19.0),
                },
                SymbolPrimitive::FillTriangle {
                    a: point(3.0, 5.0),
                    b: point(12.0, 12.0),
                    c: point(3.0, 19.0),
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::SeekStart => {
            let primitives = [
                SymbolPrimitive::FillRoundedRect {
                    x: 4.0,
                    y: 5.0,
                    width: 2.0,
                    height: 14.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillTriangle {
                    a: point(18.0, 5.0),
                    b: point(8.0, 12.0),
                    c: point(18.0, 19.0),
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::SeekEnd => {
            let primitives = [
                SymbolPrimitive::FillRoundedRect {
                    x: 18.0,
                    y: 5.0,
                    width: 2.0,
                    height: 14.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillTriangle {
                    a: point(6.0, 5.0),
                    b: point(16.0, 12.0),
                    c: point(6.0, 19.0),
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::FolderOpen => {
            let primitives = [
                SymbolPrimitive::StrokeLine {
                    from: point(4.5, 10.0),
                    to: point(9.0, 10.0),
                    width: 1.7,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(9.0, 10.0),
                    to: point(11.0, 7.5),
                    width: 1.7,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(11.0, 7.5),
                    to: point(19.5, 7.5),
                    width: 1.7,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(4.5, 10.0),
                    to: point(4.5, 18.0),
                    width: 1.7,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(4.5, 18.0),
                    to: point(18.0, 18.0),
                    width: 1.7,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(18.0, 18.0),
                    to: point(20.0, 11.0),
                    width: 1.7,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(20.0, 11.0),
                    to: point(6.5, 11.0),
                    width: 1.7,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::Stream => {
            let primitives = [
                SymbolPrimitive::FillRoundedRect {
                    x: 4.0,
                    y: 14.0,
                    width: 3.0,
                    height: 5.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 9.0,
                    y: 10.0,
                    width: 3.0,
                    height: 9.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 14.0,
                    y: 6.0,
                    width: 3.0,
                    height: 13.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 19.0,
                    y: 11.0,
                    width: 1.5,
                    height: 8.0,
                    radius: 0.8,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::DashGrid => {
            let primitives = [
                SymbolPrimitive::FillRoundedRect {
                    x: 4.0,
                    y: 5.0,
                    width: 6.0,
                    height: 6.0,
                    radius: 1.6,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 14.0,
                    y: 5.0,
                    width: 6.0,
                    height: 6.0,
                    radius: 1.6,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 4.0,
                    y: 13.0,
                    width: 6.0,
                    height: 6.0,
                    radius: 1.6,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 14.0,
                    y: 13.0,
                    width: 6.0,
                    height: 6.0,
                    radius: 1.6,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::Playlist => {
            let primitives = [
                SymbolPrimitive::FillTriangle {
                    a: point(4.0, 8.0),
                    b: point(9.0, 11.5),
                    c: point(4.0, 15.0),
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 11.0,
                    y: 7.0,
                    width: 9.5,
                    height: 2.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 11.0,
                    y: 11.0,
                    width: 8.0,
                    height: 2.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 11.0,
                    y: 15.0,
                    width: 10.5,
                    height: 2.0,
                    radius: 1.0,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::Download => {
            let primitives = [
                SymbolPrimitive::StrokeLine {
                    from: point(12.0, 5.0),
                    to: point(12.0, 14.0),
                    width: 1.9,
                },
                SymbolPrimitive::FillTriangle {
                    a: point(7.0, 12.5),
                    b: point(17.0, 12.5),
                    c: point(12.0, 18.0),
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 5.5,
                    y: 19.0,
                    width: 13.0,
                    height: 2.0,
                    radius: 1.0,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::Export => {
            let primitives = [
                SymbolPrimitive::StrokeRoundedRect {
                    x: 4.5,
                    y: 8.0,
                    width: 10.5,
                    height: 11.5,
                    radius: 2.0,
                    stroke: 1.6,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(12.0, 12.0),
                    to: point(19.5, 4.5),
                    width: 1.8,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(15.0, 4.5),
                    to: point(19.5, 4.5),
                    width: 1.8,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(19.5, 4.5),
                    to: point(19.5, 9.0),
                    width: 1.8,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::Remove => {
            let primitives = [
                SymbolPrimitive::StrokeLine {
                    from: point(6.0, 6.0),
                    to: point(18.0, 18.0),
                    width: 1.8,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(18.0, 6.0),
                    to: point(6.0, 18.0),
                    width: 1.8,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::Magic => {
            let primitives = [
                SymbolPrimitive::FillCircle {
                    center: point(12.0, 12.0),
                    radius: 2.4,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(12.0, 3.5),
                    to: point(12.0, 7.0),
                    width: 1.6,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(12.0, 17.0),
                    to: point(12.0, 20.5),
                    width: 1.6,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(3.5, 12.0),
                    to: point(7.0, 12.0),
                    width: 1.6,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(17.0, 12.0),
                    to: point(20.5, 12.0),
                    width: 1.6,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(6.0, 6.0),
                    to: point(8.5, 8.5),
                    width: 1.6,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(15.5, 15.5),
                    to: point(18.0, 18.0),
                    width: 1.6,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(6.0, 18.0),
                    to: point(8.5, 15.5),
                    width: 1.6,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(15.5, 8.5),
                    to: point(18.0, 6.0),
                    width: 1.6,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::CheckCircle => {
            let primitives = [
                SymbolPrimitive::StrokeCircle {
                    center: point(12.0, 12.0),
                    radius: 8.0,
                    stroke: 1.8,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(7.6, 12.1),
                    to: point(10.5, 15.0),
                    width: 1.9,
                },
                SymbolPrimitive::StrokeLine {
                    from: point(10.5, 15.0),
                    to: point(16.8, 8.6),
                    width: 1.9,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::AlertTriangle => {
            let primitives = [
                SymbolPrimitive::FillTriangle {
                    a: point(12.0, 4.0),
                    b: point(20.0, 19.0),
                    c: point(4.0, 19.0),
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 11.0,
                    y: 8.0,
                    width: 2.0,
                    height: 6.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillCircle {
                    center: point(12.0, 16.5),
                    radius: 1.2,
                },
            ];
            draw_symbol_primitives(
                frame,
                frame_width,
                frame_height,
                rect,
                &primitives,
                [color[0], color[1], color[2], color[3]],
            );
        }
        DesktopSymbol::VideoStack => {
            let primitives = [
                SymbolPrimitive::StrokeRoundedRect {
                    x: 5.0,
                    y: 8.0,
                    width: 12.0,
                    height: 9.5,
                    radius: 2.0,
                    stroke: 1.5,
                },
                SymbolPrimitive::StrokeRoundedRect {
                    x: 8.0,
                    y: 5.0,
                    width: 12.0,
                    height: 9.5,
                    radius: 2.0,
                    stroke: 1.5,
                },
                SymbolPrimitive::FillTriangle {
                    a: point(12.0, 8.5),
                    b: point(15.7, 10.8),
                    c: point(12.0, 13.1),
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::Waveform => {
            let primitives = [
                SymbolPrimitive::FillRoundedRect {
                    x: 4.0,
                    y: 11.0,
                    width: 2.0,
                    height: 4.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 7.5,
                    y: 8.0,
                    width: 2.0,
                    height: 10.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 11.0,
                    y: 5.5,
                    width: 2.0,
                    height: 15.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 14.5,
                    y: 8.0,
                    width: 2.0,
                    height: 10.0,
                    radius: 1.0,
                },
                SymbolPrimitive::FillRoundedRect {
                    x: 18.0,
                    y: 11.0,
                    width: 2.0,
                    height: 4.0,
                    radius: 1.0,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
        DesktopSymbol::LocalLibrary => {
            let primitives = [
                SymbolPrimitive::StrokeRoundedRect {
                    x: 4.5,
                    y: 5.0,
                    width: 5.0,
                    height: 14.0,
                    radius: 1.8,
                    stroke: 1.4,
                },
                SymbolPrimitive::StrokeRoundedRect {
                    x: 9.5,
                    y: 5.0,
                    width: 5.5,
                    height: 14.0,
                    radius: 1.8,
                    stroke: 1.4,
                },
                SymbolPrimitive::StrokeRoundedRect {
                    x: 15.0,
                    y: 7.0,
                    width: 4.5,
                    height: 12.0,
                    radius: 1.6,
                    stroke: 1.4,
                },
            ];
            draw_symbol_primitives(frame, frame_width, frame_height, rect, &primitives, color);
        }
    }
}

fn point(x: f32, y: f32) -> Point {
    Point { x, y }
}

fn draw_symbol_primitives(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    rect: DesktopUiRect,
    primitives: &[SymbolPrimitive],
    color: [u8; 4],
) {
    let draw_size = rect.width.min(rect.height) as f32;
    if draw_size <= 0.0 {
        return;
    }
    let scale = draw_size / SYMBOL_VIEWBOX;
    let offset_x = rect.x as f32 + (rect.width as f32 - draw_size) / 2.0;
    let offset_y = rect.y as f32 + (rect.height as f32 - draw_size) / 2.0;
    let sample_count = (SYMBOL_SAMPLES * SYMBOL_SAMPLES) as f32;

    for y in rect.y..rect.y.saturating_add(rect.height).min(frame_height) {
        for x in rect.x..rect.x.saturating_add(rect.width).min(frame_width) {
            let mut covered = 0_u32;
            for sample_y in 0..SYMBOL_SAMPLES {
                for sample_x in 0..SYMBOL_SAMPLES {
                    let px = x as f32 + (sample_x as f32 + 0.5) / SYMBOL_SAMPLES as f32;
                    let py = y as f32 + (sample_y as f32 + 0.5) / SYMBOL_SAMPLES as f32;
                    let local_x = (px - offset_x) / scale;
                    let local_y = (py - offset_y) / scale;
                    if !(0.0..=SYMBOL_VIEWBOX).contains(&local_x)
                        || !(0.0..=SYMBOL_VIEWBOX).contains(&local_y)
                    {
                        continue;
                    }
                    if primitives
                        .iter()
                        .any(|primitive| primitive_contains(*primitive, local_x, local_y))
                    {
                        covered = covered.saturating_add(1);
                    }
                }
            }
            if covered == 0 {
                continue;
            }
            let alpha = (f32::from(color[3]) * (covered as f32 / sample_count)).round() as u8;
            blend_pixel(
                frame,
                frame_width,
                frame_height,
                x,
                y,
                [color[0], color[1], color[2], alpha],
            );
        }
    }
}

fn primitive_contains(primitive: SymbolPrimitive, x: f32, y: f32) -> bool {
    match primitive {
        SymbolPrimitive::FillTriangle { a, b, c } => point_in_triangle(point(x, y), a, b, c),
        SymbolPrimitive::FillRoundedRect {
            x: left,
            y: top,
            width,
            height,
            radius,
        } => point_in_rounded_rect(point(x, y), left, top, width, height, radius),
        SymbolPrimitive::FillCircle { center, radius } => {
            let dx = x - center.x;
            let dy = y - center.y;
            dx * dx + dy * dy <= radius * radius
        }
        SymbolPrimitive::StrokeLine { from, to, width } => {
            distance_to_segment(point(x, y), from, to) <= width / 2.0
        }
        SymbolPrimitive::StrokeRoundedRect {
            x: left,
            y: top,
            width,
            height,
            radius,
            stroke,
        } => {
            point_in_rounded_rect(point(x, y), left, top, width, height, radius)
                && !point_in_rounded_rect(
                    point(x, y),
                    left + stroke,
                    top + stroke,
                    width - stroke * 2.0,
                    height - stroke * 2.0,
                    (radius - stroke).max(0.0),
                )
        }
        SymbolPrimitive::StrokeCircle {
            center,
            radius,
            stroke,
        } => {
            let dx = x - center.x;
            let dy = y - center.y;
            let distance = (dx * dx + dy * dy).sqrt();
            distance <= radius && distance >= (radius - stroke).max(0.0)
        }
    }
}

fn point_in_triangle(point: Point, a: Point, b: Point, c: Point) -> bool {
    let d1 = signed_area(point, a, b);
    let d2 = signed_area(point, b, c);
    let d3 = signed_area(point, c, a);
    let has_negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_negative && has_positive)
}

fn signed_area(point: Point, a: Point, b: Point) -> f32 {
    (point.x - b.x) * (a.y - b.y) - (a.x - b.x) * (point.y - b.y)
}

fn point_in_rounded_rect(
    point: Point,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
) -> bool {
    if width <= 0.0 || height <= 0.0 {
        return false;
    }
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    if radius <= f32::EPSILON {
        return point.x >= x && point.x <= x + width && point.y >= y && point.y <= y + height;
    }

    let right = x + width;
    let bottom = y + height;
    let within_horizontal = point.x >= x + radius && point.x <= right - radius;
    let within_vertical = point.y >= y + radius && point.y <= bottom - radius;
    if within_horizontal || within_vertical {
        return point.x >= x && point.x <= right && point.y >= y && point.y <= bottom;
    }

    let corner_x = if point.x < x + radius {
        x + radius
    } else {
        right - radius
    };
    let corner_y = if point.y < y + radius {
        y + radius
    } else {
        bottom - radius
    };
    let dx = point.x - corner_x;
    let dy = point.y - corner_y;
    dx * dx + dy * dy <= radius * radius
}

fn distance_to_segment(point: Point, start: Point, end: Point) -> f32 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    if dx.abs() <= f32::EPSILON && dy.abs() <= f32::EPSILON {
        return ((point.x - start.x).powi(2) + (point.y - start.y).powi(2)).sqrt();
    }
    let t = (((point.x - start.x) * dx) + ((point.y - start.y) * dy)) / (dx * dx + dy * dy);
    let t = t.clamp(0.0, 1.0);
    let nearest_x = start.x + t * dx;
    let nearest_y = start.y + t * dy;
    ((point.x - nearest_x).powi(2) + (point.y - nearest_y).powi(2)).sqrt()
}

fn blend_pixel(
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: u32,
    y: u32,
    color: [u8; 4],
) {
    if x >= frame_width || y >= frame_height {
        return;
    }

    let index = ((y * frame_width + x) * 4) as usize;
    let alpha = f32::from(color[3]) / 255.0;
    let inverse = 1.0 - alpha;
    frame[index] = (f32::from(color[0]) * alpha + f32::from(frame[index]) * inverse)
        .round()
        .clamp(0.0, 255.0) as u8;
    frame[index + 1] = (f32::from(color[1]) * alpha + f32::from(frame[index + 1]) * inverse)
        .round()
        .clamp(0.0, 255.0) as u8;
    frame[index + 2] = (f32::from(color[2]) * alpha + f32::from(frame[index + 2]) * inverse)
        .round()
        .clamp(0.0, 255.0) as u8;
    frame[index + 3] = 255;
}
