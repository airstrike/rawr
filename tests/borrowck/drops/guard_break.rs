struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn main() {
    let mut v = vec![];
    loop {
        let g = Guard(&v);
        if g.0.is_empty() {
            v.push(1);
            break;
        }
    }
}
