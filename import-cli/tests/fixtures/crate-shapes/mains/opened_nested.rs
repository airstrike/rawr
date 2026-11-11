// rawr
use std::thread;

fn main() {
    let given = opener::that_if_given(Some("https://iced.rs"));
    println!(
        "{}",
        given
            .map(|handle| handle.join().unwrap().is_ok())
            .unwrap_or(false)
    );
    println!("{}", opener::that_if_given(None).is_none());
    let (first, second) = opener::both_in_background("https://docs.rs", "");
    println!(
        "{} {}",
        first.join().unwrap().is_ok(),
        second.join().unwrap().is_err()
    );
    println!("{}", opener::finished(opener::check_in_background(3)));
    println!("{}", opener::finished(opener::check_in_background(-3)));
    println!("{}", opener::finished(thread::spawn(|| 6 * 7 == 42)));
}
