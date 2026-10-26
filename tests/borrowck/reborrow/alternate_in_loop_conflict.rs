fn main() {
    let mut even = Vec::new();
    let mut odd = Vec::new();
    let mut r = &mut even;
    for n in 0..6 {
        r.push(n);
        if n % 2 == 0 {
            r = &mut odd;
        } else {
            r = &mut even;
        }
        println!("{}", even.len());
    }
    println!("{even:?} {odd:?}");
}
