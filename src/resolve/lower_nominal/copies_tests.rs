//! A native type that counts its own copies (D568): any copy of a `Tally`
//! a run makes is one standing in for a borrow, since Rust code has no
//! `Clone` of it to call.

use std::cell::Cell;

thread_local! {
    static COPIES: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug)]
struct Tally(i64);

impl Clone for Tally {
    fn clone(&self) -> Self {
        COPIES.with(|copies| copies.set(copies.get() + 1));
        Self(self.0)
    }
}

fn facade() -> crate::host::facade::Definition {
    use crate::host::{facade, item, signature, type_ref};
    let mut builder = facade::Builder::new("tally", facade::Abi::new(1).unwrap()).unwrap();
    builder
        .type_::<Tally>("Tally", &[], &[crate::capability::Kind::Debug])
        .unwrap();
    let tally = || type_ref::Type::registered("Tally").unwrap();
    let int = || type_ref::Type::builtin(type_ref::Builtin::I64);
    let mut new = signature::Builder::new();
    new.output(tally());
    builder
        .item(item::Definition::associated_function("Tally::new", new.finish()).unwrap())
        .unwrap();
    let mut add = signature::Builder::new();
    add.parameter(int()).output(type_ref::Type::unit());
    builder
        .item(
            item::Definition::method("Tally::add", crate::receiver::Mode::Mutable, add.finish())
                .unwrap(),
        )
        .unwrap();
    let mut total = signature::Builder::new();
    total.output(int());
    builder
        .item(
            item::Definition::method(
                "Tally::total",
                crate::receiver::Mode::Shared,
                total.finish(),
            )
            .unwrap(),
        )
        .unwrap();
    builder.finish().unwrap()
}

fn run(source: &str) -> i64 {
    crate::host::debug::register::<Tally>();
    let mut engine = crate::Engine::new();
    engine.register_host_facade(facade()).unwrap();
    let engine = crate::Shared::new(engine);
    let authority = engine
        .compile_authority(source)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut runtime = crate::host::runtime::Builder::new(facade());
    runtime
        .bind("tally::Tally::new", |_| Ok(crate::Dynamic::from(Tally(0))))
        .unwrap();
    runtime
        .bind("tally::Tally::add", |args| {
            let n = args[1].as_int().expect("an i64");
            args[0].write_lock::<Tally>().expect("a tally").0 += n;
            Ok(crate::Dynamic::UNIT)
        })
        .unwrap();
    runtime
        .bind("tally::Tally::total", |args| {
            Ok(crate::Dynamic::from_int(
                args[0].read_lock::<Tally>().expect("a tally").0,
            ))
        })
        .unwrap();
    let mut registry = crate::host::runtime::Registry::new();
    registry.install(runtime.finish()).unwrap();
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    authority
        .invoke_bound(&run, &mut [], crate::Shared::new(registry))
        .unwrap_or_else(|error| panic!("{error}"))
        .as_int()
        .unwrap()
}

/// What `run` with `body` returns, and how many times the run copied a
/// `Tally`.
pub(super) fn copies(body: &str) -> (i64, usize) {
    copies_with("", body)
}

/// `copies`, with `items` (functions `body` calls) beside `run`.
pub(super) fn copies_with(items: &str, body: &str) -> (i64, usize) {
    COPIES.with(|copies| copies.set(0));
    let value = run(&format!(
        "use tally::Tally;\n{items}\npub fn run() -> i64 {{\n{body}\n}}"
    ));
    (value, COPIES.with(Cell::get))
}

