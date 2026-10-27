fn main() {
    let step = 3;
    let add = |x: i32| x + step;
    let all = [add; 2];
    let copy = all;
    let total: i32 = all.into_iter().map(|f| f(1)).sum();
    __take(copy);
    println!("{} {}", total, copy[1](2));
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
