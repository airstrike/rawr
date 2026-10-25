// A trait, two types that implement it, and generic code over both.
trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> String;
}

struct Circle {
    r: f64,
}

struct Rect {
    w: f64,
    h: f64,
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        3.14159 * self.r * self.r
    }
    fn name(&self) -> String {
        format!("circle r={}", self.r)
    }
}

impl Shape for Rect {
    fn area(&self) -> f64 {
        self.w * self.h
    }
    fn name(&self) -> String {
        format!("rect {}x{}", self.w, self.h)
    }
}

fn largest<T: Shape>(shapes: &[T]) -> Option<&T> {
    let mut best: Option<&T> = None;
    for shape in shapes {
        if best.is_none_or(|b| shape.area() > b.area()) {
            best = Some(shape);
        }
    }
    best
}

fn describe(shape: &dyn Shape) {
    println!("{:<14} {:>7.2}", shape.name(), shape.area());
}

fn main() {
    let circles = [Circle { r: 1.0 }, Circle { r: 2.5 }];
    let rects = [Rect { w: 3.0, h: 4.0 }, Rect { w: 2.0, h: 9.5 }];
    describe(largest(&circles).unwrap());
    describe(largest(&rects).unwrap());
}
