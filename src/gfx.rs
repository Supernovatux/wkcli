//! Terminal graphics: radicals that have no unicode character, and large kanji / vocabulary.
//!
//! Konsole, kitty and Ghostty get the kitty graphics protocol written directly (plain
//! placements, which Konsole supports). Other terminals go through `ratatui-image`
//! (sixel / iTerm2 / unicode half-blocks as the last resort).

use image::{DynamicImage, RgbaImage};
use ratatui::crossterm::terminal::window_size;
use ratatui::layout::{Rect, Size};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::Resize;
use resvg::{tiny_skia, usvg};
use std::io::{self, Write};

pub type Rgb = [u8; 3];

enum Mode {
    Kitty,
    Widget(Picker),
}

/// What should be on screen: (subject id, cell area).
pub type Slot = (u64, Rect);

pub struct Gfx {
    mode: Mode,
    cell: (u16, u16),
    /// SVG options, carrying the system fonts when big text is enabled.
    options: usvg::Options<'static>,
    /// Characters are drawn as images (needs real pixel graphics and a font).
    big_text: bool,
    font: String,
    /// Kitty mode: the image currently placed on screen.
    drawn: Option<Slot>,
    /// Widget mode: the prepared image for the current slot.
    cached: Option<(Slot, Protocol)>,
}

fn cell_size() -> Option<(u16, u16)> {
    let ws = window_size().ok()?;
    if ws.width == 0 || ws.height == 0 || ws.columns == 0 || ws.rows == 0 {
        return None;
    }
    Some((ws.width / ws.columns, ws.height / ws.rows))
}

fn env_set(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty())
}

fn terminal_speaks_kitty() -> bool {
    let term = std::env::var("TERM").unwrap_or_default();
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    env_set("KONSOLE_VERSION")
        || env_set("KITTY_WINDOW_ID")
        || term.contains("kitty")
        || term.contains("ghostty")
        || program == "ghostty"
}

impl Gfx {
    /// Must be called after entering the alternate screen and before reading events.
    pub fn init(preference: &str, big_text: bool, font: &str) -> Self {
        let query = || Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        let force = |t: ProtocolType| {
            let mut p = query();
            p.set_protocol_type(t);
            Mode::Widget(p)
        };
        let mode = match preference {
            "kitty" => Mode::Kitty,
            "sixel" => force(ProtocolType::Sixel),
            "iterm2" => force(ProtocolType::Iterm2),
            "halfblocks" => Mode::Widget(Picker::halfblocks()),
            _ if terminal_speaks_kitty() => Mode::Kitty,
            _ => Mode::Widget(query()),
        };
        let cell = match &mode {
            Mode::Kitty => cell_size().unwrap_or((10, 20)),
            Mode::Widget(p) => (p.font_size().width, p.font_size().height),
        };
        let pixels = match &mode {
            Mode::Kitty => true,
            Mode::Widget(p) => p.protocol_type() != ProtocolType::Halfblocks,
        };
        let big_text = big_text && pixels;
        let mut options = usvg::Options::default();
        if big_text {
            options.fontdb_mut().load_system_fonts();
        }
        Self { mode, cell, options, big_text, font: font.to_string(), drawn: None, cached: None }
    }

