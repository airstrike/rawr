// rawr

fn main() {
    let _ = opener::that_in_background("https://iced.rs");
    let opened = opener::that_in_background(String::from("https://docs.rs"));
    println!("{}", opened.join().unwrap().is_ok());
    println!(
        "{}",
        opener::that_in_background("").join().unwrap().is_err()
    );
}
