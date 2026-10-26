fn id<T>(x: &T) -> &T {
    x
}

fn main() {
    let mut v = vec![1];
    let keep = &v;
    let c = std::env::args().count() > 5;
    let t = if c { id(&keep) } else { id(&keep) };
    v.push(2);
    println!("{t:?}");
}
