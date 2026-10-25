//! A bank-statement tool whose users write their own rules in Rust. It
//! exports `Transaction`, reads a CSV, loads the user's `rules.rs` and
//! prints the total for each category. `scripts/embed-check.sh` runs it.
//!
//! - `cargo run -p rules` uses `rules.rs`.
//! - `cargo run -p rules -- scripts/typo.rs` fails as the rules load.
//! - `cargo run -p rules -- scripts/wrong.rs` fails as the app looks up `category`.
//! - `cargo run -p rules -- rules.rs scripts/edited.rs` saves an edit and reloads.
//! - `cargo run -p rules -- rules.rs scripts/wrong.rs` keeps the old rules.

use rawr::{Engine, Exports, Func};
use std::collections::BTreeMap;

#[derive(Clone)]
#[rawr::export]
pub struct Transaction {
    pub date: String,
    pub payee: String,
    pub amount: f64,
}

#[rawr::export]
impl Transaction {
    pub fn is_income(&self) -> bool {
        self.amount > 0.0
    }
}

fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    // Paths are relative to the app, as rustc names source files.
    std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).expect("the app's directory");
    let mut args = std::env::args().skip(1);
    let rules = args.next().unwrap_or("rules.rs".into());
    let edit = args.next();
    let transactions = read("transactions.csv");

    let mut bank = Exports::new("bank");
    bank.add::<Transaction>();
    let mut engine = Engine::new();
    engine.export(bank)?;

    // To show a reload, work on a copy the app can save an edit over.
    let path = match &edit {
        Some(_) => {
            let copy = std::env::current_exe().unwrap().with_file_name("rules.rs");
            std::fs::copy(&rules, &copy).expect("a copy of the rules");
            copy.to_string_lossy().into_owned()
        }
        None => rules,
    };

    let script = engine.load(&path)?;
    let category = script.func::<(Transaction,), Option<String>>("category")?;
    report(&category, &transactions)?;

    if let Some(edit) = edit {
        // The user saves new rules. The app reloads them between reports,
        // and `category` runs the new code at its next call. Rules that
        // fail to compile, or no longer fit `category`, leave the old rules
        // running.
        std::fs::copy(&edit, &path).expect("the edited rules");
        match script.reload() {
            Ok(true) => println!("reloaded"),
            Ok(false) => println!("unchanged"),
            Err(error) => eprintln!("{error}"),
        }
        report(&category, &transactions)?;
    }
    Ok(())
}

/// The total for each category, in name order. A transaction the rules
/// leave out is `other`.
fn report(
    category: &Func<(Transaction,), Option<String>>,
    transactions: &[Transaction],
) -> Result<(), Box<rawr::EvalAltResult>> {
    let mut totals = BTreeMap::new();
    for tx in transactions {
        let name = category.call(tx.clone())?.unwrap_or("other".into());
        *totals.entry(name).or_insert(0.0) += tx.amount;
    }
    for (name, total) in totals {
        println!("{name:<10} {total:>8.2}");
    }
    Ok(())
}

fn read(path: &str) -> Vec<Transaction> {
    let text = std::fs::read_to_string(path).expect("the transactions");
    text.lines()
        .skip(1)
        .map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            Transaction {
                date: fields[0].into(),
                payee: fields[1].into(),
                amount: fields[2].parse().expect("an amount"),
            }
        })
        .collect()
}
