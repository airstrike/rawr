struct S {
    field: Vec<i64>,
    other: i64,
}
fn main() {
    let mut s = S { field: vec![1], other: 2 };
    match &mut s {
        S { field, .. } => {
            field.push(4);
            s.other = 3;
        }
    }
    let __b = &mut s;
    if __c() {
        __touch(__b);
    } else {
        println!("{:?} {}", s.field, s.other);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
