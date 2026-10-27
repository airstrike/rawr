fn main() {
    let step = 3;
    let add = |x: i32| x + step;
    let all = [add; 2];
    let copy = all;
    let total: i32 = all.into_iter().map(|f| f(1)).sum();
    println!("{} {}", total, copy[1](2));
}
