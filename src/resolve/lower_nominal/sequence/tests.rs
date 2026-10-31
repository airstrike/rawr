use crate::host::runtime::{type_ref::Type, Registry};
use crate::place::{sequence::Vector, text::Text};
use crate::{Dynamic, Engine, ImmutableString, Shared};
use std::any::TypeId;

fn run(source: &str, values: &mut Vec<String>) -> Result<Dynamic, String> {
    let engine = Shared::new(Engine::new());
    let authority = engine.compile_authority(source).unwrap();
    let runtime = Shared::new(Registry::new());
    let witness = runtime.witness_for(&Type::array(
        Type::builtin("String", TypeId::of::<ImmutableString>()),
        None,
    ));
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    crate::place::borrow::<Vector<Text>, _>(values, &witness, |value| {
        authority.invoke_bound(&run, &mut [value], runtime.clone())
    })
    .unwrap()
    .map_err(|error| error.to_string())
}

fn register_strings() {
    use rawr_import::classify::representability::convert::{Builtin, Type};
    let key = serde_json::to_vec(&Type::Sequence {
        element: Box::new(Type::Builtin(Builtin::String)),
        owned: true,
        length: None,
    })
    .unwrap();
    crate::place::sequence::register::<Text>(&key).unwrap();
    crate::place::sequence::register_capabilities::<Vec<String>>(
        Some(crate::place::sequence::cloning()),
        None,
        Some(crate::place::sequence::comparing()),
    );
    crate::place::reading::register::<Vec<String>>(crate::place::reading::Reading::from_traits(
        Some(crate::place::reading::debugging()),
        None,
        Some(crate::place::reading::partial()),
        Some(crate::place::reading::total()),
    ));
}

#[test]
fn native_nested_mutation_and_forwarding_survive_evaluation_error() {
    let mut string = String::with_capacity(32);
    string.push_str("before");
    let mut values = Vec::with_capacity(16);
    values.push(string);
    let vector = values.as_ptr();
    let text = values[0].as_ptr();
    let result = run(
        r#"
        fn change(values: &mut Vec<String>) {
            values[0].push_str(" after");
            values.push("second".to_string());
        }
        pub fn run(values: &mut Vec<String>) {
            change(values);
            panic!("after changes");
        }
    "#,
        &mut values,
    );
    assert!(result.is_err());
    assert_eq!(values, ["before after", "second"]);
    assert_eq!(values.as_ptr(), vector);
    assert_eq!(values[0].as_ptr(), text);
    assert_eq!(values.capacity(), 16);
    assert_eq!(values[0].capacity(), 32);
}

#[test]
fn native_constructor_and_whole_store_keep_allocation_history() {
    register_strings();
    let mut values = vec!["old".to_owned()];
    let _ = run(
        r#"
        pub fn run(values: &mut Vec<String>) {
            let mut replacement: Vec<String> = Vec::with_capacity(12);
            replacement.push("a".to_string());
            replacement.push("b".to_string());
            *values = replacement;
        }
    "#,
        &mut values,
    )
    .unwrap();
    assert_eq!(values, ["a", "b"]);
    assert_eq!(values.capacity(), 12);
    let _ = run(
        r#"
        pub fn run(values: &mut Vec<String>) {
            *values = vec!["one".to_string(), "two".to_string()];
        }
    "#,
        &mut values,
    )
    .unwrap();
    assert_eq!(values.capacity(), 2);
}

#[test]
fn native_take_replace_and_swap_preserve_owned_carriers() {
    register_strings();
    let mut values = Vec::with_capacity(19);
    values.push("original".to_owned());
    let allocation = values.as_ptr();
    let result = run(
        r#"
        pub fn run(values: &mut Vec<String>) {
            let mut moved = std::mem::take(values);
            let old = std::mem::replace(values, vec!["temporary".to_string()]);
            std::mem::swap(values, &mut moved);
        }
    "#,
        &mut values,
    )
    .unwrap();
    assert!(result.is_unit());
    assert_eq!(values, ["original"]);
    assert_eq!(values.as_ptr(), allocation);
    assert_eq!(values.capacity(), 19);
}

