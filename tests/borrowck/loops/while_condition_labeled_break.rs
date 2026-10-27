fn main() {
    let mut n = 0;
    'w: while {
        loop {
            n += 1;
            if n == 3 {
                break 'w;
            }
            break;
        }
        true
    } {
        println!("body {}", n);
    }
    println!("after {}", n);
}
