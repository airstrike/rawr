struct Pair {
    a: Vec<i32>,
    b: Vec<i32>,
}

fn update(p: &mut Pair) {
    let mut push = |x| p.b.push(x);
    push(2);
    push(3);
    p.a.retain(|x| !p.b.contains(x));
    p.a.iter_mut().for_each(|x| p.b.push(*x));
}

fn main() {
    let mut pair = Pair { a: vec![1, 2, 3, 4], b: vec![] };
    let __b = &mut pair;
    if __c() {
        __touch(__b);
    } else {
        update(&mut pair);
    }
    println!("{:?} {:?}", pair.a, pair.b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
