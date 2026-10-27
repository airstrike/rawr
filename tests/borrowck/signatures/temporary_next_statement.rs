fn main() {
    let len = {
        let t = String::from("a").as_str();
        t.len()
    };
    println!("{len}");
}
