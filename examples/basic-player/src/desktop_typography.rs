//! Measured, antialiased text for both native and WGPU bitmap overlays.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use ab_glyph_rasterizer::{Point, Rasterizer, point};
use skrifa::{DrawMemory, Font, GlyphId, PathSink, ScalerFont};

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/Ubuntu-Light.ttf");
// Media titles can contain arbitrary characters. Keep only a bounded working
// set; evicted glyphs can be rasterized again without retaining source text.
const MAX_CACHED_GLYPHS: usize = 1024;

struct Fonts {
    body: Option<Font>,
    fallback: Option<Font>,
}

struct RasterGlyph {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

type GlyphCache = HashMap<(char, u32, u32), Arc<RasterGlyph>>;

thread_local! {
    static GLYPHS: RefCell<GlyphCache> = RefCell::new(HashMap::new());
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| Fonts {
        body: Font::new(FONT_BYTES, 0),
        fallback: system_fallback(),
    })
}

fn system_fallback() -> Option<Font> {
    // Optional installed fonts support local media names without copying or
    // redistributing operating-system fonts. Never download a font at runtime.
    #[cfg(target_os = "macos")]
    let candidates = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    ]
    .map(std::path::PathBuf::from);
    #[cfg(target_os = "windows")]
    let candidates = {
        let root = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
        ["msyh.ttc", "simhei.ttf", "simsun.ttc"]
            .map(|name| std::path::PathBuf::from(&root).join("Fonts").join(name))
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let candidates = [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ]
    .map(std::path::PathBuf::from);

    candidates
        .into_iter()
        .find_map(|path| Font::new(std::fs::read(path).ok()?, 0))
}

fn glyph_id(font: &Font, character: char) -> GlyphId {
    font.charmap().map_unicode(character).unwrap_or_default()
}

fn with_font<T>(character: char, render: impl FnOnce(&Font) -> T) -> Option<T> {
    let fonts = fonts();
    let body = fonts.body.as_ref()?;
    if glyph_id(body, character) != GlyphId::default() {
        Some(render(body))
    } else if let Some(fallback) = fonts
        .fallback
        .as_ref()
        .filter(|fallback| glyph_id(fallback, character) != GlyphId::default())
    {
        Some(render(fallback))
    } else {
        // Render the font's missing-glyph box so unsupported text stays visible.
        Some(render(body))
    }
}

fn size(scale: u32) -> f32 {
    if scale <= 1 { 16.0 } else { 26.0 }
}

pub fn line_height(scale: u32) -> u32 {
    if scale <= 1 { 18 } else { 30 }
}

pub fn measure(text: &str, scale: u32) -> u32 {
    text.chars()
        .map(|character| advance(character, scale))
        .sum::<f32>()
        .ceil() as u32
}

fn advance(character: char, scale: u32) -> f32 {
    with_font(character, |font| {
        let (factor, _) = pixel_scale(font, size(scale))?;
        Some(font.glyph_metrics().h_advance(glyph_id(font, character)) * factor)
    })
    .flatten()
    .unwrap_or(0.0)
}

fn pixel_scale(font: &Font, pixel_height: f32) -> Option<(f32, f32)> {
    let line = font.metrics().h_line()?;
    let ascent = line.ascender.to_f32();
    let height = ascent - line.descender.to_f32();
    // Overlay sizes describe ascent-to-descent height, not pixels per em.
    // Keep that convention for both layout advances and outline coordinates.
    (height > 0.0).then(|| (pixel_height / height, ascent))
}

struct RasterSink {
    rasterizer: Rasterizer,
    factor: f32,
    offset: Point,
    start: Point,
    current: Point,
}

impl RasterSink {
    fn transform(&self, x: f32, y: f32) -> Point {
        point(x * self.factor, -y * self.factor) + self.offset
    }
}

impl PathSink for RasterSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.current = self.transform(x, y);
        self.start = self.current;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let end = self.transform(x, y);
        self.rasterizer.draw_line(self.current, end);
        self.current = end;
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let control = self.transform(cx, cy);
        let end = self.transform(x, y);
        self.rasterizer.draw_quad(self.current, control, end);
        self.current = end;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let control0 = self.transform(cx0, cy0);
        let control1 = self.transform(cx1, cy1);
        let end = self.transform(x, y);
        self.rasterizer
            .draw_cubic(self.current, control0, control1, end);
        self.current = end;
    }

    fn close(&mut self) {
        self.rasterizer.draw_line(self.current, self.start);
        self.current = self.start;
    }
}

fn rasterize(character: char, scale: u32, density: u32) -> Option<Arc<RasterGlyph>> {
    let key = (character, scale.min(2), density);
    if let Some(glyph) = GLYPHS.with(|cache| cache.borrow().get(&key).cloned()) {
        return Some(glyph);
    }
    let glyph = with_font(character, |font| {
        let (factor, ascent) = pixel_scale(font, size(scale) * density as f32)?;
        let baseline = ascent * factor;
        let id = glyph_id(font, character);
        let bounds = font.glyph_metrics().extents(id)?;
        let left = (bounds.x_bearing * factor).floor();
        let right = ((bounds.x_bearing + bounds.width) * factor).ceil();
        let top = (-bounds.y_bearing * factor + baseline.fract()).floor() + baseline.trunc();
        let bottom = ((bounds.height - bounds.y_bearing) * factor + baseline.fract()).ceil()
            + baseline.trunc();
        let width = (right - left) as u32;
        let height = (bottom - top) as u32;
        if width == 0 || height == 0 {
            return None;
        }
        let mut sink = RasterSink {
            rasterizer: Rasterizer::new(width as usize, height as usize),
            factor,
            offset: point(-left, baseline - top),
            start: point(0.0, 0.0),
            current: point(0.0, 0.0),
        };
        let scaler = ScalerFont::unhinted(font.clone(), None);
        scaler
            .outline(id)?
            .draw(DrawMemory::Scratch, &mut sink)
            .ok()?;
        let mut coverage = vec![0; width as usize * height as usize];
        sink.rasterizer.for_each_pixel(|index, alpha| {
            coverage[index] = (alpha * 255.0).round() as u8;
        });
        Some(Arc::new(RasterGlyph {
            left: left as i32,
            top: top as i32,
            width,
            height,
            coverage,
        }))
    })??;
    GLYPHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= MAX_CACHED_GLYPHS {
            cache.clear();
        }
        cache.insert(key, glyph.clone());
    });
    Some(glyph)
}

pub fn paint(text: &str, scale: u32, density: u32, mut pixel: impl FnMut(i32, i32, u8)) {
    let mut cursor = 0.0_f32;
    for character in text.chars() {
        if let Some(glyph) = rasterize(character, scale, density) {
            let origin_x = cursor.round() as i32 + glyph.left;
            for y in 0..glyph.height {
                for x in 0..glyph.width {
                    let alpha = glyph.coverage[y as usize * glyph.width as usize + x as usize];
                    if alpha > 0 {
                        pixel(origin_x + x as i32, glyph.top + y as i32, alpha);
                    }
                }
            }
        }
        cursor += advance(character, scale) * density as f32;
    }
}

#[cfg(test)]
mod tests;
