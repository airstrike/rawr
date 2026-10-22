pub fn identity<T>(value: T) -> T {
    value
}

pub fn add_const<const N: usize>(value: i64) -> i64 {
    value + N as i64
}

pub fn shared<F>(function: F) -> i64
where
    F: Fn(i64) -> i64 + 'static,
{
    function(1)
}

pub fn borrowed<T, F>(value: T, function: F) -> bool
where
    F: for<'a> Fn(&'a T) -> bool,
{
    function(&value)
}

pub fn mutable<F>(mut function: F) -> i64
where
    F: FnMut(i64) -> i64 + 'static,
{
    function(1)
}

pub fn once<F>(function: F) -> i64
where
    F: FnOnce(i64) -> i64,
{
    function(1)
}

pub fn escaping<F>(function: F)
where
    F: for<'a> Fn(&'a i64) -> &'a i64,
{
    let _ = function;
}

pub fn nonstatic_mutable<F>(mut function: F)
where
    F: FnMut(i64) -> i64,
{
    let _ = function(1);
}

pub struct Marker;

impl Marker {
    pub fn identity<T>(self, value: T) -> T {
        value
    }
}

pub struct Builder {
    value: i64,
}

impl Builder {
    pub fn new(value: i64) -> Self {
        Self { value }
    }

    pub fn add(mut self, value: i64) -> Self {
        self.value += value;
        self
    }

    pub fn finish(self) -> i64 {
        self.value
    }
}
