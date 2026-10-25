// rawr
//! What hits what: the player's shots and the invaders, the invaders'
//! shots and the cannon, and the formation reaching the cannon's line.

use crate::game::{Bullet, Game, Status, BULLET_H, BULLET_W, PLAYER_H, PLAYER_LINE_Y, PLAYER_W};
use crate::march::{self, Point, INVADER_H, INVADER_W};

/// Points for each invader shot down.
const POINTS: u32 = 10;

/// A box on the field, by its top-left corner and its size.
struct Area {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Area {
    fn overlaps(&self, other: &Area) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }
}

fn shot(bullet: &Bullet) -> Area {
    Area {
        x: bullet.x,
        y: bullet.y,
        width: BULLET_W,
        height: BULLET_H,
    }
}

fn invader(at: Point) -> Area {
    Area {
        x: at.x,
        y: at.y,
        width: INVADER_W,
        height: INVADER_H,
    }
}

/// Settle this frame's hits: score each invader shot down, and end the
/// game when the cannon is hit or the formation reaches it.
pub fn resolve(game: &mut Game) {
    // The player's shots against the invaders. A shot takes down one
    // invader and is spent.
    let mut index = 0;
    while index < game.bullets.len() {
        let bullet = shot(&game.bullets[index]);
        let mut hit = false;
        for which in 0..game.invaders.len() {
            let target = game.invaders[which];
            if target.alive && bullet.overlaps(&invader(march::position(game, &target))) {
                game.invaders[which].alive = false;
                game.score += POINTS;
                hit = true;
                break;
            }
        }
        if hit {
            game.bullets.swap_remove(index);
        } else {
            index += 1;
        }
    }

    // The invaders' shots against the cannon.
    let cannon = Area {
        x: game.player_x,
        y: PLAYER_LINE_Y,
        width: PLAYER_W,
        height: PLAYER_H,
    };
    if game
        .invader_bullets
        .iter()
        .any(|bullet| shot(bullet).overlaps(&cannon))
    {
        game.status = Status::Lost;
        return;
    }

    // The formation reaches the cannon's line.
    for target in &game.invaders {
        if target.alive && march::position(game, target).y + INVADER_H >= PLAYER_LINE_Y {
            game.status = Status::Lost;
            return;
        }
    }
}
