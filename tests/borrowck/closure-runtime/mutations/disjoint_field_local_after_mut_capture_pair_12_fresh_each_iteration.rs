struct Pair {
    a: Vec<i32>,
    b: Vec<i32>,
}

fn main() {
    let mut pair = Pair { a: vec![1, 2, 3, 4], b: vec![] };
    let mut push = |x| pair.b.push(x);
    push(2);
    push(3);
    pair.a.retain(|x| !pair.b.contains(x));
    for __i in 0..2 {
        let __b = &mut pair;
        __touch(__b);
        pair.a.iter_mut().for_each(|x| pair.b.push(*x));
    }
    println!("{:?} {:?}", pair.a, pair.b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
