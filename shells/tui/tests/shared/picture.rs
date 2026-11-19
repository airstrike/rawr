//! A picture of a terminal's cells as a dark terminal draws them: each
//! cell's background filled, then its symbol in a monospace face, bold
//! cells in the bold face. A colour the shell leaves to the terminal
//! (`Reset`) and the terminal's sixteen named colours take a dark
//! terminal's palette. The pictures go to `target/screens`.

use std::path::{Path, PathBuf};

use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont as _};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

/// A regular face and its bold one, each a font file and its index in it:
/// macOS's Menlo, else the DejaVu Sans Mono most Linux systems carry.
const FACES: [[(&str, u32); 2]; 2] = [
    [
        ("/System/Library/Fonts/Menlo.ttc", 0),
        ("/System/Library/Fonts/Menlo.ttc", 1),
    ],
    [
        ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 0),
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
            0,
        ),
    ],
];

/// A cell's width in pixels; its height follows from the face.
const CELL_WIDTH: f32 = 16.0;
/// The space around the cells, as a terminal window's padding.
const MARGIN: u32 = 12;

const FOREGROUND: Rgb = [0xc0, 0xc5, 0xce];
const BACKGROUND: Rgb = [0x1e, 0x21, 0x28];

/// The named colours, in the order of their ANSI numbers.
const NAMED: [Rgb; 16] = [
    [0x00, 0x00, 0x00],
    [0xe0, 0x6c, 0x75],
    [0x98, 0xc3, 0x79],
    [0xe5, 0xc0, 0x7b],
    [0x61, 0xaf, 0xef],
    [0xc6, 0x78, 0xdd],
    [0x56, 0xb6, 0xc2],
    [0xab, 0xb2, 0xbf],
    [0x5c, 0x63, 0x70],
    [0xef, 0x82, 0x8b],
    [0xb5, 0xd8, 0x9b],
    [0xf0, 0xd1, 0x97],
    [0x8c, 0xc8, 0xf5],
    [0xd8, 0x9f, 0xe8],
    [0x7f, 0xd0, 0xda],
    [0xff, 0xff, 0xff],
];

type Rgb = [u8; 3];

/// Save `buffer` as `target/screens/{name}`, and return where it went.
pub fn save(buffer: &Buffer, name: &str) -> PathBuf {
    let screens = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .parent()
        .expect("the tests' scratch directory is inside the target directory")
        .join("screens");
    std::fs::create_dir_all(&screens).unwrap();
    let path = screens.join(name);
    let (width, height, pixels) = draw(buffer);
    let file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&pixels).unwrap();
    writer.finish().unwrap();
    path
}

/// The picture's width, height and RGB rows. Every background is filled
/// before any symbol is drawn, so a wide symbol isn't cut by its
/// neighbour's background.
fn draw(buffer: &Buffer) -> (u32, u32, Vec<u8>) {
    let [regular, bold] = faces();
    let scale = PxScale::from(
        CELL_WIDTH * regular.height_unscaled() / regular.h_advance_unscaled(regular.glyph_id('0')),
    );
    let scaled = regular.as_scaled(scale);
    // A row is as tall as the face, so box-drawing lines meet across rows
    // as a terminal draws them.
    let cell_height = scaled.height().ceil();
    let ascent = scaled.ascent();
    let columns = u32::from(buffer.area.width);
    let rows = u32::from(buffer.area.height);
    let mut canvas = Canvas {
        width: columns * CELL_WIDTH as u32 + 2 * MARGIN,
        height: rows * cell_height as u32 + 2 * MARGIN,
        pixels: Vec::new(),
    };
    canvas.pixels = BACKGROUND.repeat((canvas.width * canvas.height) as usize);
    let origin = |index: usize| {
        let index = index as u32;
        (
            (MARGIN + index % columns * CELL_WIDTH as u32) as f32,
            (MARGIN as f32) + (index / columns) as f32 * cell_height,
        )
    };
    for (index, cell) in buffer.content.iter().enumerate() {
        let (x, y) = origin(index);
        let (_, background) = colours(cell.fg, cell.bg, cell.modifier);
        canvas.fill(x, y, CELL_WIDTH, cell_height, background);
    }
    for (index, cell) in buffer.content.iter().enumerate() {
        let (x, y) = origin(index);
        let (foreground, _) = colours(cell.fg, cell.bg, cell.modifier);
        let face = if cell.modifier.contains(Modifier::BOLD) {
            &bold
        } else {
            &regular
        };
        for (offset, symbol) in cell.symbol().chars().enumerate() {
            if symbol.is_whitespace() {
                continue;
            }
            let glyph = face.glyph_id(symbol).with_scale_and_position(
                scale,
                ab_glyph::point(x + offset as f32 * CELL_WIDTH, y + ascent),
            );
            let Some(outline) = face.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                canvas.blend(
                    bounds.min.x as i64 + i64::from(gx),
                    bounds.min.y as i64 + i64::from(gy),
                    foreground,
                    coverage,
                );
            });
        }
        if cell.modifier.contains(Modifier::UNDERLINED) {
            canvas.fill(x, y + ascent + 2.0, CELL_WIDTH, 1.0, foreground);
        }
    }
    (canvas.width, canvas.height, canvas.pixels)
}

