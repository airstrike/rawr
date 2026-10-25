// rawr
//! The game's state and its rules: the player, the shots and the order of
//! a frame. Under `rawr run` this file is interpreted, and a save takes
//! effect at the next frame, with the game as it was. `cargo build`
//! compiles it like any other module.

use crate::{collide, march};

/// The field's size, in framebuffer pixels.
pub const FIELD_W: f32 = 640.0;
pub const FIELD_H: f32 = 520.0;

pub const PLAYER_W: f32 = 39.0;
pub const PLAYER_H: f32 = 24.0;
/// The top of the player's cannon.
pub const PLAYER_LINE_Y: f32 = FIELD_H - 60.0;

pub const BULLET_W: f32 = 3.0;
pub const BULLET_H: f32 = 12.0;

/// How fast the cannon moves, in pixels per second.
const PLAYER_SPEED: f32 = 280.0;
/// How fast the player's shots climb, in pixels per second.
const BULLET_SPEED: f32 = 480.0;
/// The least time between two of the player's shots.
const FIRE_COOLDOWN_MS: u64 = 280;
/// The longest step a frame takes, so a stall doesn't jump the game.
const MAX_STEP: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Playing,
    Lost,
    Won,
}

/// One invader of the formation, by its place in it.
#[derive(Clone, Copy, Debug)]
pub struct Invader {
    pub col: u8,
    pub row: u8,
    pub alive: bool,
}

/// A shot, by its top-left corner.
#[derive(Clone, Copy, Debug)]
pub struct Bullet {
    pub x: f32,
    pub y: f32,
}

/// The keys held this frame.
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub left: bool,
    pub right: bool,
    pub fire: bool,
    pub restart: bool,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub player_x: f32,
    pub bullets: Vec<Bullet>,
    pub invader_bullets: Vec<Bullet>,
    pub invaders: Vec<Invader>,
    /// How far the formation has marched from where it started.
    pub offset_x: f32,
    pub offset_y: f32,
    /// 1 marching right, -1 marching left.
    pub direction: f32,
    pub step_accum: f32,
    pub fire_accum: f32,
    pub since_fire_ms: u64,
    pub score: u32,
    pub status: Status,
    pub rng_state: u64,
}

impl Game {
    /// A new game, the formation at its start and the cannon in the middle.
    pub fn new() -> Game {
        Game {
            player_x: FIELD_W / 2.0 - PLAYER_W / 2.0,
            bullets: Vec::new(),
            invader_bullets: Vec::new(),
            invaders: march::formation(),
            offset_x: 0.0,
            offset_y: 0.0,
            direction: 1.0,
            step_accum: 0.0,
            fire_accum: 0.0,
            since_fire_ms: 9_999,
            score: 0,
            status: Status::Playing,
            rng_state: 0xC0FF_EEDE_ADBE_EF00,
        }
    }

    /// Move the game on by `elapsed` seconds with the keys in `input`.
    pub fn update(&mut self, input: &Input, elapsed: f32) {
        let dt = elapsed.clamp(0.0, MAX_STEP);
        self.since_fire_ms = self.since_fire_ms.saturating_add((dt * 1000.0) as u64);
        if self.status != Status::Playing {
            if input.restart {
                *self = Game::new();
            }
            return;
        }

        // The cannon.
        let mut speed = 0.0;
        if input.left {
            speed -= PLAYER_SPEED;
        }
        if input.right {
            speed += PLAYER_SPEED;
        }
        self.player_x = (self.player_x + speed * dt).clamp(0.0, FIELD_W - PLAYER_W);
        if input.fire {
            self.fire();
        }

        // The player's shots climb and leave the field at its top.
        for bullet in &mut self.bullets {
            bullet.y -= BULLET_SPEED * dt;
        }
        self.bullets.retain(|bullet| bullet.y + BULLET_H > 0.0);

        march::step(self, dt);
        if self.status == Status::Playing {
            collide::resolve(self);
        }
    }

    fn fire(&mut self) {
        if self.since_fire_ms < FIRE_COOLDOWN_MS {
            return;
        }
        self.bullets.push(Bullet {
            x: self.player_x + PLAYER_W / 2.0 - BULLET_W / 2.0,
            y: PLAYER_LINE_Y - BULLET_H,
        });
        self.since_fire_ms = 0;
    }
}

/// The window's title.
pub fn title(game: &Game) -> String {
    match game.status {
        Status::Playing => format!("Invaders · {}", game.score),
        Status::Lost => format!("Invaders · game over · {}", game.score),
        Status::Won => format!("Invaders · victory · {}", game.score),
    }
}
