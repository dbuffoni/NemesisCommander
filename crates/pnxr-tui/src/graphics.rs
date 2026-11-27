//! Pictures in the terminal (SC-38): the best way the terminal has to show a
//! picture. Terminals with graphics (kitty, iTerm2, sixel) get the real
//! pixels; the others get block characters, two pixels per cell, or
//! sextants, six, with the picture scaled by a good filter.
//!
//! The terminal is recognised from the environment, not asked: a question to
//! the terminal would have to be answered while the keys are read. The
//! setting "Pictures in the terminal" (or `PNXR_IMAGES`) forces a way when the
//! terminal has it but is not recognised.

use std::cell::RefCell;

use image::{DynamicImage, RgbaImage, imageops::FilterType};
use pnxr_config::TerminalImages;
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui::style::Color;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::{FontSize, Image, Resize};

/// How a picture is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Graphics of the terminal.
    Graphics(ProtocolType),
    /// Six pixels per cell.
    Sextants,
    /// Two pixels per cell.
    Halfblocks,
}

/// The way to draw pictures for `setting`, given the environment `env`.
pub fn detect(setting: TerminalImages, env: &dyn Fn(&str) -> Option<String>) -> Mode {
    let setting = match setting {
        TerminalImages::Auto => match env("PNXR_IMAGES").as_deref().map(str::to_lowercase) {
            Some(v) if v == "kitty" => TerminalImages::Kitty,
            Some(v) if v == "iterm2" => TerminalImages::Iterm2,
            Some(v) if v == "sixel" => TerminalImages::Sixel,
            Some(v) if v == "blocks" => TerminalImages::Blocks,
            Some(v) if v == "sextants" => TerminalImages::Sextants,
            _ => TerminalImages::Auto,
        },
        forced => forced,
    };
    match setting {
        TerminalImages::Kitty => Mode::Graphics(ProtocolType::Kitty),
        TerminalImages::Iterm2 => Mode::Graphics(ProtocolType::Iterm2),
        TerminalImages::Sixel => Mode::Graphics(ProtocolType::Sixel),
        TerminalImages::Blocks => Mode::Halfblocks,
        TerminalImages::Sextants => Mode::Sextants,
        TerminalImages::Auto => auto(env),
    }
}

fn auto(env: &dyn Fn(&str) -> Option<String>) -> Mode {
    let has = |name: &str| env(name).is_some_and(|v| !v.is_empty());
    let is = |name: &str, value: &str| env(name).is_some_and(|v| v == value);
    let term = env("TERM").unwrap_or_default();
    // Graphics do not pass through a multiplexer reliably.
    if has("TMUX") || has("STY") || term.starts_with("screen") || term.starts_with("tmux") {
        return Mode::Halfblocks;
    }
    if has("KITTY_WINDOW_ID") || term == "xterm-kitty" || is("TERM_PROGRAM", "ghostty") {
        return Mode::Graphics(ProtocolType::Kitty);
    }
    if is("TERM_PROGRAM", "iTerm.app")
        || is("LC_TERMINAL", "iTerm2")
        || is("TERM_PROGRAM", "WezTerm")
    {
        return Mode::Graphics(ProtocolType::Iterm2);
    }
    if term.starts_with("foot") || term.contains("mlterm") || is("TERM_PROGRAM", "contour") {
        return Mode::Graphics(ProtocolType::Sixel);
    }
    Mode::Halfblocks
}

/// The size of a cell in pixels, when the terminal tells it.
fn font_size() -> FontSize {
    match crossterm::terminal::window_size() {
        Ok(size) if size.width > 0 && size.height > 0 && size.columns > 0 && size.rows > 0 => {
            FontSize::new(size.width / size.columns, size.height / size.rows)
        }
        _ => FontSize::new(10, 20),
    }
}

/// What was made for the last picture drawn: drawing is done again on every
/// key, encoding the picture is not.
struct Cached {
    key: (usize, (u32, u32), u64, Size, ProtocolType, (u16, u16)),
    protocol: Protocol,
}

thread_local! {
    static SETTING: RefCell<TerminalImages> = const { RefCell::new(TerminalImages::Auto) };
    static CACHE: RefCell<Option<Cached>> = const { RefCell::new(None) };
}

