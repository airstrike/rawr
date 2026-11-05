// rawr
use natives::looks::{self, Look, Tone};

fn bracketed(look: &Look) -> String {
    format!("<{look}>")
}

fn main() {
    println!("{:?}", looks::labels(&Look::all(), Look::to_string));
    println!("{:?}", looks::labels(&Look::all(), bracketed));
    let names: Vec<&str> = Look::all().iter().map(Look::name).collect();
    let shades: Vec<u8> = Look::all().into_iter().map(Look::shade).collect();
    let mixed: Vec<u8> = Look::all()
        .iter()
        .zip(Look::all())
        .map(|(a, b)| Look::mix(a, b))
        .collect();
    println!("{names:?} {shades:?} {mixed:?}");
    let tones: Vec<i32> = Look::all().iter().map(Tone::tone).collect();
    let qualified: Vec<i32> = Look::all().iter().map(<Look as Tone>::tone).collect();
    let through: Vec<i32> = Look::all().iter().map(Look::tone).collect();
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
