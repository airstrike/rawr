//! Native signatures must retain Result even when both variants hold builtins.

pub trait Callback {
    fn check(&self, fail: bool) -> Result<(), String>;
    fn value(&self, fail: bool) -> Result<i64, String>;
    fn echo(&self, value: Result<i64, String>) -> Result<i64, String>;
}

pub fn run(callback: impl Callback) {
    println!(
        "check {:?} {:?}",
        callback.check(false),
        callback.check(true)
    );
    println!(
        "value {:?} {:?}",
        callback.value(false),
        callback.value(true)
    );
    println!(
        "echo {:?} {:?}",
        callback.echo(Ok(23)),
        callback.echo(Err("input".to_owned()))
    );
}
