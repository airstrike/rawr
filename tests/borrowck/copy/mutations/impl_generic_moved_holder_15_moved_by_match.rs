struct Holder<T> {
    value: Option<T>,
}

impl<T> Holder<T> {
    fn both(&mut self) -> Option<(T, T)> {
        let value = self.value.take()?;
        let first = value;
        Some((first, value))
    }
}

fn main() {
    let mut holder = Holder { value: Some(3) };
    match holder {
        __v => __take(__v),
    }
    println!("{:?}", holder.both());
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
