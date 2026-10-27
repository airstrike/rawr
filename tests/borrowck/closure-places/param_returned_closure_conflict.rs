fn pusher(r: &mut Vec<i32>) -> impl FnMut(i32) + '_ {
    move |x| r.push(x)
}

fn main() {
    let mut v = vec![0];
    let mut p = pusher(&mut v);
    p(1);
    v.push(5);
    p(2);
    println!("{v:?}");
}
