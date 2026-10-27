struct Pair<T> {
    left: T,
    right: T,
}

impl<T: Copy + std::fmt::Debug> Pair<T> {
    fn swap(&self) -> Pair<T> {
        let left = self.left;
        let copy = left;
        Pair { left: self.right, right: copy }
    }

    fn show(&self) -> String {
        let right = self.right;
        let again = right;
        format!("{:?} {:?} {:?}", self.left, right, again)
    }
}

fn main() {
    let pair = Pair { left: 1, right: 2 }.swap();
    println!("{}", pair.show());
    __take(pair);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
