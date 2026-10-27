fn make(step: i32) -> impl Fn(i32) -> i32 {
    move |x| x + step
}

fn main() {
    let mut f = make(1);
    let g = &f;
    println!("{} {}", f(1), g(2));
    f = make(10);
    println!("{}", f(1));
}
