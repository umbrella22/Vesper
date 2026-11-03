//! Measured, antialiased text for both native and WGPU bitmap overlays.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/Ubuntu-Light.ttf");
// Media titles can contain arbitrary characters. Keep only a bounded working
// set; evicted glyphs can be rasterized again without retaining source text.
const MAX_CACHED_GLYPHS: usize = 1024;

struct Fonts {
    body: Option<FontArc>,
    fallback: Option<FontArc>,
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
        body: FontArc::try_from_slice(FONT_BYTES).ok(),
        fallback: system_fallback(),
    })
}

fn system_fallback() -> Option<FontArc> {
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
        .find_map(|path| FontArc::try_from_vec(std::fs::read(path).ok()?).ok())
}

fn with_font<T>(character: char, render: impl FnOnce(&FontArc) -> T) -> Option<T> {
    let fonts = fonts();
    let body = fonts.body.as_ref()?;
    if body.glyph_id(character).0 != 0 {
        Some(render(body))
    } else if let Some(fallback) = fonts
        .fallback
        .as_ref()
        .filter(|fallback| fallback.glyph_id(character).0 != 0)
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
        let scaled = font.as_scaled(PxScale::from(size(scale)));
        scaled.h_advance(font.glyph_id(character))
    })
    .unwrap_or(0.0)
}

fn rasterize(character: char, scale: u32, density: u32) -> Option<Arc<RasterGlyph>> {
    let key = (character, scale.min(2), density);
    if let Some(glyph) = GLYPHS.with(|cache| cache.borrow().get(&key).cloned()) {
        return Some(glyph);
    }
    let glyph = with_font(character, |font| {
        let scaled = font.as_scaled(PxScale::from(size(scale) * density as f32));
        let glyph = font
            .glyph_id(character)
            .with_scale_and_position(size(scale) * density as f32, point(0.0, scaled.ascent()));
        let outlined = font.outline_glyph(glyph)?;
        let bounds = outlined.px_bounds();
        let width = bounds.width() as u32;
        let height = bounds.height() as u32;
        let mut coverage = vec![0; width as usize * height as usize];
        outlined.draw(|x, y, alpha| {
            coverage[y as usize * width as usize + x as usize] = (alpha * 255.0).round() as u8;
        });
        Some(Arc::new(RasterGlyph {
            left: bounds.min.x as i32,
            top: bounds.min.y as i32,
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
