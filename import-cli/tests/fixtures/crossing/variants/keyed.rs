// rawr
use std::collections::{BTreeMap, HashMap, HashSet};

use events::keyed::{self, Item};

fn main() {
    let mut items = HashMap::new();
    items.insert("pen".to_string(), Item::new("pen", 3));
    items.insert("ink".to_string(), Item::new("ink", 4));
    println!("total {}", keyed::total(&items));
    println!("kept {} {}", items["pen"].price(), items.len());
    let mut sorted = BTreeMap::new();
    sorted.insert("pad".to_string(), Item::new("pad", 9));
    sorted.insert("nib".to_string(), Item::new("nib", 2));
    println!(
        "cheapest {} {}",
        keyed::cheapest(&sorted),
        sorted["nib"].name()
    );
    println!("prices {:?}", keyed::prices());
    let catalog = keyed::catalog();
    let mut names: Vec<String> = catalog.keys().cloned().collect();
    names.sort();
    println!("catalog {:?} {}", names, catalog["cup"].price());
    let mut words = HashSet::new();
    words.insert("one".to_string());
    words.insert("three".to_string());
    println!("tags {}", keyed::tags(&words));
    println!("evens {:?}", keyed::evens(7));
    let mut letters: Vec<char> = keyed::letters().into_iter().collect();
    letters.sort();
    println!("letters {:?}", letters);
    let mut counts = HashMap::new();
    counts.insert("one".to_string(), 1);
    counts.insert("two".to_string(), 2);
    println!("invert {:?}", keyed::invert(counts));
}
