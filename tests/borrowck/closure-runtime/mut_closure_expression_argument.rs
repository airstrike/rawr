fn twice(f: &mut impl FnMut(i32)) {
    f(1);
    f(2);
}

fn main() {
    let mut v = Vec::new();
    twice(&mut |x| v.push(x));
    twice(&mut |x| v.push(x * 10));
    println!("{v:?}");
}
