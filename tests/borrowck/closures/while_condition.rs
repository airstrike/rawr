fn main() {
    let mut n = 0;
    let mut c = || n += 1;
    while n < 3 {
        c();
    }
}
