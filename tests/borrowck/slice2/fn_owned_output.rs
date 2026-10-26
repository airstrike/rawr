fn sum(v: &Vec<i32>) -> i32 {
    v.iter().sum()
}

fn main() {
    let mut v = vec![1, 2];
    let s = sum(&v);
    v.push(s);
    println!("{v:?}");
}
