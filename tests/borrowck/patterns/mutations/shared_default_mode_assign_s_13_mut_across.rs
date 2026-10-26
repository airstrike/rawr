struct T {
    field: Vec<i64>,
    other: i64,
}
fn main() {
    let mut s = T { field: vec![1], other: 2 };
    match &s {
        T { field, .. } => {
            s.other = 3;
            println!("{:?}", field);
        }
    }
    let __b = &mut s;
    println!("{}", s.other);
    __touch(__b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