#[test]
fn native_resize_with_keeps_completed_items_on_error() {
    register_strings();
    let mut values = vec!["original".to_owned()];
    let result = run(
        r#"
        pub fn run(values: &mut Vec<String>) {
            let mut calls: usize = 0;
            values.resize_with(5, || {
                calls += 1;
                if calls == 3 { panic!("constructor failed"); }
                "made".to_string()
            });
        }
    "#,
        &mut values,
    );
    assert!(result.is_err());
    assert_eq!(values, ["original", "made", "made"]);
}

#[test]
fn native_clone_is_owned_and_nested_string_reads_are_live() {
    register_strings();
    let mut text = String::with_capacity(40);
    text.push_str("original");
    let mut values = Vec::with_capacity(11);
    values.push(text);
    let pointer = values[0].as_ptr();
    let result = run(
        r#"
        pub fn run(values: &mut Vec<String>) {
            let mut cloned = values.clone();
            cloned[0].push_str(" clone");
            assert_eq!(values[0].len(), 8);
            assert_eq!(cloned[0].len(), 14);
            assert_eq!(cloned.capacity(), 1);
            values[0].push_str(" kept");
        }
    "#,
        &mut values,
    )
    .unwrap();
    assert!(result.is_unit());
    assert_eq!(values, ["original kept"]);
    assert_eq!(values[0].as_ptr(), pointer);
}

struct Integer;

impl crate::place::Policy for Integer {
    type Value = i64;
    type Owner = crate::place::ownership::Native;

    fn accepts(witness: &crate::host::runtime::Witness) -> bool {
        witness.is_builtin("i64")
    }

    fn from_dynamic(value: Dynamic, _: &crate::host::runtime::Witness) -> crate::RawrResultOf<i64> {
        value
            .as_int()
            .map_err(|_| crate::place::failure("expected integer"))
    }

    fn into_dynamic(value: i64, _: &crate::host::runtime::Witness) -> crate::RawrResult {
        Ok(value.into())
    }

    fn copying() -> Option<&'static crate::place::sequence::Copying<i64>> {
        static COPY: crate::place::sequence::Copying<i64> =
            crate::place::sequence::copying::<i64>();
        Some(&COPY)
    }

    fn invoke(
        mut value: crate::place::Access<'_, i64>,
        _: &crate::host::runtime::Witness,
        operation: crate::place::Operation,
        arguments: &mut [Dynamic],
        _: Option<&mut crate::scope::Rest<'_>>,
    ) -> crate::RawrResult {
        match operation {
            crate::place::Operation::Copy => Ok((*value.read()).into()),
            crate::place::Operation::Store => {
                *value.write()? = arguments[0].as_int().unwrap();
                Ok(Dynamic::UNIT)
            }
            _ => Err(crate::place::failure("unexpected integer operation")),
        }
    }
}

#[test]
fn native_index_copy_and_compound_assignment_update_actual_element() {
    let engine = Shared::new(Engine::new());
    let authority = engine
        .compile_authority(
            r#"
        pub fn run(values: &mut Vec<i64>) -> i64 {
            let before = values[0];
            values[0] += 7;
            before + values[0]
        }
    "#,
        )
        .unwrap();
    let runtime = Shared::new(Registry::new());
    let witness = runtime.witness_for(&Type::array(
        Type::builtin("i64", TypeId::of::<i64>()),
        None,
    ));
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    let mut values = vec![5];
    let result = crate::place::borrow::<Vector<Integer>, _>(&mut values, &witness, |value| {
        authority.invoke_bound(&run, &mut [value], runtime.clone())
    })
    .unwrap()
    .unwrap();
    assert_eq!(result.as_int().unwrap(), 17);
    assert_eq!(values, [12]);
}