    /// An SVG drawing `chars` large, or `None` when big text is off or unsupported here.
    pub fn text_svg(&self, chars: &str) -> Option<Vec<u8>> {
        if !self.big_text {
            return None;
        }
        let n = chars.chars().count().max(1);
        let escaped = chars.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let font = self.font.replace('"', "");
        Some(
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} 1000"><text x="{x}" y="840" font-size="900" text-anchor="middle" font-family="{font}, Noto Sans CJK JP, Noto Sans JP, IPAGothic, sans-serif">{escaped}</text></svg>"#,
                w = n * 1000,
                x = n * 500,
            )
            .into_bytes(),
        )
    }

    pub fn is_kitty(&self) -> bool {
        matches!(self.mode, Mode::Kitty)
    }

    /// A cell area for an image `aspect` times as wide as it is tall, centred in `panel`.
    pub fn image_area(&self, panel: Rect, aspect: u32) -> Rect {
        let (cw, ch) = (self.cell.0.max(1) as u32, self.cell.1.max(1) as u32);
        let max_cols = panel.width.saturating_sub(2).max(1) as u32;
        let mut rows = panel.height.saturating_sub(2).max(1) as u32;
        // Shrink long words until they fit the panel width.
        while rows > 1 && (rows * ch * aspect).div_ceil(cw) > max_cols {
            rows -= 1;
        }
        let cols = (rows * ch * aspect).div_ceil(cw).clamp(1, max_cols) as u16;
        let rows = rows as u16;
        Rect {
            x: panel.x + panel.width.saturating_sub(cols) / 2,
            y: panel.y + panel.height.saturating_sub(rows) / 2,
            width: cols,
            height: rows,
        }
    }

    fn render(&self, svg: &[u8], area: Rect, fg: Rgb, bg: Rgb) -> Option<RgbaImage> {
        render_svg(
            svg,
            area.width as u32 * self.cell.0 as u32,
            area.height as u32 * self.cell.1 as u32,
            fg,
            bg,
            &self.options,
        )
    }

    /// Widget mode: the protocol to render with `ratatui_image::Image` inside the frame.
    pub fn widget(&mut self, slot: Slot, svg: &[u8], fg: Rgb, bg: Rgb) -> Option<&Protocol> {
        let Mode::Widget(picker) = &self.mode else { return None };
        if self.cached.as_ref().map(|(s, _)| *s) != Some(slot) {
            let img = self.render(svg, slot.1, fg, bg)?;
            let proto = picker
                .new_protocol(
                    DynamicImage::ImageRgba8(img),
                    Size::new(slot.1.width, slot.1.height),
                    Resize::Fit(None),
                )
                .ok()?;
            self.cached = Some((slot, proto));
        }
        self.cached.as_ref().map(|(_, p)| p)
    }

    /// Kitty mode: does the screen need to change to show `want`?
    pub fn kitty_stale(&self, want: Option<Slot>) -> bool {
        self.is_kitty() && self.drawn != want
    }

    /// Kitty mode: is an image on screen that has to be removed first?
    pub fn kitty_has_image(&self) -> bool {
        self.drawn.is_some()
    }

    pub fn kitty_clear(&mut self) -> io::Result<()> {
        self.drawn = None;
        let mut out = io::stdout().lock();
        out.write_all(b"\x1b_Ga=d,d=A,q=2\x1b\\")?;
        out.flush()
    }

    /// Kitty mode: place the image. Call after the frame has been drawn.
    pub fn kitty_place(&mut self, slot: Slot, svg: &[u8], fg: Rgb, bg: Rgb) -> io::Result<()> {
        self.drawn = Some(slot);
        let Some(img) = self.render(svg, slot.1, fg, bg) else { return Ok(()) };
        let mut png = Vec::new();
        if DynamicImage::ImageRgba8(img)
            .write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
            .is_err()
        {
            return Ok(());
        }
        let data = base64(&png);
        let area = slot.1;
        let mut out = io::stdout().lock();
        // Save cursor, move to the top-left cell of the image.
        write!(out, "\x1b7\x1b[{};{}H", area.y + 1, area.x + 1)?;
        let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let more = if i + 1 < chunks.len() { 1 } else { 0 };
            if i == 0 {
                write!(out, "\x1b_Ga=T,f=100,q=2,C=1,c={},r={},m={more};", area.width, area.height)?;
            } else {
                write!(out, "\x1b_Gm={more};")?;
            }
            out.write_all(chunk)?;
            out.write_all(b"\x1b\\")?;
        }
        out.write_all(b"\x1b8")?;
        out.flush()
    }

    /// The terminal was resized (or its font changed): forget what was drawn.
    pub fn invalidate(&mut self) -> io::Result<()> {
        self.cached = None;
        if self.is_kitty() {
            if let Some(cell) = cell_size() {
                self.cell = cell;
            }
            self.kitty_clear()?;
        }
        Ok(())
    }
}

/// Rasterise a WaniKani radical SVG centred in a `width`×`height` canvas, drawn in `fg` on `bg`.
pub fn render_svg(svg: &[u8], width: u32, height: u32, fg: Rgb, bg: Rgb, options: &usvg::Options) -> Option<RgbaImage> {
    if width == 0 || height == 0 {
        return None;
    }
    let tree = usvg::Tree::from_data(svg, options).ok()?;
    let size = tree.size();
    let scale = (width as f32 / size.width()).min(height as f32 / size.height());
    let dx = (width as f32 - size.width() * scale) / 2.0;
    let dy = (height as f32 - size.height() * scale) / 2.0;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // The artwork is black strokes on transparency: use its alpha as a mask for `fg` over `bg`.
    let mut out = RgbaImage::new(width, height);
    for (px, dst) in pixmap.pixels().iter().zip(out.pixels_mut()) {
        let a = px.alpha() as u32;
        let mix = |f: u8, b: u8| ((f as u32 * a + b as u32 * (255 - a)) / 255) as u8;
        dst.0 = [mix(fg[0], bg[0]), mix(fg[1], bg[1]), mix(fg[2], bg[2]), 255];
    }
    Some(out)
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn svg_renders_strokes_in_foreground_colour() {
        let item = crate::demo::items().into_iter().find(|i| i.subject.image_svg.is_some()).unwrap();
        let img = render_svg(item.subject.image_svg.as_deref().unwrap(), 200, 100, [255, 255, 255], [0, 170, 255], &usvg::Options::default()).unwrap();
        // Corner is background; the vertical stroke through the centre is foreground.
        assert_eq!(img.get_pixel(0, 0).0, [0, 170, 255, 255]);
        assert_eq!(img.get_pixel(100, 50).0, [255, 255, 255, 255]);
    }
}
