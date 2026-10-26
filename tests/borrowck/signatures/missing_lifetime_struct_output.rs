struct W<'a> {
    r: &'a i32,
}

fn f() -> W {
    W { r: &1 }
}

fn main() {
    println!("{}", f().r);
}
