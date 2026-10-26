fn keep<F: Fn() -> usize + 'static>(f: F) -> usize {
    f()
}
fn main() {
    let v = vec![1, 2];
    let n = keep(move || v.len());
    println!("{}", n);
}
