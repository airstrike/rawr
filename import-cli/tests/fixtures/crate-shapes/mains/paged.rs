// rawr

fn main() {
    let page = opener::read_in_background(3).join().unwrap();
    println!("{} {}", page.number(), page.text());
    match opener::find_in_background(5).join().unwrap() {
        Some(page) => println!("found {}", page.text()),
        None => println!("none"),
    }
    println!(
        "{}",
        opener::find_in_background(0).join().unwrap().is_none()
    );
    println!("{:?}", opener::read_in_background(6).join().unwrap());
}
