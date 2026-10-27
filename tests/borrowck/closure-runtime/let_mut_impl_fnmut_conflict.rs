fn counter() -> impl FnMut() -> i32 {
    let mut n = 0;
    move || {
        n += 1;
        n
    }
}

fn main() {
    let mut next = counter();
    let peek = &next;
    println!("{}", next());
    let _ = peek;
}
