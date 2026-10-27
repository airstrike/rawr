trait Source {
    type Item;
    fn get(&self) -> Self::Item;
}

struct Counter(u8);

impl Source for Counter {
    type Item = u8;
    fn get(&self) -> u8 {
        self.0
    }
}

fn twice<S: Source>(source: &S) -> (S::Item, S::Item) {
    let item = source.get();
    let first = item;
    (first, item)
}

fn main() {
    println!("{:?}", twice(&Counter(7)));
}
