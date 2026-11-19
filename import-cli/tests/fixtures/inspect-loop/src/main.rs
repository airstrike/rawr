// rawr
// A marked `main` that loops for as long as it runs, calling into its
// dependency and back, and never returns to native code.
fn main() {
    println!("pid {}", tally::pid());
    let mut count: u64 = 0;
    loop {
        count = tally::next(count);
    }
}