/// Tells the drawing code which setting is on (once per frame).
pub fn use_setting(setting: TerminalImages) {
    SETTING.with(|s| *s.borrow_mut() = setting);
}

/// The way pictures are drawn now.
pub fn mode() -> Mode {
    detect(SETTING.with(|s| *s.borrow()), &|name| std::env::var(name).ok())
}

/// A few pixels of the picture, to tell two pictures apart without reading
/// all of them.
fn fingerprint(picture: &RgbaImage) -> u64 {
    let (w, h) = picture.dimensions();
    let mut hash: u64 = 0xcbf29ce484222325;
    for i in 0..64u32 {
        let (x, y) = ((i * 7919) % w.max(1), (i * 104_729) % h.max(1));
        for b in picture.get_pixel(x, y).0 {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
        }
    }
    hash
}

/// Draws `picture` centred in `area`. `true` when it was drawn with the
/// terminal's graphics; `false` when the caller should draw it with
/// characters (also when the graphics failed).
pub fn draw_graphics(frame: &mut Frame, picture: &RgbaImage, area: Rect) -> bool {
    let Mode::Graphics(protocol_type) = mode() else { return false };
    if area.width == 0 || area.height == 0 {
        return false;
    }
    let font = font_size();
    let key = (
        picture.as_ptr() as usize,
        picture.dimensions(),
        fingerprint(picture),
        Size::new(area.width, area.height),
        protocol_type,
        (font.width, font.height),
    );
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref().is_none_or(|c| c.key != key) {
            #[allow(deprecated)]
            let mut picker = Picker::from_fontsize(font);
            picker.set_protocol_type(protocol_type);
            let made = picker.new_protocol(
                DynamicImage::ImageRgba8(picture.clone()),
                Size::new(area.width, area.height),
                Resize::Fit(Some(FilterType::Lanczos3)),
            );
            match made {
                Ok(protocol) => *cache = Some(Cached { key, protocol }),
                Err(_) => {
                    *cache = None;
                    return false;
                }
            }
        }
        let Some(cached) = cache.as_ref() else { return false };
        let size = cached.protocol.size();
        let width = size.width.min(area.width);
        let height = size.height.min(area.height);
        let target = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Image::new(&cached.protocol), target);
        true
    })
}

// ------------------------------------------------------------ characters

/// A cell of block characters: the character and its foreground and
/// background colours (`None`: the terminal's own).
type BlockCell = (char, Option<[u8; 3]>, Option<[u8; 3]>);

/// The cells that show `picture` in at most `columns` × `rows` cells with
/// block characters: `(character, foreground, background)` per cell, `None`
/// colours for transparent pixels. `sextants` draws six pixels per cell,
/// else two.
pub fn block_cells(
    picture: &RgbaImage,
    columns: u16,
    rows: u16,
    sextants: bool,
) -> Vec<Vec<BlockCell>> {
    if columns == 0 || rows == 0 || picture.width() == 0 || picture.height() == 0 {
        return Vec::new();
    }
    // A cell is (1 × 2) or (2 × 3) pixels; a cell is about twice as high as wide.
    let (per_col, per_row) = if sextants { (2.0, 3.0) } else { (1.0, 2.0) };
    let max_w = f64::from(columns) * per_col;
    let max_h = f64::from(rows) * per_row;
    let (w, h) = (f64::from(picture.width()), f64::from(picture.height()));
    // Only shrunk, never enlarged: a small picture is not smeared.
    let scale = (max_w / w).min(max_h / h).min(1.0);
    let out_w = ((w * scale).round() as u32).max(1);
    let out_h = ((h * scale).round() as u32).max(1);
    let small = if (out_w, out_h) == picture.dimensions() {
        picture.clone()
    } else {
        // Lanczos keeps the edges sharp, which a few pixels of text need.
        image::imageops::resize(picture, out_w, out_h, FilterType::Lanczos3)
    };
    let px = |x: u32, y: u32| -> Option<[u8; 3]> {
        if x >= out_w || y >= out_h {
            return None;
        }
        let [r, g, b, a] = small.get_pixel(x, y).0;
        // Transparent pixels are left to the terminal's background.
        (a >= 128).then_some([r, g, b])
    };
    let (cw, ch) = (per_col as u32, per_row as u32);
    (0..out_h.div_ceil(ch))
        .map(|row| {
            (0..out_w.div_ceil(cw))
                .map(|col| {
                    let pixels: Vec<Option<[u8; 3]>> = (0..ch)
                        .flat_map(|dy| (0..cw).map(move |dx| (dx, dy)))
                        .map(|(dx, dy)| px(col * cw + dx, row * ch + dy))
                        .collect();
                    if sextants { sextant_cell(&pixels) } else { half_cell(&pixels) }
                })
                .collect()
        })
        .collect()
}

