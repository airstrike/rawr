//! Allocation-free, stack-independent destruction of success and partial data.
//! Dotted keys inside nested inline tables can make retained depth much greater
//! than the raw parser's recursion depth. A cursor in each existing container
//! scans its slots once; repeated descent finds and destroys one leaf container.
//! No path stack, recursion fallback, depth assertion, or new rejection exists.

use super::{Array, Table, Value};

enum Container<'a, 'i> {
    Table(&'a mut Table<'i>),
    Array(&'a mut Array<'i>),
}

impl<'i> Container<'_, 'i> {
    fn next_container(&mut self) -> Option<usize> {
        loop {
            let (index, len) = match self {
                Self::Table(table) => (*table.disposal(), table.slot_count()),
                Self::Array(array) => (array.disposal, array.values.len()),
            };
            if index >= len {
                return None;
            }
            if matches!(self.slot(index), Some(Value::Table(_) | Value::Array(_))) {
                return Some(index);
            }
            match self {
                Self::Table(table) => *table.disposal() += 1,
                Self::Array(array) => array.disposal += 1,
            }
        }
    }

    fn slot(&mut self, index: usize) -> Option<&mut Value<'i>> {
        match self {
            Self::Table(table) => table.slot_mut(index),
            Self::Array(array) => array
                .values
                .as_mut_slice()
                .get_mut(index)
                .map(|item| &mut item.value),
        }
    }
}

pub(super) fn table(table: &mut Table<'_>) {
    drain(Container::Table(table));
}

pub(super) fn array(array: &mut Array<'_>) {
    drain(Container::Array(array));
}

fn drain(mut root: Container<'_, '_>) {
    while let Some(index) = root.next_container() {
        let Some(mut value) = root.slot(index) else {
            continue;
        };
        loop {
            let next = match &mut *value {
                Value::Table(table) => Container::Table(table).next_container(),
                Value::Array(array) => Container::Array(array).next_container(),
                _ => None,
            };
            if let Some(index) = next {
                value = match value {
                    Value::Table(table) => table.slot_mut(index).expect("live disposal slot"),
                    Value::Array(array) => &mut array.values.as_mut_slice()[index].value,
                    _ => unreachable!("disposal selected a container"),
                };
            } else {
                // This container has no child containers. Its own destructor
                // observes an exhausted cursor, so dropping it cannot recurse.
                *value = Value::Boolean(false);
                break;
            }
        }
    }
}
