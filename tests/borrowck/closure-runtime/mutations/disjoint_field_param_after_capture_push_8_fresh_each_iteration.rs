struct Pair {
    a: Vec<i32>,
    b: Vec<i32>,
}

fn update(p: &mut Pair) {
    let mut push = |x| p.b.push(x);
    for __i in 0..2 {
        let __b = &mut push;
        __touch(__b);
        push(2);
    }
    push(3);
    p.a.retain(|x| !p.b.contains(x));
    p.a.iter_mut().for_each(|x| p.b.push(*x));
}

fn main() {
    let mut pair = Pair { a: vec![1, 2, 3, 4], b: vec![] };
    update(&mut pair);
    println!("{:?} {:?}", pair.a, pair.b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
