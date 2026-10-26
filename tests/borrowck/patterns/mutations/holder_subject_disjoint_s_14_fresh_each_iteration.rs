struct T {
    field: Vec<i64>,
    other: i64,
}
fn main() {
    let mut s = T { field: vec![1], other: 2 };
    let r = &mut s;
    match r {
        T { field, .. } => {
            r.other = 3;
            field.push(4);
        }
    }
    for __i in 0..2 {
        let __b = &mut s;
        __touch(__b);
        println!("{:?} {}", s.field, s.other);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
