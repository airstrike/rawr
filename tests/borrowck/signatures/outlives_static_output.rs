fn keep(x: &i32) -> &'static i32 {
    x
}

fn main() {
    println!("{}", keep(&1));
}
