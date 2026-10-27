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
    assert_eq!(
        through,
        (33000, 0),
        "a copy stood in for a closure's borrow"
    );
    assert_eq!(named, (33000, 0));
}

/// A local a closure captured is shared, and a `&self` call on it, inside
/// a closure or after one, is lent the value under its lock: a thousand
/// reads copy no `Tally` (T178). Before, each read copied it.
#[test]
fn captured_locals_are_read_without_a_copy() {
    for (body, expected) in [
        (
            "let mut t = Tally::new(); let mut sum = 0;
            for n in 0..1000 { let mut add = |k| t.add(k); add(n); sum += t.total(); } sum",
            166666500,
        ),
        (
            "let mut t = Tally::new(); t.add(2); let show = || t.total();
            let mut sum = 0; for _ in 0..1000 { sum += show(); } sum",
            2000,
        ),
        (
            "let mut t = Tally::new(); { let mut add = |k| t.add(k); add(5); }
            let mut sum = 0; for _ in 0..1000 { sum += t.total(); } sum",
            5000,
        ),
    ] {
        assert_eq!(copies(body), (expected, 0), "{body}");
    }
}

/// Script structs holding 100 native values in each `Vec`, for
/// `struct_places`.
const STATE: &str = "struct Inner { items: Vec<Tally>, count: i64 }
    impl Inner {
        fn bump(&mut self, n: i64) { self.count += n; }
        fn total(&self) -> i64 { self.count }
        fn with(&self, state: &State) -> i64 { self.count + view(state) }
        fn absorb(&mut self, other: &mut Inner) { self.count += other.count; other.count = 0; }
    }
    struct State { inner: Inner, items: Vec<Tally>, count: i64 }
    impl State {
        fn show(&self) -> i64 { view(self) }
        fn sizes(&self) -> i64 { lens(&self.items) + lens(&self.inner.items) }
    }
    fn set(state: &mut State, n: i64) { state.count = n; }
    fn view(state: &State) -> i64 { state.count + state.items.len() as i64 + peek(state) }
    fn peek(state: &State) -> i64 { state.inner.count }
    fn two(a: &State, b: &Inner) -> i64 { a.count + b.count }
    fn both(a: &mut State, b: &mut Inner) { a.count += 1; b.count += 1; }
    fn pair(a: &mut Inner, b: &mut Inner) { a.count += b.count; b.count = 1; }
    fn lens(items: &Vec<Tally>) -> i64 { items.len() as i64 }
    fn filled() -> Vec<Tally> {
        let mut items = Vec::new();
        for _ in 0..100 { items.push(Tally::new()); }
        items
    }";

/// What `work`, run a thousand times on a `State` made of `STATE`'s
/// structs, adds to `sum`, and how many `Tally` values the run copied.
fn struct_places(work: &str) -> (i64, usize) {
    copies_with(
        STATE,
        &format!(
            "let mut s = State {{ inner: Inner {{ items: filled(), count: 0 }}, items: filled(), count: 0 }};
            let mut sum = 0;
            for n in 0..1000 {{ {work} }}
            sum"
        ),
    )
}

/// A field read, a field written directly or through `&mut`, a nested
/// field, a method called on a field and a field's element reach the
/// place itself: a thousand of each copy no `Tally` (T248, T143, T167).
/// Before, each field read copied the struct, so its 100 values, once,
/// and each write through `&mut` twice.
#[test]
fn struct_fields_are_reached_without_a_copy() {
    for (work, expected) in [
        ("sum += s.count; s.count = n;", 498501),
        ("set(&mut s, n); sum += s.count;", 499500),
        ("let r = &mut s; r.count = n; sum += r.count;", 499500),
        (
            "s.inner.count = n; sum += s.inner.count + s.inner.items.len() as i64;",
            599500,
        ),
        ("s.inner.bump(1); sum += s.inner.total();", 500500),
        ("s.items[0].add(n); sum += s.items[0].total();", 166666500),
        (
            "s.items.push(Tally::new()); sum += s.items.len() as i64;",
            600500,
        ),
    ] {
        assert_eq!(struct_places(work), (expected, 0), "{work}");
    }
    // The same reads, writes and borrows of locals copy nothing either.
    let named = copies_with(
        STATE,
        "let items = filled();
        let mut count = 0;
        let mut sum = 0;
        for n in 0..1000 {
            sum += count;
            count = n;
            sum += count + items.len() as i64 + lens(&items);
        }
        sum",
    );
    assert_eq!(named, (498501 + 499500 + 200000, 0));
}

/// A struct or a field of one passed by `&` to a function, passed on from
/// there, passed as `self`, to a method, or beside another, is lent: a
/// thousand calls copy no `Tally` (T248). Before, each call copied the
/// struct, and each field read inside it copied it again, three copies a
/// call, and `&s.items` copied the field.
#[test]
fn structs_passed_by_reference_are_lent() {
    for (work, expected) in [
        ("s.count = n; sum += view(&s);", 599500),
        ("s.count = n; sum += s.show();", 599500),
        (
            "let o = Inner { items: filled(), count: n }; sum += two(&s, &o);",
            499500,
        ),
        (
            "s.count = n; let o = Inner { items: filled(), count: 1 }; sum += o.with(&s);",
            600500,
        ),
        ("s.inner.count = n; sum += two(&s, &s.inner);", 499500),
        (
            "sum += lens(&s.items) + lens(&s.inner.items) + s.sizes();",
            400000,
        ),
        (
            "let r = &s; sum += r.count + r.items.len() as i64 + view(r);",
            200000,
        ),
        (
            "let r = &s.inner; sum += r.count + lens(&r.items) + r.total();",
            100000,
        ),
    ] {
        assert_eq!(struct_places(work), (expected, 0), "{work}");
    }
}

/// A local, or a field of one, passed to a `&mut` parameter after the
/// first moves in and back out of the call: a thousand calls copy no
/// `Tally` (T248). Before, each copied the value in, out of the callee
/// and back into the place, three copies a call.
#[test]
fn locals_passed_by_mut_reference_move_in_and_out() {
    for (work, expected) in [
        (
            "let mut o = Inner { items: filled(), count: n }; both(&mut s, &mut o);
            pair(&mut o, &mut s.inner); sum += o.count + s.inner.count;",
            502499,
        ),
        (
            "let mut o = Inner { items: filled(), count: 0 }; s.inner.count = n;
            o.absorb(&mut s.inner); sum += o.count + s.inner.count;",
            499500,
        ),
    ] {
        assert_eq!(struct_places(work), (expected, 0), "{work}");
    }
}
