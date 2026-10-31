// rawr
struct Editor;

fn helper(values: &mut Vec<String>) {
    values[0].push_str("-helper");
    values.push("helper call".to_string());
}

impl events::sequences::Callback for Editor {
    fn edit(
        &self,
        values: &mut Vec<String>,
        other: &mut Vec<String>,
        mode: u8,
    ) -> Result<(), String> {
        if mode == 1 {
            values[0].push_str("-edit");
        }
        if mode == 2 {
            values.push("growth".to_string());
            values.pop();
        }
        if mode == 3 {
            *values = vec!["replaced".to_string(), "twice".to_string()];
        }
        if mode == 4 {
            let moved = std::mem::take(values);
            *values = moved;
        }
        if mode == 5 {
            values.append(other);
            return Err("after append".to_string());
        }
        if mode == 6 {
            values[0] = "before panic".to_string();
            panic!("mutation panic");
        }
        if mode == 7 {
            let mut count = 0;
            values.resize_with(5, || {
                count += 1;
                if count == 3 {
                    panic!("constructor panic");
                }
                "made".to_string()
            });
        }
        if mode == 8 {
            events::sequences::mutate(values);
        }
        if mode == 9 {
            helper(values);
        }
        Ok(())
    }
}

impl events::sequences::Locals for Editor {
    fn edit(&self, values: &mut Vec<events::sequences::Local>) {
        let value = values.pop().unwrap();
        values.push(value);
    }
}

impl events::sequences::Counting for Editor {
    fn edit(&self, values: &mut Vec<events::sequences::Counted>, mode: u8) {
        // Observing the vector clones and drops nothing.
        if mode == 0 {
            assert_eq!(values.len(), 3);
            assert!(!values.is_empty());
        }
        // Elements move within the native vector without a clone.
        if mode == 1 {
            values.swap(0, 2);
            values.swap(1, 2);
        }
        // A removed element drops once, where Rust drops it.
        if mode == 2 {
            values.remove(1);
        }
        // An explicit clone of an element is the only copy, dropped once.
        if mode == 3 {
            let copy = values[0].clone();
            drop(copy);
        }
    }
}

fn slot(values: &mut Vec<String>) {
    values[0].push_str("-slot");
    values.push("slot call".to_string());
}

fn main() {
    events::sequences::run(Editor);
    events::sequences::local_moves(Editor);
    events::sequences::counted(Editor);
    events::sequences::function(slot, slot);
    events::sequences::callable(|values: &mut Vec<String>| {
        values[0].push_str("-closure");
        values.push("closure call".to_string());
    });
}