fn half_cell(p: &[Option<[u8; 3]>]) -> BlockCell {
    match (p[0], p[1]) {
        (None, None) => (' ', None, None),
        (Some(up), None) => ('▀', Some(up), None),
        (None, Some(down)) => ('▄', Some(down), None),
        (Some(up), Some(down)) => ('▀', Some(up), Some(down)),
    }
}

/// One cell of 2 × 3 pixels (left to right, top to bottom): the pixels are
/// split into a light and a dark group around the mean, and the character
/// that has the light ones set is drawn in their colour on the dark one.
fn sextant_cell(p: &[Option<[u8; 3]>]) -> BlockCell {
    let drawn: Vec<[u8; 3]> = p.iter().flatten().copied().collect();
    if drawn.is_empty() {
        return (' ', None, None);
    }
    let luma = |c: [u8; 3]| u32::from(c[0]) * 3 + u32::from(c[1]) * 6 + u32::from(c[2]);
    let mean = drawn.iter().map(|c| luma(*c)).sum::<u32>() / drawn.len() as u32;
    let average = |group: &[[u8; 3]]| -> Option<[u8; 3]> {
        (!group.is_empty()).then(|| {
            [0, 1, 2].map(|i| {
                (group.iter().map(|c| u32::from(c[i])).sum::<u32>() / group.len() as u32) as u8
            })
        })
    };
    let light: Vec<[u8; 3]> = drawn.iter().copied().filter(|c| luma(*c) >= mean).collect();
    let dark: Vec<[u8; 3]> = drawn.iter().copied().filter(|c| luma(*c) < mean).collect();
    let (fg, bg) = (average(&light), average(&dark));
    let mut bits = 0u8;
    for (i, pixel) in p.iter().enumerate() {
        if pixel.is_some_and(|c| luma(c) >= mean) {
            bits |= 1 << i;
        }
    }
    // A hole (transparent pixel) stays background: the background colour is
    // the dark group's, and a cell without dark pixels has none.
    (sextant_char(bits), fg, bg)
}

/// The character for the sextants set in `bits` (bit 0 top left, 1 top
/// right, 2 middle left... 5 bottom right). The four patterns that have a
/// character of their own elsewhere are not in the sextant block.
pub fn sextant_char(bits: u8) -> char {
    match bits {
        0 => ' ',
        21 => '▌',
        42 => '▐',
        63 => '█',
        n => {
            let index = u32::from(n) - 1 - u32::from(n > 21) - u32::from(n > 42);
            char::from_u32(0x1FB00 + index).unwrap_or('?')
        }
    }
}

