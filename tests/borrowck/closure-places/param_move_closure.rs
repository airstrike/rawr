fn bump(r: &mut Vec<i32>) {
    let mut push = move |x| r.push(x);
    push(1);
    push(2);
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
    println!("{v:?}");
}
