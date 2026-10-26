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
    __touch(__b);
    println!("{:?} {}", s.field, s.other);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
