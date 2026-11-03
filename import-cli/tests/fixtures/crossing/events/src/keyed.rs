//! std's maps and sets in a crate's signature (#777): rawr's keyed
//! collections, from which the crate's are made where they cross. A map
//! of crate values lends them to a call that borrows it, without a copy.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// No `Clone`: a map of these can only cross by moving or lending them.
#[derive(Debug)]
pub struct Item {
    name: String,
    price: i64,
}

impl Item {
    pub fn new(name: &str, price: i64) -> Self {
        Item {
            name: name.to_string(),
            price,
        }
    }

    pub fn price(&self) -> i64 {
        self.price
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }
}

pub fn total(items: &HashMap<String, Item>) -> i64 {
    items.values().map(|item| item.price).sum()
}

pub fn cheapest(items: &BTreeMap<String, Item>) -> String {
    items
        .values()
        .min_by_key(|item| item.price)
        .map(|item| item.name.clone())
        .unwrap_or_default()
}

pub fn prices() -> BTreeMap<String, i64> {
    BTreeMap::from([("b".to_string(), 2), ("a".to_string(), 1)])
}

pub fn catalog() -> HashMap<String, Item> {
    HashMap::from([
        ("cup".to_string(), Item::new("cup", 5)),
        ("mug".to_string(), Item::new("mug", 6)),
    ])
}

pub fn tags(words: &HashSet<String>) -> usize {
    words.iter().map(String::len).sum()
}

pub fn evens(limit: i64) -> BTreeSet<i64> {
    (0..limit).filter(|n| n % 2 == 0).collect()
}

pub fn letters() -> HashSet<char> {
    "abcab".chars().collect()
}

pub fn invert(map: HashMap<String, i64>) -> BTreeMap<i64, String> {
    map.into_iter().map(|(key, value)| (value, key)).collect()
}
