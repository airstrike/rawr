fn make() -> Vec<i32> {
    vec![3, 1, 2]
}

fn name() -> String {
    String::from("rawr")
}

fn main() {
    let Some(last) = make().last().copied() else {
        return;
    };
    if let Some(first) = make().first() {
        println!("{first}");
    }
    match name().as_str() {
        "rawr" => println!("yes"),
        other => println!("{other}"),
    }
    for c in name().chars().rev() {
        print!("{c}");
    }
    println!();
    let total: i32 = make().iter().sum();
    let upper = name().as_str().to_uppercase();
    let len = name().trim().len();
    let joined = [name(), name()].concat();
    let s: &str = &name();
    let n = &make()[0];
    println!("{last} {total} {upper} {len} {joined} {s} {n}");
}
