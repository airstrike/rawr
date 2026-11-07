// rawr
const SCALE: i64 = 1;

pub struct Counter {
    pub ticks: i64,
}

impl Counter {
    pub fn new() -> Counter {
        Counter { ticks: 0 }
    }

    pub fn step(&mut self) {
        self.ticks += 1;
    }
}

pub fn label(counter: &Counter) -> String {
    format!("value {}", counter.ticks * SCALE)
}
