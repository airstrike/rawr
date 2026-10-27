struct W<'a> {
    r: &'a i32,
}

fn f<'a>() -> W<'a> {
    let x = 1;
    W { r: &x }
}

fn main() {
    println!("{}", f().r);
}