/// The regular and bold faces of the first family found.
fn faces() -> [FontVec; 2] {
    FACES
        .iter()
        .find_map(|family| {
            let [regular, bold] = family.map(|(file, index)| {
                std::fs::read(file)
                    .ok()
                    .and_then(|data| FontVec::try_from_vec_and_index(data, index).ok())
            });
            Some([regular?, bold?])
        })
        .unwrap_or_else(|| panic!("a monospace face is installed at one of {FACES:?}"))
}

/// A cell's foreground and background as the terminal shows them: reversed
/// cells swap them, and dim ones draw halfway to the background.
fn colours(fg: Color, bg: Color, modifier: Modifier) -> (Rgb, Rgb) {
    let (mut foreground, mut background) = (rgb(fg, FOREGROUND), rgb(bg, BACKGROUND));
    if modifier.contains(Modifier::REVERSED) {
        (foreground, background) = (background, foreground);
    }
    if modifier.contains(Modifier::DIM) {
        foreground = mix(foreground, background, 0.5);
    }
    (foreground, background)
}

/// `color` in RGB, with `default` for the terminal's own.
fn rgb(color: Color, default: Rgb) -> Rgb {
    match color {
        Color::Reset => default,
        Color::Rgb(red, green, blue) => [red, green, blue],
        Color::Indexed(index) => indexed(index),
        Color::Black => NAMED[0],
        Color::Red => NAMED[1],
        Color::Green => NAMED[2],
        Color::Yellow => NAMED[3],
        Color::Blue => NAMED[4],
        Color::Magenta => NAMED[5],
        Color::Cyan => NAMED[6],
        Color::Gray => NAMED[7],
        Color::DarkGray => NAMED[8],
        Color::LightRed => NAMED[9],
        Color::LightGreen => NAMED[10],
        Color::LightYellow => NAMED[11],
        Color::LightBlue => NAMED[12],
        Color::LightMagenta => NAMED[13],
        Color::LightCyan => NAMED[14],
        Color::White => NAMED[15],
    }
}

/// The 256-colour palette: the named sixteen, a 6×6×6 cube, then 24 greys.
fn indexed(index: u8) -> Rgb {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match index {
        0..=15 => NAMED[usize::from(index)],
        16..=231 => {
            let cube = usize::from(index - 16);
            [LEVELS[cube / 36], LEVELS[cube / 6 % 6], LEVELS[cube % 6]]
        }
        232..=255 => [8 + (index - 232) * 10; 3],
    }
}

/// `over` laid on `under` at `amount`, from 0 (all `under`) to 1.
fn mix(over: Rgb, under: Rgb, amount: f32) -> Rgb {
    let mut mixed = under;
    for (channel, (over, under)) in mixed.iter_mut().zip(over.iter().zip(under)) {
        *channel = (f32::from(*over) * amount + f32::from(under) * (1.0 - amount)).round() as u8;
    }
    mixed
}

/// RGB pixels, row by row.
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn fill(&mut self, x: f32, y: f32, width: f32, height: f32, colour: Rgb) {
        for row in y as i64..(y + height) as i64 {
            for column in x as i64..(x + width) as i64 {
                self.blend(column, row, colour, 1.0);
            }
        }
    }

    fn blend(&mut self, x: i64, y: i64, colour: Rgb, coverage: f32) {
        let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
            return;
        };
        if x >= self.width || y >= self.height {
            return;
        }
        let at = (y * self.width + x) as usize * 3;
        let under = [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]];
        self.pixels[at..at + 3].copy_from_slice(&mix(colour, under, coverage.min(1.0)));
    }
}
