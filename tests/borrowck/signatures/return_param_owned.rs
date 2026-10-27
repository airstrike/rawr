fn f(v: Vec<i32>) -> &'static i32 {
    &v[0]
}

fn main() {
    println!("{}", f(vec![1]));
}
