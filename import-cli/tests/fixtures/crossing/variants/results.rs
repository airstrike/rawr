// rawr
struct Callback;

impl events::results::Callback for Callback {
    fn check(&self, fail: bool) -> Result<(), String> {
        if fail {
            Err("check".to_string())
        } else {
            Ok(())
        }
    }

    fn value(&self, fail: bool) -> Result<i64, String> {
        if fail {
            Err("value".to_string())
        } else {
            Ok(17)
        }
    }

    fn echo(&self, value: Result<i64, String>) -> Result<i64, String> {
        value
    }
}

fn main() {
    events::results::run(Callback);
}
