fn fill(a: &mut Vec<i32>, b: &mut Vec<i32>) {
    let mut push = |x| b.push(x);
    let __b = &mut push;
    if __c() {
        __touch(__b);
    } else {
        push(1);
    }
    a.push(2);
    push(3);
}

fn main() {
    let mut a = vec![];
    let mut b = vec![];
    fill(&mut a, &mut b);
    println!("{a:?} {b:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
