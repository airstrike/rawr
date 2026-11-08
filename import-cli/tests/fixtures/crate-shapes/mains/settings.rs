// rawr
use natives::form::Setting;

fn main() {
    let lengths: Vec<u16> = Setting::<u16>::parse_list("ab,c")
        .map(|setting| setting.0)
        .collect();
    let mut halves = Setting::<f32>::parse_list("abc");
    println!("{lengths:?} {:?}", halves.next().map(|setting| setting.0));
}
