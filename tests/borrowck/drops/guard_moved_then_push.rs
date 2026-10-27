struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn take(g: Guard<'_>) {
    println!("took {}", g.0.len());
}

fn main() {
    let mut v = vec![];
    let g = Guard(&v);
    take(g);
    v.push(1);
    println!("{}", v.len());
}
