fn main() {
    let mut v = vec![1];
    let empty = v.is_empty();
    let r = &mut v;
    if empty {
        v.push(1);
        panic!("empty");
    }
    r.push(2);
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        println!("{:?}", v);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
