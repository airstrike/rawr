enum State {
    Idle,
    Running { ticks: Vec<i64>, total: i64 },
}

struct Machine {
    state: State,
    log: Vec<String>,
}

impl Machine {
    fn step(&mut self, n: i64) {
        match &mut self.state {
            State::Idle => {
                self.log.push(String::from("start"));
                self.state = State::Running { ticks: Vec::new(), total: 0 };
            }
            State::Running { ticks, total } => {
                ticks.push(n);
                *total += n;
                self.log.push(format!("tick {n}"));
            }
        }
    }
}

fn main() {
    let mut m = Machine { state: State::Idle, log: Vec::new() };
    for n in 0..4 {
        m.step(n);
    }
    if let State::Running { ticks, total } = &m.state {
        println!("{ticks:?} {total}");
    }
    println!("{:?}", m.log);
}
