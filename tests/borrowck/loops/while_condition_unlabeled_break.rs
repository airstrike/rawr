fn main() {
    let mut n = 0;
    'outer: loop {
        while {
            n += 1;
            if n > 2 {
                break;
            }
            true
        } {}
        break 'outer;
    }
    println!("{}", n);
}
