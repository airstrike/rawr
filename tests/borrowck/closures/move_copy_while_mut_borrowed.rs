fn main() {
    let mut n = 1;
    let r = &mut n;
    let c = move || n + 1;
    *r += 1;
    println!("{}", c());
}
