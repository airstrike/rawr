struct S {
    field: Vec<i64>,
    other: i64,
}
fn main() {
    let mut s = S { field: vec![1], other: 2 };
    match &mut s {
        S { field, .. } => {
            s.other = 3;
            field.push(4);
        }
    }
    let __s = &s;
    println!("{:?} {}", s.field, s.other);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
