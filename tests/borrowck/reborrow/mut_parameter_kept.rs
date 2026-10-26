fn fill(mut r: &mut Vec<i32>, n: i32) {
    r.push(n);
    r.push(n + 1);
}

fn main() {
    let mut a = vec![1];
    fill(&mut a, 5);
    println!("{a:?}");
}
