fn main() {
    let mut n = 0;
    let mut log = Vec::new();
    'w: while {
        loop {
            n += 1;
            if n % 2 == 0 {
                continue 'w;
            }
            break;
        }
        n < 7
    } {
        log.push(n);
    }
    println!("{:?} {}", log, n);
}
