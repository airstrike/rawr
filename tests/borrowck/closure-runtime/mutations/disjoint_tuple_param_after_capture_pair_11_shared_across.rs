fn update(pair: &mut (Vec<i32>, Vec<i32>)) {
    let mut push = |x| pair.1.push(x);
    push(3);
    pair.0.retain(|x| !pair.1.contains(x));
    pair.0.sort_by_key(|x| pair.1.len() as i32 - *x);
}

fn main() {
    let mut pair = (vec![1, 2, 3, 4], vec![2]);
    update(&mut pair);
    let __s = &pair;
    println!("{:?} {:?}", pair.0, pair.1);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
