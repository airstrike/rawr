trait Shape: Copy {
    fn area(self) -> i32;
}

#[derive(Clone, Copy)]
struct Square(i32);

impl Shape for Square {
    fn area(self) -> i32 {
        self.0 * self.0
    }
}

fn where_bound<T>(value: T) -> (T, T)
where
    T: Copy,
{
    let first = value;
    (first, value)
}

fn super_bound<T: Shape>(value: T) -> i32 {
    let first = value;
    first.area() + value.area()
}

fn main() {
    println!("{:?} {}", where_bound(4), super_bound(Square(3)));
}
