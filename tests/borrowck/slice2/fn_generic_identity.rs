fn id<T>(x: T) -> T {
    x
}

fn main() {
    let mut v = vec![1];
    let r = id(&v);
    v.push(2);
    println!("{r:?}");
}
