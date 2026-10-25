// rawr
//! What a frame shows, as plain data: the engine paints the items in
//! order, so colours, sprite shapes and the text on screen are all here.

use crate::game::{Game, Status, BULLET_H, BULLET_W, FIELD_H, FIELD_W, PLAYER_LINE_Y, PLAYER_W};
use crate::march::{self, INVADER_H, INVADER_W};

/// The size of one sprite cell, in framebuffer pixels.
const PIXEL: i32 = 3;

const BACKGROUND: u32 = 0x1e1f29;
const STARS: u32 = 0xf8f8f2;
const CANNON: u32 = 0x50fa7b;
const CANNON_HIT: u32 = 0x6272a4;
const SHOT: u32 = 0xf8f8f2;
const INVADER_SHOT: u32 = 0xff5555;
const TEXT: u32 = 0xf8f8f2;

/// A frame for the engine to paint: its size, the colour under
/// everything, and what goes on top, in order.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub background: u32,
    pub items: Vec<Item>,
}

/// One thing to paint. A sprite's rows are lines of cells, each `scale`
/// pixels square: `.` is clear and `#` is lit.
#[derive(Clone, Debug)]
pub enum Item {
    Rect {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: u32,
    },
    Sprite {
        x: i32,
        y: i32,
        scale: i32,
        color: u32,
        rows: Vec<String>,
    },
}

/// The frame that shows `game`.
pub fn frame(game: &Game) -> Frame {
    let mut items = Vec::new();
    stars(&mut items);

    // The formation. Legs move with each step sideways.
    let step = (game.offset_x as i32).rem_euclid(24) < 12;
    for invader in &game.invaders {
        if !invader.alive {
            continue;
        }
        let at = march::position(game, invader);
        let rows = invader_shape(invader.row, step);
        let x = at.x as i32 + (INVADER_W as i32 - rows[0].len() as i32 * PIXEL) / 2;
        let y = at.y as i32 + (INVADER_H as i32 - rows.len() as i32 * PIXEL) / 2;
        items.push(Item::Sprite {
            x,
            y,
            scale: PIXEL,
            color: row_color(invader.row),
            rows,
        });
    }

    // The cannon.
    let color = if game.status == Status::Lost {
        CANNON_HIT
    } else {
        CANNON
    };
    let cannon = shape(&[
        "......#......",
        ".....###.....",
        ".....###.....",
        ".###########.",
        "#############",
        "#############",
        "#############",
        "#############",
    ]);
    items.push(Item::Sprite {
        x: game.player_x as i32 + (PLAYER_W as i32 - 13 * PIXEL) / 2,
        y: PLAYER_LINE_Y as i32,
        scale: PIXEL,
        color,
        rows: cannon,
    });

    // The shots.
    for bullet in &game.bullets {
        items.push(rect(bullet.x, bullet.y, SHOT));
    }
    for bullet in &game.invader_bullets {
        items.push(rect(bullet.x, bullet.y, INVADER_SHOT));
    }

    // The ground and the score.
    items.push(Item::Rect {
        x: 0,
        y: FIELD_H as i32 - 24,
        width: FIELD_W as i32,
        height: 2,
        color: CANNON,
    });
    text(
        &mut items,
        &format!("SCORE {:04}", game.score),
        16,
        16,
        TEXT,
    );
    match game.status {
        Status::Playing => {}
        Status::Lost => centred(&mut items, "GAME OVER - PRESS R", INVADER_SHOT),
        Status::Won => centred(&mut items, "YOU WIN - PRESS R", CANNON),
    }

    Frame {
        width: FIELD_W as u32,
        height: FIELD_H as u32,
        background: BACKGROUND,
        items,
    }
}

fn rect(x: f32, y: f32, color: u32) -> Item {
    Item::Rect {
        x: x as i32,
        y: y as i32,
        width: BULLET_W as i32,
        height: BULLET_H as i32,
        color,
    }
}

/// The colour of an invader in formation row `row`.
fn row_color(row: u8) -> u32 {
    match row {
        0 => 0xff79c6,
        1 => 0x8be9fd,
        2 => 0xf1fa8c,
        _ => 0xbd93f9,
    }
}

