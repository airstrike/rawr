//! This script defines a trait method and calls it on an integer.

trait Action {
    fn action(&mut self, x: i64, y: i64);
}

impl Action for i64 {
    // `self` is the value the method is called on, and it can be modified
    fn action(&mut self, x: i64, y: i64) {
        *self = self.abs() + x * y;
    }
}

let mut obj: i64 = -40;

obj.action(1, 2);                   // call 'action' as a method

println!("obj should now be 42: {obj}");
