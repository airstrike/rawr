const GRAVITY: f64 = 9.8;
const BOUNCE: f64 = 0.8;

struct Ball {
    height: f64,
    up: f64,
}

impl Ball {
    fn advance(&mut self, step: f64) {
        self.up -= GRAVITY * step;
        self.height += self.up * step;
        if self.height < 0.0 {
            self.height = 0.0;
            self.up = -self.up * BOUNCE;
        }
    }
}

fn main() {
    let mut ball = Ball {
        height: 8.0,
        up: 0.0,
    };
    for tick in 1..=12 {
        ball.advance(0.25);
        let column = (ball.height * 2.0).round() as usize;
        println!("{tick:>2} |{}o", " ".repeat(column));
    }
}
