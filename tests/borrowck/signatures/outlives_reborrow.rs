fn first<'a, 'b>(_x: &'a Vec<i32>, y: &'b Vec<i32>) -> &'a i32 {
    &y[0]
}

fn main() {
    println!("{}", first(&vec![1], &vec![2]));
}