#[test]
fn native_append_moves_both_inputs_without_replacing_donor_allocation() {
    let engine = Shared::new(Engine::new());
    let authority = engine
        .compile_authority(
            r#"
        pub fn run(left: &mut Vec<String>, right: &mut Vec<String>) {
            left.append(right);
            right.push("after".to_string());
        }
    "#,
        )
        .unwrap();
    let runtime = Shared::new(Registry::new());
    let witness = runtime.witness_for(&Type::array(
        Type::builtin("String", TypeId::of::<ImmutableString>()),
        None,
    ));
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    let mut left = Vec::with_capacity(12);
    left.push("left".to_owned());
    let mut right = Vec::with_capacity(17);
    right.push("right".to_owned());
    let allocation = right.as_ptr();
    let _ = crate::place::borrow::<Vector<Text>, _>(&mut left, &witness, |left| {
        crate::place::borrow::<Vector<Text>, _>(&mut right, &witness, |right| {
            authority.invoke_bound(&run, &mut [left, right], runtime.clone())
        })
    })
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(left, ["left", "right"]);
    assert_eq!(right, ["after"]);
    assert_eq!(right.capacity(), 17);
    assert_eq!(right.as_ptr(), allocation);
}

#[test]
fn native_sorting_compares_live_elements_and_keeps_allocation() {
    register_strings();
    let mut values = Vec::with_capacity(23);
    values.extend(["bbb", "a", "cc"].map(str::to_owned));
    let pointer = values.as_ptr();
    let _ = run(
        r#"
        pub fn run(values: &mut Vec<String>) {
            values.sort_by_key(|value| value.len());
            assert_eq!(*values, vec!["a".to_string(), "cc".to_string(), "bbb".to_string()]);
            values.sort_unstable_by(|left, right| right.cmp(left));
            assert_eq!(values[0].len(), 2);
            values.sort();
            assert_eq!(values[0].len(), 1);
        }
    "#,
        &mut values,
    )
    .unwrap();
    assert_eq!(values, ["a", "bbb", "cc"]);
    assert_eq!(values.capacity(), 23);
    assert_eq!(values.as_ptr(), pointer);
}

fn run_two(
    source: &str,
    left: &mut Vec<String>,
    right: &mut Vec<String>,
) -> Result<Dynamic, String> {
    let engine = Shared::new(Engine::new());
    let authority = engine
        .compile_authority(source)
        .map_err(|error| error.to_string())?;
    let runtime = Shared::new(Registry::new());
    let witness = runtime.witness_for(&Type::array(
        Type::builtin("String", TypeId::of::<ImmutableString>()),
        None,
    ));
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    crate::place::borrow::<Vector<Text>, _>(left, &witness, |left| {
        crate::place::borrow::<Vector<Text>, _>(right, &witness, |right| {
            authority.invoke_bound(&run, &mut [left, right], runtime.clone())
        })
    })
    .unwrap()
    .unwrap()
    .map_err(|error| error.to_string())
}

#[test]
fn crossing_child_loans_end_at_their_own_last_use() {
    let mut a = vec!["a0".to_owned()];
    let mut b = vec!["b0".to_owned()];
    let result = run_two(
        r#"
        pub fn run(a: &mut Vec<String>, b: &mut Vec<String>) {
            let x = &mut a[0];
            let y = &mut b[0];
            x.push_str("-x");
            a.push("after-x".to_string());
            y.push_str("-y");
        }
    "#,
        &mut a,
        &mut b,
    );
    assert_eq!(result.map(|_| ()), Ok(()));
    assert_eq!(a, ["a0-x", "after-x"]);
    assert_eq!(b, ["b0-y"]);
}

