struct Pair {
    a: Vec<i32>,
    b: Vec<i32>,
}

fn main() {
    let mut pairs = vec![Pair { a: vec![1, 2, 3], b: vec![] }];
    if let Some(p) = pairs.get_mut(0) {
        let mut push = |x| p.b.push(x);
        push(2);
        p.a.retain(|x| !p.b.contains(x));
        p.a.iter_mut().for_each(|x| p.b.push(*x * 10));
    }
    for __i in 0..2 {
        let __b = &mut pairs;
        __touch(__b);
        println!("{:?} {:?}", pairs[0].a, pairs[0].b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
