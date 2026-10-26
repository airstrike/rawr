struct T {
    field: Vec<i64>,
    other: i64,
}
fn main() {
    let mut s = T { field: vec![1], other: 2 };
    let r = &mut s;
    match r {
        T { field, .. } => {
            let w = &r;
            println!("{}", w.other);
            field.push(4);
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