/// The sprite of an invader in formation row `row`, in one of its two
/// steps.
fn invader_shape(row: u8, step: bool) -> Vec<String> {
    match (row, step) {
        (0, false) => shape(&[
            "...##...", "..####..", ".######.", "##.##.##", "########", "..#..#..", ".#.##.#.",
            "#.#..#.#",
        ]),
        (0, true) => shape(&[
            "...##...", "..####..", ".######.", "##.##.##", "########", ".#.##.#.", "#......#",
            ".#....#.",
        ]),
        (1 | 2, false) => shape(&[
            "..#.....#..",
            "...#...#...",
            "..#######..",
            ".##.###.##.",
            "###########",
            "#.#######.#",
            "#.#.....#.#",
            "...##.##...",
        ]),
        (1 | 2, true) => shape(&[
            "..#.....#..",
            "#..#...#..#",
            "#.#######.#",
            "###.###.###",
            "###########",
            ".#########.",
            "..#.....#..",
            ".#.......#.",
        ]),
        (_, false) => shape(&[
            "....####....",
            ".##########.",
            "############",
            "###..##..###",
            "############",
            "...##..##...",
            "..##.##.##..",
            "##........##",
        ]),
        (_, true) => shape(&[
            "....####....",
            ".##########.",
            "############",
            "###..##..###",
            "############",
            "..###..###..",
            ".##..##..##.",
            "..##....##..",
        ]),
    }
}

fn shape(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| row.to_string()).collect()
}

/// A still field of stars, the same each frame: each star's place and
/// brightness come from a hash of its number.
fn stars(items: &mut Vec<Item>) {
    for number in 0..160_u32 {
        let mut hash = number.wrapping_mul(0x9E37_79B1);
        hash ^= hash >> 15;
        hash = hash.wrapping_mul(0x85EB_CA6B);
        hash ^= hash >> 13;
        let x = (hash & 0xFFFF) as f32 / 65535.0 * FIELD_W;
        let y = ((hash >> 16) & 0xFFFF) as f32 / 65535.0 * FIELD_H;
        let brightness = (hash.wrapping_mul(0x9E37_79B1) >> 24) as f32 / 255.0;
        let size = if brightness > 0.85 { 2 } else { 1 };
        items.push(Item::Rect {
            x: x as i32,
            y: y as i32,
            width: size,
            height: size,
            color: mix(BACKGROUND, STARS, 0.15 + brightness * 0.55),
        });
    }
}

/// `from` moved toward `to` by `amount`, channel by channel.
fn mix(from: u32, to: u32, amount: f32) -> u32 {
    let mut color = 0;
    for shift in [16, 8, 0] {
        let a = ((from >> shift) & 0xFF) as f32;
        let b = ((to >> shift) & 0xFF) as f32;
        let channel = (a + (b - a) * amount).round() as u32;
        color |= channel.min(255) << shift;
    }
    color
}

/// `line` centred across the field, below the formation's start.
fn centred(items: &mut Vec<Item>, line: &str, color: u32) {
    let width = line.chars().count() as i32 * 4 * PIXEL;
    text(
        items,
        line,
        (FIELD_W as i32 - width) / 2,
        FIELD_H as i32 / 2,
        color,
    );
}

/// `line` in the game's 3 by 5 font, from (`x`, `y`).
fn text(items: &mut Vec<Item>, line: &str, x: i32, y: i32, color: u32) {
    let mut left = x;
    for letter in line.chars() {
        let rows: Vec<String> = glyph(letter)
            .split('/')
            .map(|row| row.to_string())
            .collect();
        items.push(Item::Sprite {
            x: left,
            y,
            scale: PIXEL,
            color,
            rows,
        });
        left += 4 * PIXEL;
    }
}

fn glyph(letter: char) -> &'static str {
    match letter {
        '0' | 'O' => "###/#.#/#.#/#.#/###",
        '1' => ".#./##./.#./.#./###",
        '2' => "###/..#/###/#../###",
        '3' => "###/..#/###/..#/###",
        '4' => "#.#/#.#/###/..#/..#",
        '5' | 'S' => "###/#../###/..#/###",
        '6' => "###/#../###/#.#/###",
        '7' => "###/..#/..#/..#/..#",
        '8' => "###/#.#/###/#.#/###",
        '9' => "###/#.#/###/..#/###",
        'A' => "###/#.#/###/#.#/#.#",
        'C' => "###/#../#../#../###",
        'E' => "###/#../###/#../###",
        'G' => "###/#../#.#/#.#/###",
        'I' => "###/.#./.#./.#./###",
        'M' => "#.#/###/###/#.#/#.#",
        'N' => "##./#.#/#.#/#.#/#.#",
        'P' => "###/#.#/###/#../#..",
        'R' => "##./#.#/##./#.#/#.#",
        'U' => "#.#/#.#/#.#/#.#/###",
        'V' => "#.#/#.#/#.#/#.#/.#.",
        'W' => "#.#/#.#/###/###/#.#",
        'Y' => "#.#/#.#/.#./.#./.#.",
        '-' => ".../.../###/.../...",
        _ => ".../.../.../.../...",
    }
}
