fn make() -> Vec<i32> {
    vec![3, 1, 2]
}

fn main() {
    let Some(first) = make().first() else {
        return;
    };
    println!("{first}");
}
