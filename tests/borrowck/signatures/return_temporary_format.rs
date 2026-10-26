fn f(n: i32) -> &'static String {
    &format!("{n}")
}

fn main() {
    println!("{}", f(1));
}
