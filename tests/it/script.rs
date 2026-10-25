//! `Script` and its typed handles (D514): an app's exported types cross a
//! checked handle, and `Script::reload` swaps every handle to a new program
//! or keeps the old one.
#![cfg(not(feature = "no_function"))]
#![cfg(not(feature = "no_std"))]

use rawr::{Engine, Exports, Script};
use std::path::PathBuf;

#[derive(Clone)]
#[rawr::export]
pub struct Transaction {
    pub payee: String,
    pub amount: f64,
}

fn engine() -> Engine {
    let mut bank = Exports::new("bank");
    bank.add::<Transaction>();
    let mut engine = Engine::new();
    engine.export(bank).unwrap();
    engine
}

fn coffee() -> Transaction {
    Transaction { payee: "Blue Bottle".into(), amount: -4.5 }
}

fn failure<T>(result: Result<T, Box<rawr::EvalAltResult>>) -> String {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(error) => error.to_string(),
    }
}

/// A rules file of its own for each test, under the target directory.
fn rules(test: &str, text: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("script-{test}.rs"));
    std::fs::write(&path, text).unwrap();
    path
}

const CATEGORY: &str = "use bank::Transaction;

fn category(tx: Transaction) -> Option<String> {
    if tx.payee.contains(\"Bottle\") { Some(\"coffee\".into()) } else { None }
}

fn total(tx: &Transaction, fee: f64) -> f64 {
    tx.amount - fee
}
";

#[test]
fn an_exported_type_crosses_a_checked_handle() {
    let script = engine().script(CATEGORY).unwrap();
    let category = script.func::<(Transaction,), Option<String>>("category").unwrap();
    assert_eq!(category.call(coffee()).unwrap().as_deref(), Some("coffee"));
    // A `&Transaction` parameter borrows the handle's `Transaction`.
    let total = script.func::<(Transaction, f64), f64>("total").unwrap();
    assert_eq!(total.call(coffee(), 0.5).unwrap(), -5.0);
}

#[test]
fn a_handle_s_wrong_return_type_is_e0308_naming_the_exported_type() {
    let script = engine().script(CATEGORY).unwrap();
    assert_eq!(
        failure(script.func::<(Transaction,), String>("category")),
        "error[E0308]: mismatched types
 --> <script>:3:1-3:47
 3 | fn category(tx: Transaction) -> Option<String> {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `fn(Transaction) -> String`, found `fn(Transaction) -> Option<String>`"
    );
}

#[test]
fn load_reports_the_script_s_compile_error_at_its_path() {
    let path = rules("typo", "use bank::Transaction;\nfn category(tx: Transaction) -> Option<String> { Some(tx.payeee) }\n");
    let error = failure(engine().load(&path));
    assert!(error.starts_with("error[E0609]: no field `payeee` on type `Transaction`"), "{error}");
    assert!(error.contains(&format!(" --> {}:2:", path.display())), "{error}");
}

#[test]
fn reload_skips_unchanged_text_and_swaps_every_handle_on_an_edit() {
    let path = rules("edit", CATEGORY);
    let script: Script = engine().load(&path).unwrap();
    let category = script.func::<(Transaction,), Option<String>>("category").unwrap();
    let total = script.func::<(Transaction, f64), f64>("total").unwrap();
    assert!(!script.reload().unwrap());

    std::fs::write(&path, CATEGORY.replace("\"coffee\"", "\"cafe\"").replace("tx.amount - fee", "tx.amount - 2.0 * fee")).unwrap();
    assert!(script.reload().unwrap());
    assert_eq!(category.call(coffee()).unwrap().as_deref(), Some("cafe"));
    assert_eq!(total.call(coffee(), 0.5).unwrap(), -5.5);
    assert!(!script.reload().unwrap());
}

#[test]
fn a_failed_compile_keeps_the_old_program_running() {
    let path = rules("broken", CATEGORY);
    let script = engine().load(&path).unwrap();
    let category = script.func::<(Transaction,), Option<String>>("category").unwrap();

    std::fs::write(&path, CATEGORY.replace("tx.payee", "tx.payeee")).unwrap();
    let error = failure(script.reload());
    assert!(error.starts_with("error[E0609]: no field `payeee`"), "{error}");
    assert_eq!(category.call(coffee()).unwrap().as_deref(), Some("coffee"));

    // Fixing the file publishes it.
    std::fs::write(&path, CATEGORY.replace("\"coffee\"", "\"cafe\"")).unwrap();
    assert!(script.reload().unwrap());
    assert_eq!(category.call(coffee()).unwrap().as_deref(), Some("cafe"));
}

#[test]
fn a_reload_a_live_handle_no_longer_matches_is_refused_whole() {
    let path = rules("mismatch", CATEGORY);
    let script = engine().load(&path).unwrap();
    let category = script.func::<(Transaction,), Option<String>>("category").unwrap();
    let total = script.func::<(Transaction, f64), f64>("total").unwrap();

    // `total` changes too, but neither change is published.
    let edited = CATEGORY
        .replace("-> Option<String> {", "-> String {")
        .replace("Some(\"coffee\".into()) } else { None }", "\"coffee\".into() } else { String::new() }")
        .replace("tx.amount - fee", "tx.amount + fee");
    std::fs::write(&path, &edited).unwrap();
    let error = failure(script.reload());
    assert!(error.starts_with("error[E0308]: mismatched types") && error.contains("expected `fn(Transaction) -> Option<String>`, found `fn(Transaction) -> String`"), "{error}");
    assert_eq!(category.call(coffee()).unwrap().as_deref(), Some("coffee"));
    assert_eq!(total.call(coffee(), 0.5).unwrap(), -5.0);

    // Once the handle that wanted the old signature is gone, the edit is
    // published.
    drop(category);
    assert!(script.reload().unwrap());
    assert_eq!(total.call(coffee(), 0.5).unwrap(), -4.0);
    assert_eq!(script.func::<(Transaction,), String>("category").unwrap().call(coffee()).unwrap(), "coffee");
}

#[test]
fn a_reload_that_removes_a_handle_s_function_is_e0425() {
    let path = rules("removed", CATEGORY);
    let script = engine().load(&path).unwrap();
    let total = script.func::<(Transaction, f64), f64>("total").unwrap();
    std::fs::write(&path, CATEGORY.replace("fn total", "fn cost")).unwrap();
    let error = failure(script.reload());
    assert!(error.starts_with("error[E0425]: cannot find function `total` in this script"), "{error}");
    assert_eq!(total.call(coffee(), 0.5).unwrap(), -5.0);
}

mod ledger {
    #[derive(Clone)]
    #[rawr::export]
    pub struct Transaction {
        pub amount: f64,
    }
}

#[test]
fn two_roots_types_of_one_name_are_different_types() {
    let mut bank = Exports::new("bank");
    bank.add::<Transaction>();
    let mut ledger = Exports::new("ledger");
    ledger.add::<ledger::Transaction>();
    let mut engine = Engine::new();
    engine.export(bank).unwrap().export(ledger).unwrap();
    let script = engine.script("fn amount(tx: ledger::Transaction) -> f64 { tx.amount }").unwrap();

    assert_eq!(
        failure(script.func::<(Transaction,), f64>("amount")),
        "error[E0308]: mismatched types
 --> <script>:1:1-1:42
 1 | fn amount(tx: ledger::Transaction) -> f64 { tx.amount }
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `fn(bank::Transaction) -> f64`, found `fn(ledger::Transaction) -> f64`"
    );
    let amount = script.func::<(ledger::Transaction,), f64>("amount").unwrap();
    assert_eq!(amount.call(ledger::Transaction { amount: 2.5 }).unwrap(), 2.5);
}
