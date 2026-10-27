fn call(f: &mut impl Fn(i32) -> i32) -> i32 {
    f(2) + f(3)
}

fn main() {
    let base = 10;
    let mut add = |x| x + base;
    println!("{}", call(&mut add));
    println!("{}", add(1));
}
