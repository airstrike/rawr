fn take(x: String) -> usize {
    let c = || x;
    let y = x;
    y.len()
}

fn main() {
    println!("{}", take(String::from("ab")));
}
