fn keep<F: 'static + Fn() -> usize>(f: F) -> usize {
    f()
}
fn main() {
    let v = vec![1, 2];
    let n = keep(|| v.len());
    println!("{}", n);
}
