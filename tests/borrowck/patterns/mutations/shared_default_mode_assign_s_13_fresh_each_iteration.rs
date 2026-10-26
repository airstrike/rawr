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
    for __i in 0..2 {
        let __b = &mut s;
        __touch(__b);
        println!("{}", s.other);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
