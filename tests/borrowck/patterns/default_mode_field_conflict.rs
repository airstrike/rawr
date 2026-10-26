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
    println!("{:?} {}", s.field, s.other);
}
