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
    let mut heights = Vec::new();
    for _ in 0..13 {
        heights.push(ball.height.round() as i64);
        ball.advance(0.25);
    }
    for row in (0..=8).rev() {
        let cells: Vec<&str> = heights
            .iter()
            .map(|&height| if height == row { "()" } else { "  " })
            .collect();
        println!("{}", cells.join(" ").trim_end());
    }
}
