//! Native `main`: uses a direct proc-macro dependency's macro and calls a
//! marked module.
mod scale;

fn main() {
    let doubled = numerals::double!(21);
    println!("{doubled} {}", scale::label(doubled));
}
