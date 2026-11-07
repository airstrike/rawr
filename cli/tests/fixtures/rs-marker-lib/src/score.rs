// rawr
use crate::units::BONUS;

pub struct Game {
    pub points: i64,
    pub rounds: i64,
}

impl Game {
    pub fn new() -> Game {
        Game {
            points: 0,
            rounds: 0,
        }
    }

    pub fn play(&mut self, hits: i64) {
        self.rounds += 1;
        self.points += hits * 10 + BONUS;
    }
}

pub fn label(game: &Game) -> String {
    format!("{} points in {} rounds", game.points, game.rounds)
}
