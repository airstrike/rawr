struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn main() {
    let mut v = vec![];
    let o = Some(Guard(&v));
    v.push(1);
}
