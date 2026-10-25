// rawr
//! The invaders' march: the formation steps sideways, drops a row at each
//! edge and speeds up as it thins, and its front rank shoots back.

use crate::game::{Bullet, Game, Invader, Status, FIELD_H, FIELD_W};

pub const INVADER_W: f32 = 36.0;
pub const INVADER_H: f32 = 24.0;

const ROWS: u8 = 4;
const COLS: u8 = 9;
/// From one invader's corner to the next.
const SPACE_X: f32 = 50.0;
const SPACE_Y: f32 = 36.0;
/// Where the formation starts.
const LEFT: f32 = 50.0;
const TOP: f32 = 60.0;
/// One step sideways, and the drop at an edge.
const STEP: f32 = 12.0;
const DROP: f32 = 14.0;
/// How close to an edge the formation comes before it turns.
const MARGIN: f32 = 10.0;
/// How fast the invaders' shots fall, in pixels per second.
const SHOT_SPEED: f32 = 220.0;
/// Larger is slower: it scales the time between steps and between shots.
const PACE: f32 = 1.0;

/// A point on the field.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// The formation as a game starts, every invader alive.
pub fn formation() -> Vec<Invader> {
    let mut invaders = Vec::new();
    for row in 0..ROWS {
        for col in 0..COLS {
            invaders.push(Invader {
                col,
                row,
                alive: true,
            });
        }
    }
    invaders
}

/// Where `invader` is now: the top-left corner of its box.
pub fn position(game: &Game, invader: &Invader) -> Point {
    Point {
        x: LEFT + invader.col as f32 * SPACE_X + game.offset_x,
        y: TOP + invader.row as f32 * SPACE_Y + game.offset_y,
    }
}

/// Move the invaders' shots and the formation on by `dt` seconds, and
/// let the front rank shoot. A formation with no one left is a win.
pub fn step(game: &mut Game, dt: f32) {
    for bullet in &mut game.invader_bullets {
        bullet.y += SHOT_SPEED * dt;
    }
    game.invader_bullets.retain(|bullet| bullet.y < FIELD_H);

    let alive = game.invaders.iter().filter(|invader| invader.alive).count();
    if alive == 0 {
        game.status = Status::Won;
        return;
    }
    // The tempo rises as the ranks thin.
    let total = (ROWS as usize * COLS as usize) as f32;
    let intensity = 1.0 - (alive as f32 / total) * 0.7;
    let interval = ((0.55 - intensity * 0.45) * PACE).max(0.06);

    game.step_accum += dt;
    if game.step_accum >= interval {
        game.step_accum = 0.0;
        game.offset_x += game.direction * STEP;
        let mut least = FIELD_W;
        let mut most: f32 = 0.0;
        for invader in game.invaders.iter().filter(|invader| invader.alive) {
            let at = position(game, invader);
            least = least.min(at.x);
            most = most.max(at.x + INVADER_W);
        }
        if least < MARGIN || most > FIELD_W - MARGIN {
            game.direction = -game.direction;
            game.offset_y += DROP;
        }
    }

    game.fire_accum += dt;
    let fire_interval = ((1.2 - intensity * 0.9) * PACE).max(0.18);
    if game.fire_accum >= fire_interval {
        game.fire_accum = 0.0;
        shoot(game);
    }
}

/// The lowest invader of a column picked at random shoots.
fn shoot(game: &mut Game) {
    let pick = random(game) as usize;
    let mut columns: Vec<u8> = Vec::new();
    for col in 0..COLS {
        if game
            .invaders
            .iter()
            .any(|invader| invader.alive && invader.col == col)
        {
            columns.push(col);
        }
    }
    if columns.is_empty() {
        return;
    }
    let col = columns[pick % columns.len()];
    let mut shooter: Option<Invader> = None;
    for invader in &game.invaders {
        if invader.alive
            && invader.col == col
            && shooter.is_none_or(|lowest| invader.row > lowest.row)
        {
            shooter = Some(*invader);
        }
    }
    if let Some(invader) = shooter {
        let at = position(game, &invader);
        game.invader_bullets.push(Bullet {
            x: at.x + INVADER_W / 2.0 - 1.5,
            y: at.y + INVADER_H,
        });
    }
}

/// The next number of the game's xorshift generator.
fn random(game: &mut Game) -> u32 {
    let mut x = game.rng_state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    game.rng_state = x;
    (x >> 11) as u32
}
