fn main() {
    let mut n = 0;
    loop {
        while n > 3 || continue {
            n += 1;
        }
        n += 1;
        if n > 5 {
            break;
        }
    }
    println!("{n}");
}
