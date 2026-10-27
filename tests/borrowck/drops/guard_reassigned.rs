struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn main() {
    let mut a = vec![];
    let mut b = vec![];
    let mut g = Guard(&a);
    println!("{}", g.0.len());
    g = Guard(&b);
    a.push(2);
    println!("{} {}", a.len(), g.0.len());
    b.push(1);
}