/// A closure using a `&mut` local, a function's `&mut` parameter, a `&mut`
/// pattern binding or a mutable loop's element reaches the place itself:
/// a thousand calls through each copy no `Tally`, as the same calls made
/// on the places by name copy none (T144, T164). Before, these closures
/// were refused, and before D585 a pattern binding's closure copied.
#[test]
fn closures_reach_places_without_a_copy() {
    let items = "fn push_one(all: &mut Vec<Tally>) {
        let mut push = || all.push(Tally::new());
        push();
    }
    fn add_one(t: &mut Tally, n: i64) {
        let mut add = |k| t.add(k);
        add(n);
    }";
    let through = copies_with(
        items,
        "let mut all = vec![Tally::new()];
        let mut first = Tally::new();
        let mut slot = Some(Tally::new());
        let mut pair = vec![Tally::new(), Tally::new()];
        for n in 0..1000 {
            push_one(&mut all);
            add_one(&mut first, n);
            {
                let r = &mut all;
                let mut push = || r.push(Tally::new());
                push();
            }
            if let Some(t) = &mut slot {
                let mut add = |k| t.add(k);
                add(n);
            }
            if let Some(t) = pair.get_mut(1) {
                let mut add = || t.add(1);
                add();
            }
            for t in &mut pair {
                let mut add = || t.add(1);
                add();
            }
        }
        let second = match &mut slot {
            Some(t) => t.total(),
            None => 0,
        };
        let last = match pair.get_mut(1) {
            Some(t) => t.total(),
            None => 0,
        };
        all.len() as i64 * 1000000000000 + first.total() * 1000 + second + last",
    );
    let named = copies(
        "let mut all = vec![Tally::new()];
        let mut first = Tally::new();
        let mut slot = Tally::new();
        let mut last = Tally::new();
        for n in 0..1000 {
            all.push(Tally::new());
            first.add(n);
            all.push(Tally::new());
            slot.add(n);
            last.add(1);
            last.add(1);
        }
        all.len() as i64 * 1000000000000 + first.total() * 1000 + slot.total() + last.total()",
    );
    let expected = (2001 * 1000000000000 + 499500 * 1000 + 499500 + 2000, 0);
    assert_eq!(through, expected, "a copy stood in for a closure's borrow");
    assert_eq!(named, expected);
}

/// A native call's output, a `&mut self` and a `&self` call on a local
/// and a `format!` argument reach the value where it is: a hundred of each
/// copy nothing (T126). Before, the output was copied to check it, each
/// call copied its receiver twice, and each argument was copied twice.
#[test]
fn native_calls_reach_their_values_without_a_copy() {
    for (body, expected) in [
        ("let a = Tally::new(); 0", 0),
        (
            "let mut a = Tally::new(); for n in 0..100 { a.add(n); } a.total()",
            4950,
        ),
        (
            "let mut a = Tally::new(); a.add(2); let b = Tally::new(); let mut sum = 0;
            for _ in 0..100 { sum += a.total() + b.total(); } sum",
            200,
        ),
        (
            "let mut a = Tally::new(); a.add(12); let mut len = 0;
            for _ in 0..100 { len += format!(\"{a:?} {:?}\", a).len(); } len as i64",
            1900,
        ),
    ] {
        assert_eq!(copies(body), (expected, 0), "{body}");
    }
}

/// A closure passed as a function's `&mut impl FnMut` argument, named or
/// written in place, is called where the caller has it: a thousand calls
/// through each copy no `Tally` beyond what the same calls by name copy
/// (T175). Before, the call didn't find the parameter, and a closure
/// written in place was refused.
#[test]
fn closures_passed_by_mut_reference_copy_nothing() {
    let items = "fn twice(f: &mut impl FnMut(i64)) {
        f(1);
        f(2);
    }";
    let through = copies_with(
        items,
        "let mut t = Tally::new();
        let mut u = Tally::new();
        let mut add = |k| t.add(k);
        for _ in 0..1000 {
            twice(&mut add);
            twice(&mut |k| u.add(k * 10));
        }
        t.total() + u.total()",
    );
    let named = copies(
        "let mut t = Tally::new();
        let mut u = Tally::new();
        let mut add = |k| t.add(k);
        let mut add_ten = |k| u.add(k * 10);
        for _ in 0..1000 {
            add(1);
            add(2);
            add_ten(1);
            add_ten(2);
        }
        t.total() + u.total()",
    );
    assert_eq!(through.0, 33000);
    assert_eq!(through, named, "a copy stood in for a closure's borrow");
}
