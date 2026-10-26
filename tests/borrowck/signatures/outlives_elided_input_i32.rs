fn larger<'a>(x: &'a i32, y: &i32) -> &'a i32 {
    if *x > *y { x } else { y }
}

fn main() {
    println!("{}", larger(&1, &2));
}
