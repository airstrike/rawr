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
    println!("{:?} {}", s.field, s.other);
}
