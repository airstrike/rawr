//! This script shows object-oriented programming (OOP) with structs and a trait.

// Shared behaviour, with a default method that every object gets.
trait Object {
    fn data(&self) -> i64;
    fn set_data(&mut self, x: i64);

    fn action(&self) {
        println!("Data={}", self.data());
    }

    // The default setter, which another object can replace.
    fn update(&mut self, x: i64) -> i64 {
        self.set_data(x);
        self.action();
        self.data()
    }
}

// Define an object
struct First {
    data: i64,
}

impl Object for First {
    fn data(&self) -> i64 { self.data }
    fn set_data(&mut self, x: i64) { self.data = x; }
}

// Define another object, which replaces only the setter
struct Second {
    data: i64,
}

impl Object for Second {
    fn data(&self) -> i64 { self.data }
    fn set_data(&mut self, x: i64) { self.data = x; }

    fn update(&mut self, x: i64) -> i64 {
        self.set_data(x * 2);
        self.action();
        self.data()
    }
}

let mut last_value = 0;

let mut obj1 = First { data: 42 };

if obj1.data() > 0 {                            // property access
    last_value = obj1.update(123);              // call method
} else {
    println!("we have a problem here");
}

let mut obj2 = Second { data: 0 };

if obj2.data() > 0 {                            // property access
    println!("we have another problem here");
} else {
    last_value = obj2.update(42);               // call method
}

println!("Should be 84: {last_value}");
