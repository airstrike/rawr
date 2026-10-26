fn main() {
    let mut n = 1;
    let r = &mut n;
    let n = 10;
    *r += 5;
    println!("{n} {r}");
}
