// rawr
use natives::form::Setting;

fn main() {
    let whole: Vec<Setting<u16>> = Setting::<u16>::parse_list("1, 2, x").collect();
    let mut halves = Setting::<f32>::parse_list("0.5, 1.5");
    println!("{whole:?} {:?} {}", halves.next(), halves.count());
}
