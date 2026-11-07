// rawr
use natives::lifetimes::Clock;

fn main() {
    let clock = Clock::new(9, 5);
    println!("{}", clock.strftime("%H:%M"));
    let format = String::from("at %H h");
    let shown = clock.strftime(&format);
    println!("{shown}");
    println!("{}", clock.strftime("%M past").to_string());
    let bytes: &[u8] = b"%H%M";
    println!("{}", clock.strftime(bytes));
}
