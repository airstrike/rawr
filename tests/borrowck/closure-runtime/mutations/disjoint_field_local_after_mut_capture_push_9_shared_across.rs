struct Pair {
    a: Vec<i32>,
    b: Vec<i32>,
}

fn main() {
    let mut pair = Pair { a: vec![1, 2, 3, 4], b: vec![] };
    let mut push = |x| pair.b.push(x);
    let __s = &push;
    push(2);
    __see(__s);
    push(3);
    pair.a.retain(|x| !pair.b.contains(x));
    pair.a.iter_mut().for_each(|x| pair.b.push(*x));
    println!("{:?} {:?}", pair.a, pair.b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
