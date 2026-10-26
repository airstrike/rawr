struct W<'a> {
    r: &'a String,
}

fn main() {
    let w = W { r: &String::from("a") };
    let t = (&String::from("b"), 1);
    let b = { &String::from("c") };
    println!("{} {} {b}", w.r, t.0);
}
