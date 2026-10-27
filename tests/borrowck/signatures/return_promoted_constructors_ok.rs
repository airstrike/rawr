fn some() -> &'static Option<i32> {
    &Some(5)
}

fn none() -> &'static Option<String> {
    &None
}

fn pair() -> &'static (i32, &'static str) {
    &(1, "a")
}

fn main() {
    println!("{:?} {:?} {:?}", some(), none(), pair());
}
