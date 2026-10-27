fn main() {
    let step = 3;
    let add = |x: i32| x + step;
    let all = [add; 2];
    let copy = all;
    let total: i32 = all.into_iter().map(|f| f(1)).sum();
    for __i in 0..2 {
        if __c() {
            __take(total);
        }
    }
    println!("{} {}", total, copy[1](2));
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