/// Draws the cells of [`block_cells`] centred in `area`.
pub fn draw_blocks(frame: &mut Frame, picture: &RgbaImage, area: Rect, sextants: bool) {
    let cells = block_cells(picture, area.width, area.height, sextants);
    let height = (cells.len() as u16).min(area.height);
    let width = cells.first().map_or(0, |row| row.len() as u16).min(area.width);
    let left = area.x + (area.width - width) / 2;
    let top = area.y + (area.height - height) / 2;
    let buffer = frame.buffer_mut();
    let rgb = |c: [u8; 3]| Color::Rgb(c[0], c[1], c[2]);
    for (y, row) in cells.iter().enumerate().take(usize::from(height)) {
        for (x, (ch, fg, bg)) in row.iter().enumerate().take(usize::from(width)) {
            let cell = &mut buffer[(left + x as u16, top + y as u16)];
            cell.set_char(*ch);
            cell.set_fg(fg.map_or(Color::Reset, rgb));
            cell.set_bg(bg.map_or(Color::Reset, rgb));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
    }

    #[test]
    fn terminals_with_graphics_are_recognised_and_multiplexers_are_not_trusted() {
        let auto = TerminalImages::Auto;
        assert_eq!(
            detect(auto, &env(&[("TERM", "xterm-kitty")])),
            Mode::Graphics(ProtocolType::Kitty)
        );
        assert_eq!(
            detect(auto, &env(&[("TERM_PROGRAM", "iTerm.app")])),
            Mode::Graphics(ProtocolType::Iterm2)
        );
        assert_eq!(detect(auto, &env(&[("TERM", "foot")])), Mode::Graphics(ProtocolType::Sixel));
        assert_eq!(detect(auto, &env(&[("TERM", "xterm-256color")])), Mode::Halfblocks);
        // Inside tmux even kitty gets characters.
        assert_eq!(
            detect(auto, &env(&[("TERM", "xterm-kitty"), ("TMUX", "/tmp/tmux-1/default,1,0")])),
            Mode::Halfblocks
        );
    }

    #[test]
    fn the_setting_and_the_variable_force_a_way() {
        let plain = env(&[("TERM", "xterm-256color")]);
        assert_eq!(detect(TerminalImages::Sixel, &plain), Mode::Graphics(ProtocolType::Sixel));
        assert_eq!(detect(TerminalImages::Sextants, &plain), Mode::Sextants);
        let forced = env(&[("TERM", "xterm-256color"), ("PNXR_IMAGES", "Kitty")]);
        assert_eq!(detect(TerminalImages::Auto, &forced), Mode::Graphics(ProtocolType::Kitty));
        // The setting wins over the variable.
        assert_eq!(detect(TerminalImages::Blocks, &forced), Mode::Halfblocks);
    }

    #[test]
    fn sextant_characters_follow_unicode() {
        assert_eq!(sextant_char(0), ' ');
        assert_eq!(sextant_char(63), '█');
        assert_eq!(sextant_char(21), '▌');
        assert_eq!(sextant_char(42), '▐');
        // First and last of the block, and the one after the left half.
        assert_eq!(sextant_char(1), '\u{1FB00}');
        assert_eq!(sextant_char(62), '\u{1FB3B}');
        assert_eq!(sextant_char(22), '\u{1FB14}');
    }

    #[test]
    fn a_picture_is_cut_into_cells_and_only_shrunk() {
        let mut picture = RgbaImage::new(4, 6);
        for (x, y, pixel) in picture.enumerate_pixels_mut() {
            *pixel =
                if x < 2 { image::Rgba([255, 255, 255, 255]) } else { image::Rgba([0, 0, 0, 255]) };
            let _ = y;
        }
        // Six pixels per cell: 4 × 6 pixels are 2 × 2 cells, half light, half dark.
        let cells = block_cells(&picture, 10, 10, true);
        assert_eq!((cells.len(), cells[0].len()), (2, 2));
        // A cell of one colour is a full block in that colour.
        assert_eq!(cells[0][0], ('█', Some([255, 255, 255]), None), "a light cell");
        assert_eq!(cells[0][1], ('█', Some([0, 0, 0]), None), "a dark cell");
        // Two pixels per cell: 4 wide, 3 rows.
        let halves = block_cells(&picture, 10, 10, false);
        assert_eq!((halves.len(), halves[0].len()), (3, 4));
        // Too big for the area: shrunk to fit.
        let small = block_cells(&picture, 2, 1, false);
        assert!(small.len() <= 1 && small[0].len() <= 2);
        assert!(block_cells(&picture, 0, 4, false).is_empty());
    }

    #[test]
    fn transparent_pixels_stay_the_terminals_own_background() {
        let picture = RgbaImage::new(2, 2);
        let cells = block_cells(&picture, 4, 4, false);
        assert!(
            cells.iter().flatten().all(|(c, fg, bg)| *c == ' ' && fg.is_none() && bg.is_none())
        );
    }
}
