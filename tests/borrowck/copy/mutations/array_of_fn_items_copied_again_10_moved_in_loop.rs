fn double(x: i32) -> i32 {
    x * 2
}

fn main() {
    let all = [double; 3];
    let copy = all;
    let total: i32 = all.into_iter().map(|f| f(1)).sum();
    let again: i32 = all.into_iter().map(|f| f(2)).sum();
    for __i in 0..2 {
        if __c() {
            __take(again);
        }
    }
    println!("{} {} {}", total, again, copy[0](5));
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
