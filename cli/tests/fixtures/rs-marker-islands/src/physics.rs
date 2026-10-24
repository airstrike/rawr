// rawr
const GRAVITY: f64 = 9.8;

pub struct Ball {
    pub height: f64,
    pub speed: f64,
}

impl Ball {
    pub fn new(height: f64) -> Ball {
        Ball { height, speed: 0.0 }
    }

    pub fn advance(&mut self, step: f64) {
        self.speed -= GRAVITY * step;
        self.height = (self.height + self.speed * step).max(0.0);
    }
}

pub fn describe(ball: &Ball) -> String {
    format!("height {:.2}", ball.height)
}
