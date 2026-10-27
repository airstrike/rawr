fn keep(f: impl Fn() -> usize + 'static) -> usize {
    f()
}
fn main() {
    let v = vec![1, 2];
    let n = keep(|| v.len());
    println!("{}", n);
}
