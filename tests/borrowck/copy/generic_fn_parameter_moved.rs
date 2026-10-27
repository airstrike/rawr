fn twice<F: Fn(i32) -> i32>(f: F) -> i32 {
    let g = f;
    g(1) + f(2)
}

fn main() {
    println!("{}", twice(|x| x + 1));
}
