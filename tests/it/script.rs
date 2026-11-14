//! `Script` and its typed handles (D514): an app's exported types cross a
//! checked handle, and `Script::reload` swaps every handle to a new program
//! or keeps the old one.

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
 --> <script>:3:1
  |
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
    // The same broken text gives the same error again.
    assert_eq!(failure(script.reload()), error);

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
 --> <script>:1:1
  |
1 | fn amount(tx: ledger::Transaction) -> f64 { tx.amount }
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `fn(bank::Transaction) -> f64`, found `fn(ledger::Transaction) -> f64`"
    );
    let amount = script.func::<(ledger::Transaction,), f64>("amount").unwrap();
    assert_eq!(amount.call(ledger::Transaction { amount: 2.5 }).unwrap(), 2.5);
}

#[test]
fn an_inline_module_compiles_in_a_script() {
    let script = Engine::new().script("mod m { pub fn f() -> i64 { 1 } }\nfn g() -> i64 { m::f() }").unwrap();
    assert_eq!(script.func::<(), i64>("g").unwrap().call().unwrap(), 1);
}

const GAME: &str = "mod game {
    pub mod rules {
        pub fn score(hits: i64) -> i64 {
            super::bonus() + hits * 10
        }
    }

    fn bonus() -> i64 {
        5
    }

    pub struct Board {
        pub hits: i64,
    }

    impl Board {
        pub fn score(&self) -> i64 {
            rules::score(self.hits)
        }
    }
}

use game::rules;
use game::Board;

fn score(hits: i64) -> i64 {
    rules::score(hits) + Board { hits }.score()
}
";

#[test]
fn inline_modules_resolve_paths_use_and_super_as_rustc_does() {
    let script = Engine::new().script(GAME).unwrap();
    assert_eq!(script.func::<(i64,), i64>("score").unwrap().call(2).unwrap(), 50);

    let engine = Engine::new();
    let ast = engine.compile(GAME).unwrap();
    let score: i64 = engine.call_fn(&mut rawr::Scope::new(), &ast, "score", (3_i64,)).unwrap();
    assert_eq!(score, 70);
}

#[test]
fn a_private_item_of_an_inline_module_is_e0603() {
    let error = failure(Engine::new().script("mod m { fn f() -> i64 { 1 } }\nfn g() -> i64 { m::f() }"));
    assert!(error.starts_with("error[E0603]: function `f` is private"), "{error}");
}

#[test]
fn a_file_module_in_a_script_needs_a_package() {
    let error = failure(Engine::new().script("mod m;\nfn g() -> i64 { 1 }"));
    assert!(error.contains("file modules require project/package compilation"), "{error}");
    let error = failure(Engine::new().script("mod m { mod n; }\nfn g() -> i64 { 1 }"));
    assert!(error.contains("file modules require project/package compilation"), "{error}");
}

const UPDATE: &str = "use bank::Transaction;

fn update(tx: &mut Transaction, fee: f64) -> f64 {
    tx.amount -= fee;
    tx.amount
}

fn settle(label: String, tx: &mut Transaction) -> String {
    tx.payee = format!(\"{label} {}\", tx.payee);
    tx.payee.clone()
}
";

#[test]
fn call_fn_passes_a_leading_mut_parameter_as_written() {
    let engine = engine();
    let ast = engine.compile(UPDATE).unwrap();
    let mut scope = rawr::Scope::new();
    let amount: f64 = engine.call_fn(&mut scope, &ast, "update", (coffee(), 0.5)).unwrap();
    assert_eq!(amount, -5.0);
    // A `&mut` parameter after another one.
    let payee: String = engine.call_fn(&mut scope, &ast, "settle", ("paid".to_string(), coffee())).unwrap();
    assert_eq!(payee, "paid Blue Bottle");
    // The arity the function is written with is the one a call names.
    let error = engine.call_fn::<f64>(&mut scope, &ast, "update", (0.5,)).unwrap_err();
    assert!(matches!(*error, rawr::EvalAltResult::ErrorFunctionNotFound(..)), "{error}");
}

#[test]
fn bind_this_ptr_still_lends_a_leading_mut_parameter() {
    let engine = engine();
    let ast = engine.compile(UPDATE).unwrap();
    let mut tx = rawr::Dynamic::from(coffee());
    let options = rawr::CallFnOptions::new().bind_this_ptr(&mut tx);
    let amount = engine.call_fn_with_options::<f64>(options, &mut rawr::Scope::new(), &ast, "update", (0.5,)).unwrap();
    assert_eq!(amount, -5.0);
    assert_eq!(tx.cast::<Transaction>().amount, -5.0);
}

#[test]
fn a_typed_handle_lends_a_mut_parameter_the_handle_s_value() {
    let script = engine().script(UPDATE).unwrap();
    let update = script.func::<(Transaction, f64), f64>("update").unwrap();
    assert_eq!(update.call(coffee(), 0.5).unwrap(), -5.0);
    let settle = script.func::<(String, Transaction), String>("settle").unwrap();
    assert_eq!(settle.call("paid".into(), coffee()).unwrap(), "paid Blue Bottle");
}

/// The embedding guide's examples of modules and of `call_fn`.
#[test]
fn the_guide_s_module_and_call_fn_examples_print_what_it_says() {
    let script = Engine::new()
        .script(
            "mod game {
    pub mod rules {
        pub fn score(hits: i64) -> i64 {
            super::BONUS + hits * 10
        }
    }

    const BONUS: i64 = 5;
}

use game::rules;

fn score(hits: i64) -> i64 {
    rules::score(hits)
}",
        )
        .unwrap();
    assert_eq!(script.func::<(i64,), i64>("score").unwrap().call(2).unwrap(), 25);

    let engine = Engine::new();
    let ast = engine.compile("fn update(score: &mut i64, hits: i64) -> i64 {\n    *score += hits * 10;\n    *score\n}").unwrap();
    let mut scope = rawr::Scope::new();
    assert_eq!(engine.call_fn::<i64>(&mut scope, &ast, "update", (5_i64, 2_i64)).unwrap(), 25);
    let mut score = rawr::Dynamic::from(5_i64);
    let options = rawr::CallFnOptions::new().bind_this_ptr(&mut score);
    engine.call_fn_with_options::<i64>(options, &mut scope, &ast, "update", (2_i64,)).unwrap();
    assert_eq!(score.as_int().unwrap(), 25);
}
