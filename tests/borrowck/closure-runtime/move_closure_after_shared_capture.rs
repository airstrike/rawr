fn main() {
    let mut n = 1;
    {
        let show = || println!("{n}");
        show();
    }
    let mut c = move || {
        n += 1;
        n
    };
    println!("{} {}", c(), c());
    println!("{n}");
    n += 5;
    println!("{} {n}", c());
}
