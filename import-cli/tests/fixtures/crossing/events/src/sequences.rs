//! Native storage observations for callback lending and actual owned moves.

pub trait Callback {
    fn edit(
        &self,
        values: &mut Vec<String>,
        other: &mut Vec<String>,
        mode: u8,
    ) -> Result<(), String>;
}

fn text(value: &str, capacity: usize) -> String {
    let mut text = String::with_capacity(capacity);
    text.push_str(value);
    text
}

pub fn run(callback: impl Callback) {
    for mode in 0..10 {
        let mut values = Vec::with_capacity(if mode == 2 { 1 } else { 16 });
        values.push(text("native", 64));
        let mut other = Vec::with_capacity(8);
        other.push(text("other", 80));
        let (vector, string, donor, donor_string) = (
            values.as_ptr(),
            values[0].as_ptr(),
            other.as_ptr(),
            other[0].as_ptr(),
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            callback.edit(&mut values, &mut other, mode)
        }));
        let outcome = match result {
            Ok(value) => format!("{value:?}"),
            Err(_) => "panic".to_owned(),
        };
        println!(
            "mode={mode} values={values:?} cap={} vector={} string={} other={other:?} donor-cap={} donor={} moved={} result={outcome}",
            values.capacity(), values.as_ptr() == vector,
            values.first().is_some_and(|value| value.as_ptr() == string),
            other.capacity(), other.as_ptr() == donor,
            values.iter().any(|value| value.as_ptr() == donor_string),
        );
    }
}

pub fn mutate(values: &mut Vec<String>) {
    values[0].push_str("-native");
    values.push("native call".to_owned());
}

pub struct Local(std::rc::Rc<std::cell::Cell<usize>>);
impl Drop for Local {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
pub trait Locals {
    fn edit(&self, values: &mut Vec<Local>);
}
pub fn local_moves(callback: impl Locals) {
    let drops = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut values = Vec::with_capacity(8);
    values.push(Local(drops.clone()));
    let pointer = values.as_ptr();
    callback.edit(&mut values);
    println!(
        "local len={} drops={} capacity={} allocation={}",
        values.len(),
        drops.get(),
        values.capacity(),
        pointer == values.as_ptr()
    );
    drop(values);
    println!("local final-drops={}", drops.get());
}

/// An element that counts its clones and drops, so a callback that only
/// observes the vector shows it made neither.
pub struct Counted {
    pub value: i64,
    tally: std::rc::Rc<std::cell::Cell<(usize, usize)>>,
}
impl Clone for Counted {
    fn clone(&self) -> Self {
        let (clones, drops) = self.tally.get();
        self.tally.set((clones + 1, drops));
        Self {
            value: self.value,
            tally: self.tally.clone(),
        }
    }
}
impl Drop for Counted {
    fn drop(&mut self) {
        let (clones, drops) = self.tally.get();
        self.tally.set((clones, drops + 1));
    }
}
pub trait Counting {
    fn edit(&self, values: &mut Vec<Counted>, mode: u8);
}
pub fn counted(callback: impl Counting) {
    for mode in 0..4 {
        let tally = std::rc::Rc::new(std::cell::Cell::new((0, 0)));
        let mut values = Vec::with_capacity(6);
        for value in [1, 2, 3] {
            values.push(Counted {
                value,
                tally: tally.clone(),
            });
        }
        let pointer = values.as_ptr();
        callback.edit(&mut values, mode);
        let (clones, drops) = tally.get();
        let order: Vec<i64> = values.iter().map(|value| value.value).collect();
        println!("counted mode={mode} values={order:?} clones={clones} drops={drops} capacity={} allocation={}", values.capacity(), pointer == values.as_ptr());
    }
}

fn call(name: &str, callback: impl Fn(&mut Vec<String>)) {
    let mut values = Vec::with_capacity(12);
    values.push(text("entry", 48));
    let (vector, string) = (values.as_ptr(), values[0].as_ptr());
    callback(&mut values);
    println!(
        "{name} values={values:?} cap={} vector={} string={}",
        values.capacity(),
        values.as_ptr() == vector,
        values[0].as_ptr() == string
    );
}
/// Two callable parameters, as `iced::run(update, view)` takes: script
/// functions fill them by slots (D594).
pub fn function<F: Fn(&mut Vec<String>), G: Fn(&mut Vec<String>)>(first: F, second: G) {
    call("slot", first);
    call("second slot", second);
}
pub fn callable(callback: impl Fn(&mut Vec<String>) + 'static) {
    call("closure", callback);
}
