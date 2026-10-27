struct Guard<'a>(&'a Vec<i32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        println!("guard drops with {}", self.0.len());
    }
}

fn keep(g: Guard<'_>, v: &mut Vec<i32>) {
    v.push(1);
    println!("{}", g.0.len());
}

fn main() {
    let a = vec![];
    let mut b = vec![];
    keep(Guard(&a), &mut b);
    println!("{} {}", a.len(), b.len());
}
