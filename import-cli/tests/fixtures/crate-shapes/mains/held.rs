// rawr
use std::thread;

fn main() {
    let held = opener::Held::new(thread::spawn(|| 6 * 7));
    println!("{}", held.into_inner().join().unwrap());
    let checked = opener::Held::new(opener::check_in_background(1));
    println!("{}", checked.into_inner().join().unwrap());
}
