// rawr
use events::flags::{self, Flag, Slot};

fn flag(f: &Flag) -> &'static str {
    match f {
        Flag::On(true) => "on",
        Flag::On(false) => "off",
        Flag::Level(i64::MIN..=-1) => "below",
        Flag::Level(0) => "zero",
        Flag::Level(1..) => "above",
        Flag::Off => "none",
    }
}

fn slot(s: Slot) -> &'static str {
    match s {
        Slot {
            a: Some(_),
            b: true,
        } => "some on",
        Slot {
            a: Some(_),
            b: false,
        } => "some off",
        Slot { a: None, .. } => "empty",
    }
}

fn main() {
    let names: Vec<&str> = flags::flags().iter().map(flag).collect();
    println!("{}", names.join(" "));
    println!(
        "{} {} {}",
        slot(flags::slot(Some(1), true)),
        slot(flags::slot(Some(2), false)),
        slot(flags::slot(None, true))
    );
}
