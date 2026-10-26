struct W<'a> {
    r: &'a i32,
}

fn inner<'a, 'b>(_x: &'a i32, w: W<'b>) -> &'a i32 {
    w.r
}

fn main() {
    let n = 1;
    println!("{}", inner(&0, W { r: &n }));
}
