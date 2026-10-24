// rawr
// `rawr run` interprets this file, and `cargo build` compiles it as
// ordinary Rust.

struct Greeting {
    name: String,
    times: i64,
}

impl Greeting {
    fn line(&self, count: i64) -> String {
        format!("{count}. Hello, {}!", self.name)
    }
}

fn main() {
    let greeting = Greeting {
        name: String::from("rawr"),
        times: 3,
    };
    for count in 1..=greeting.times {
        println!("{}", greeting.line(count));
    }
}
