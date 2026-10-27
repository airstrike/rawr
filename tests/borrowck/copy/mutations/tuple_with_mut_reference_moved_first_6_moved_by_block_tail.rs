fn main() {
    let mut list = vec![1, 2, 3];
    let pair = (&mut list, 1);
    let first = pair;
    let second = pair;
    let __m = { first };
    first.0.push(4);
    println!("{}", second.1);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
