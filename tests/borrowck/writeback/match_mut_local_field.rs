struct T {
    field: Vec<i64>,
    other: i64,
    count: i64,
}
fn main() {
    let mut s = T { field: vec![1], other: 2, count: 0 };
    let r = &mut s;
    match r {
        T { field, count, .. } => {
            field.push(4);
            r.other += 10;
            *count += 1;
            field.push(5);
            r.other += 100;
        }
    }
    println!("{:?} {} {}", s.field, s.other, s.count);
}
