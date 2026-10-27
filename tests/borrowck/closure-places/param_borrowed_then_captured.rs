fn bump(r: &mut Vec<i32>) {
    let first = &r[0];
    let mut push = |x| r.push(x);
    push(1);
    println!("{first}");
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
    println!("{v:?}");
}
