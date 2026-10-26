fn name(_v: &Vec<i32>) -> &'static str {
    "vec"
}

fn main() {
    let mut v = vec![1];
    let n = name(&v);
    v.push(2);
    println!("{n} {v:?}");
}
