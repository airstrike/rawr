fn twice(f: impl Fn(i32) -> i32 + Copy) -> i32 {
    let g = f;
    g(1) + f(2)
}

fn main() {
    println!("{}", twice(|x| x + 1));
}
