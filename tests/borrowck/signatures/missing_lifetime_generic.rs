fn either<T>(a: &T, b: &T) -> &T {
    a
}

fn main() {
    println!("{}", either(&1, &2));
}
