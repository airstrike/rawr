//! Native `main`: prints its arguments and each line of stdin through the
//! marked module `words`.
mod words;

use std::io::BufRead as _;

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    println!("name {}", arguments[0]);
    for argument in &arguments[1..] {
        println!("arg {}", words::shout(argument));
    }
    for line in std::io::stdin().lock().lines() {
        println!("line {}", words::shout(&line.unwrap()));
    }
    println!("done");
}
