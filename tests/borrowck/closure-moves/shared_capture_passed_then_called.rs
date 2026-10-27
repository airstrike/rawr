fn call(f: impl Fn() -> usize) -> usize {
    f()
}

fn main() {
    let v = vec![1, 2];
    let c = || v.len();
    println!("{} {}", call(c), c());
}
