struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn run(flag: bool) -> usize {
    let mut v = vec![];
    let g = Guard(&v);
    if flag {
        return 0;
    }
    drop(g);
    v.push(1);
    v.len()
}

fn main() {
    println!("{} {}", run(true), run(false));
}
