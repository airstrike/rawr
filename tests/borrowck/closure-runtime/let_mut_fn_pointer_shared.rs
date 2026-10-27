fn double(x: i32) -> i32 {
    x * 2
}

fn triple(x: i32) -> i32 {
    x * 3
}

fn main() {
    let mut f: fn(i32) -> i32 = double;
    let g = &f;
    println!("{} {}", f(1), g(2));
    f = triple;
    println!("{}", f(1));
}
