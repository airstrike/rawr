fn last(v: &mut Vec<i32>) -> &mut i32 {
    let n = v.len();
    &mut v[n - 1]
}

fn main() {
    let mut v = vec![1, 2, 3];
    let l = last(&mut v);
    println!("{}", v.len());
    *l += 1;
}
