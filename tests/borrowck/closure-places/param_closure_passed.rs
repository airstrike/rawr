fn twice(f: &mut impl FnMut(i32)) {
    f(1);
    f(2);
}

fn bump(r: &mut Vec<i32>) {
    twice(&mut |x| r.push(x));
    r.push(3);
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
    println!("{v:?}");
}
