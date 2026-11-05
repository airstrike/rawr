// rawr
use natives::looks::{self, Look, Tone};

fn bracketed(look: &Look) -> String {
    format!("<{look}>")
}

fn main() {
    println!("{:?}", looks::labels(&Look::ALL, Look::to_string));
    println!("{:?}", looks::labels(&Look::ALL, bracketed));
    println!("{:?}", looks::pointed(&Look::ALL, bracketed));
    println!("{:?}", looks::pointed(&Look::ALL, Look::to_string));
    let names: Vec<&str> = Look::ALL.iter().map(Look::name).collect();
    let shades: Vec<u8> = Look::ALL.into_iter().map(Look::shade).collect();
    let mixed: Vec<u8> = Look::ALL
        .iter()
        .zip(Look::ALL)
        .map(|(a, b)| Look::mix(a, b))
        .collect();
    println!("{names:?} {shades:?} {mixed:?}");
    let tones: Vec<i32> = Look::ALL.iter().map(Tone::tone).collect();
    let qualified: Vec<i32> = Look::ALL.iter().map(<Look as Tone>::tone).collect();
    let through: Vec<i32> = Look::ALL.iter().map(Look::tone).collect();
    println!("{tones:?} {qualified:?} {through:?}");
    let name: fn(&Look) -> &'static str = Look::name;
    let mut look = Look::Light;
    Look::flip(&mut look);
    println!(
        "{} {} {}",
        name(&look),
        Look::name(&Look::Light),
        Look::to_string(&look)
    );
}