#[test]
fn native_borrowed_extension_and_string_retain_keep_owners() {
    register_strings();
    let mut text = String::with_capacity(40);
    text.push_str("a-b-c");
    let mut values = Vec::with_capacity(9);
    values.push(text);
    let text_pointer = values[0].as_ptr();
    let _ = run(
        r#"
        pub fn run(values: &mut Vec<String>) {
            let source = vec!["extra".to_string()];
            values.extend_from_slice(&source);
            assert_eq!(source.len(), 1);
            assert!(values.contains(&"extra".to_string()));
            values[0].retain(|letter| letter != '-');
        }
    "#,
        &mut values,
    )
    .unwrap();
    assert_eq!(values, ["abc", "extra"]);
    assert_eq!(values.capacity(), 9);
    assert_eq!(values[0].as_ptr(), text_pointer);
}

#[test]
fn element_borrows_reach_the_native_element_without_a_copy() {
    let mut a = Vec::with_capacity(4);
    a.push(String::with_capacity(30));
    a.push("a1".to_owned());
    a[0].push_str("a0");
    let mut b = vec!["b0".to_owned()];
    let (text, vector) = (a[0].as_ptr(), a.as_ptr());
    let result = run_two(
        r#"
        fn first(values: &mut Vec<String>) -> &mut String { &mut values[0] }
        pub fn run(a: &mut Vec<String>, b: &mut Vec<String>) {
            let x = first(a);
            x.push_str("-helper");
            let last = a.last_mut().unwrap();
            last.push_str("-last");
            let copy = a[0].clone();
            b.push(copy);
            let x = &mut a[1];
            let y = &mut b[0];
            std::mem::swap(x, y);
            a.push("after".to_string());
        }
    "#,
        &mut a,
        &mut b,
    );
    assert_eq!(result.map(|_| ()), Ok(()));
    assert_eq!(a, ["a0-helper", "b0", "after"]);
    assert_eq!(b, ["a1-last", "a0-helper"]);
    assert_eq!(a[0].as_ptr(), text);
    assert_eq!(a.as_ptr(), vector);
}

#[test]
fn copy_elements_are_read_by_copy_and_written_in_place() {
    let engine = Shared::new(Engine::new());
    let authority = engine
        .compile_authority(
            r#"
        fn first(values: &mut Vec<i64>) -> &mut i64 { &mut values[0] }
        pub fn run(values: &mut Vec<i64>) -> i64 {
            let x = first(values);
            *x += 5;
            let read = values[0];
            *first(values) = read * 2;
            read
        }
    "#,
        )
        .unwrap();
    let runtime = Shared::new(Registry::new());
    let witness = runtime.witness_for(&Type::array(
        Type::builtin("i64", TypeId::of::<i64>()),
        None,
    ));
    let run = authority
        .items()
        .find(|item| item.key().name() == "run")
        .unwrap()
        .clone();
    let mut values = vec![1, 2];
    let result = crate::place::borrow::<Vector<Integer>, _>(&mut values, &witness, |value| {
        authority.invoke_bound(&run, &mut [value], runtime.clone())
    })
    .unwrap()
    .unwrap();
    assert_eq!(result.as_int(), Ok(6), "{result:?}");
    assert_eq!(values, [12, 2]);
}

#[test]
fn an_element_kept_past_its_callback_is_refused() {
    let witness = Shared::new(Registry::new()).witness_for(&Type::array(
        Type::builtin("String", TypeId::of::<ImmutableString>()),
        None,
    ));
    let mut values = vec!["kept".to_owned()];
    let mut kept = None;
    crate::place::borrow::<Vector<Text>, _>(&mut values, &witness, |value| {
        let place = crate::place::get(&value).unwrap();
        let element = place.project(0).unwrap();
        assert_eq!(
            element
                .invoke(crate::place::Operation::Len, &mut [])
                .unwrap()
                .as_int(),
            Ok(4)
        );
        assert!(place.project(1).is_err());
        kept = Some(element);
    })
    .unwrap();
    let kept = kept.unwrap();
    assert!(kept.invoke(crate::place::Operation::Len, &mut []).is_err());
    assert!(kept
        .invoke(
            crate::place::Operation::PushStr,
            &mut [Dynamic::from("late")]
        )
        .is_err());
    assert_eq!(values, ["kept"]);
}
