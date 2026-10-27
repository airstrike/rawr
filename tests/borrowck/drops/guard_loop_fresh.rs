struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn main() {
    let mut v = vec![];
    for i in 0..3 {
        let g = Guard(&v);
        println!("{} {}", i, g.0.len());
    }
    v.push(9);
    println!("{}", v.len());
}
