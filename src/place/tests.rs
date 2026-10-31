use super::{get, lend, owned, sequence::Vector, text::Text, Access, Operation, Policy};
use crate::host::runtime::type_ref::Type;
use crate::host::runtime::{Registry, Witness};
use crate::{Dynamic, RawrResult, RawrResultOf};
use std::any::TypeId;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn strings() -> Witness {
    Registry::new().witness_for(&Type::array(
        Type::builtin("String", TypeId::of::<crate::ImmutableString>()),
        None,
    ))
}

fn text(value: &str, capacity: usize) -> String {
    let mut text = String::with_capacity(capacity);
    text.push_str(value);
    text
}

#[test]
fn native_vector_and_element_writes_survive_a_caught_panic() {
    let mut values = Vec::with_capacity(8);
    values.push(text("before", 32));
    let (vector, string) = (values.as_ptr(), values[0].as_ptr());
    let witness = strings();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        lend::<Vector<Text>>(&mut values, &witness, &mut |value| {
            let place = get(&value).unwrap();
            let _ = place.with(
                Operation::Element { mutable: true },
                &mut [Dynamic::from_int(0)],
                &mut |child| {
                    let _ = get(&child)
                        .unwrap()
                        .invoke(Operation::PushStr, &mut [Dynamic::from(" after")])?;
                    Ok(Dynamic::UNIT)
                },
            )?;
            let _ = place.invoke(Operation::Push, &mut [Dynamic::from("next")])?;
            panic!("after completed writes");
        })
    }));
    assert!(caught.is_err());
    assert_eq!(values, ["before after", "next"]);
    assert_eq!(values.as_ptr(), vector);
    assert_eq!(values.capacity(), 8);
    assert_eq!(values[0].as_ptr(), string);
    assert_eq!(values[0].capacity(), 32);
}

#[test]
fn an_owned_value_recovers_after_callback_unwind_without_losing_mutations() {
    let mut values = Vec::with_capacity(8);
    values.push(text("before", 32));
    let (vector, string) = (values.as_ptr(), values[0].as_ptr());
    let witness = strings();
    let value = owned::<Vector<Text>>(values, &witness).unwrap();
    let place = get(&value).unwrap();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        place.with(
            Operation::Element { mutable: true },
            &mut [Dynamic::from_int(0)],
            &mut |child| {
                let _ = get(&child)
                    .unwrap()
                    .invoke(Operation::PushStr, &mut [Dynamic::from(" after")])?;
                panic!("inside owned native access");
            },
        )
    }));
    assert!(caught.is_err());
    assert_eq!(
        place.invoke(Operation::Len, &mut []).unwrap().as_int(),
        Ok(1)
    );
    let _ = place
        .invoke(Operation::Push, &mut [Dynamic::from("next")])
        .unwrap();
    let values = super::take::<Vector<Text>>(&place, &witness).unwrap();
    assert_eq!(values, ["before after", "next"]);
    assert_eq!(values.as_ptr(), vector);
    assert_eq!(values[0].as_ptr(), string);
    assert!(place.invoke(Operation::Len, &mut []).is_err());
}

#[test]
fn owned_shared_reentry_is_compatible_but_mutable_reentry_is_not() {
    let witness = strings();
    let value = owned::<Vector<Text>>(vec![text("value", 32)], &witness).unwrap();
    let place = get(&value).unwrap();
    let alias = place.clone();
    let mut stale = None;
    let _ = place
        .with(
            Operation::Element { mutable: false },
            &mut [Dynamic::from_int(0)],
            &mut |child| {
                stale = get(&child);
                assert_eq!(alias.invoke(Operation::Len, &mut [])?.as_int(), Ok(1));
                let _ = alias.with(
                    Operation::Element { mutable: false },
                    &mut [Dynamic::from_int(0)],
                    &mut |second| {
                        assert_eq!(
                            get(&second)
                                .unwrap()
                                .invoke(Operation::Capacity, &mut [])?
                                .as_int(),
                            Ok(32)
                        );
                        Ok(Dynamic::UNIT)
                    },
                )?;
                assert!(alias
                    .invoke(Operation::Push, &mut [Dynamic::from("invalid")])
                    .is_err());
                Ok(Dynamic::UNIT)
            },
        )
        .unwrap();
    assert!(stale.unwrap().invoke(Operation::Len, &mut []).is_err());
    // The child's last use ended its actual parent loan.
    let _ = place
        .invoke(Operation::Push, &mut [Dynamic::from("valid")])
        .unwrap();
}

#[test]
fn take_and_assignment_move_the_original_native_allocation() {
    let mut values = Vec::with_capacity(16);
    values.push(text("value", 32));
    let allocation = values.as_ptr();
    let witness = strings();
    let _ = lend::<Vector<Text>>(&mut values, &witness, &mut |value| {
        let place = get(&value).unwrap();
        let moved = place.invoke(Operation::Take, &mut [])?;
        assert_eq!(place.invoke(Operation::Capacity, &mut [])?.as_int(), Ok(0));
        place.invoke(Operation::Store, &mut [moved])
    })
    .unwrap();
    assert_eq!(values.as_ptr(), allocation);
    assert_eq!(values.capacity(), 16);
    assert_eq!(values, ["value"]);
}

#[test]
fn append_moves_elements_and_preserves_the_donors_allocation() {
    let mut recipient = Vec::with_capacity(8);
    recipient.push(text("first", 32));
    let mut donor = Vec::with_capacity(4);
    donor.push(text("second", 32));
    let (recipient_at, donor_at, string_at) =
        (recipient.as_ptr(), donor.as_ptr(), donor[0].as_ptr());
    let witness = strings();
    let _ = lend::<Vector<Text>>(&mut recipient, &witness, &mut |recipient| {
        lend::<Vector<Text>>(&mut donor, &witness, &mut |donor| {
            get(&recipient)
                .unwrap()
                .invoke(Operation::Append, &mut [donor])
        })
    })
    .unwrap();
    assert_eq!(recipient, ["first", "second"]);
    assert!(donor.is_empty());
    assert_eq!(recipient.as_ptr(), recipient_at);
    assert_eq!(donor.as_ptr(), donor_at);
    assert_eq!(donor.capacity(), 4);
    assert_eq!(recipient[1].as_ptr(), string_at);
}

struct Tracked(Arc<AtomicUsize>);

impl Drop for Tracked {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Element;

impl Policy for Element {
    type Value = Tracked;
    type Owner = super::ownership::Native;

    fn accepts(witness: &Witness) -> bool {
        witness.is_builtin("Tracked")
    }

    fn from_dynamic(value: Dynamic, witness: &Witness) -> RawrResultOf<Tracked> {
        super::take::<Self>(&get(&value).unwrap(), witness)
    }

    fn into_dynamic(value: Tracked, witness: &Witness) -> RawrResult {
        owned::<Self>(value, witness)
    }

    fn invoke(
        _: Access<'_, Tracked>,
        _: &Witness,
        _: Operation,
        _: &mut [Dynamic],
        _: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult {
        Err(super::failure("Tracked has no copied operations"))
    }
}

#[test]
fn non_clone_elements_are_lent_moved_and_dropped_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut values = Vec::with_capacity(8);
    values.push(Tracked(drops.clone()));
    let witness = Registry::new().witness_for(&Type::array(
        Type::builtin("Tracked", TypeId::of::<Tracked>()),
        None,
    ));
    let _ = lend::<Vector<Element>>(&mut values, &witness, &mut |value| {
        // No Clone implementation exists, including during entry/exit.
        let alias = get(&value).unwrap().clone();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let mut moved = alias.invoke(Operation::Pop, &mut [])?;
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let moved = crate::option::take(&mut moved).unwrap().unwrap();
        let second_handle = moved.clone();
        drop(moved);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(second_handle);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        Ok(Dynamic::UNIT)
    })
    .unwrap();
    assert!(values.is_empty());
    assert_eq!(values.capacity(), 8);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn expired_roots_and_wrong_witnesses_cannot_access_native_storage() {
    let mut values = vec![text("value", 16)];
    let witness = strings();
    let mut stale = None;
    let _ = lend::<Vector<Text>>(&mut values, &witness, &mut |value| {
        stale = get(&value);
        Ok(Dynamic::UNIT)
    })
    .unwrap();
    assert!(stale.unwrap().invoke(Operation::Len, &mut []).is_err());
    let wrong = Registry::new().witness_for(&Type::array(
        Type::builtin("bool", TypeId::of::<bool>()),
        None,
    ));
    let mut reached = false;
    assert!(lend::<Vector<Text>>(&mut values, &wrong, &mut |_| {
        reached = true;
        Ok(Dynamic::UNIT)
    })
    .is_err());
    assert!(!reached);
    assert_eq!(values, ["value"]);
}

#[test]
fn shared_claim_cleanup_survives_unwind_and_does_not_cross_threads() {
    let witness = strings();
    let value = owned::<Vector<Text>>(vec![text("value", 32)], &witness).unwrap();
    let place = get(&value).unwrap();
    let alias = place.clone();
    let caught = catch_unwind(AssertUnwindSafe(|| {
        place.with(
            Operation::Element { mutable: false },
            &mut [Dynamic::from_int(0)],
            &mut |_| {
                // Without `sync`, a handle is not Send at all; with it, another
                // thread still cannot find this thread's shared claim.
                #[cfg(feature = "sync")]
                assert!(std::thread::spawn({
                    let alias = alias.clone();
                    move || alias.invoke(Operation::Len, &mut []).is_err()
                })
                .join()
                .unwrap());
                assert_eq!(alias.invoke(Operation::Len, &mut [])?.as_int(), Ok(1));
                panic!("inside shared native access");
            },
        )
    }));
    assert!(caught.is_err());
    let _ = place
        .invoke(Operation::Push, &mut [Dynamic::from("after")])
        .unwrap();
    assert_eq!(
        place.invoke(Operation::Len, &mut []).unwrap().as_int(),
        Ok(2)
    );
}

#[test]
fn callback_errors_run_native_partial_progress_cleanup() {
    let mut values = vec!["original".to_owned()];
    let mut calls = 0;
    let result = super::callback::run(|abort| {
        values.resize_with(5, || {
            calls += 1;
            abort.value(if calls == 3 {
                Err(super::failure("constructor failed"))
            } else {
                Ok(format!("made {calls}"))
            })
        });
    });
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("constructor failed"));
    assert_eq!(calls, 3);
    assert_eq!(values, ["original", "made 1", "made 2"]);
    values.push("after error".to_owned());
    assert_eq!(values.len(), 4);

    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _: RawrResultOf<()> = super::callback::run(|_| {
            std::panic::resume_unwind(Box::new(73_usize));
        });
    }))
    .unwrap_err();
    assert_eq!(*panic.downcast::<usize>().unwrap(), 73);
}

#[test]
fn typed_construction_retains_its_native_allocation_through_assignment() {
    let key = br#"{"Sequence":{"element":{"Builtin":"String"},"owned":true}}"#;
    super::sequence::register::<Text>(key).unwrap();
    let witness = strings();
    let incoming = super::sequence::construct(key, &witness, 2)
        .unwrap()
        .unwrap();
    let incoming_place = get(&incoming).unwrap();
    for text in ["one", "two"] {
        let _ = incoming_place
            .invoke(Operation::Push, &mut [Dynamic::from(text)])
            .unwrap();
    }
    let pointer = super::reach::<Vector<Text>, _>(&incoming_place, &witness, false, |value| {
        Ok(value.read().as_ptr() as usize)
    })
    .unwrap();
    let mut values = Vec::with_capacity(16);
    let _ = lend::<Vector<Text>>(&mut values, &witness, &mut |value| {
        get(&value)
            .unwrap()
            .invoke(Operation::Store, &mut [incoming.clone()])
    })
    .unwrap();
    assert_eq!(values, ["one", "two"]);
    assert_eq!(values.capacity(), 2);
    assert_eq!(values.as_ptr() as usize, pointer);
    assert!(incoming_place.invoke(Operation::Len, &mut []).is_err());
    assert!(
        super::sequence::construct(b"no registered concrete type", &witness, 2)
            .unwrap()
            .is_none()
    );
}

#[test]
fn drain_and_dedup_use_native_storage_and_move_the_removed_strings() {
    let witness = strings();
    let mut values = Vec::with_capacity(12);
    values.extend([text("same", 32), text("same", 32), text("last", 32)]);
    let vector = values.as_ptr();
    let last = values[2].as_ptr();
    let mut removed = None;
    let _ = lend::<Vector<Text>>(&mut values, &witness, &mut |value| {
        let place = get(&value).unwrap();
        let _ = place.invoke(Operation::Dedup, &mut [])?;
        let tail = place.invoke(
            Operation::Drain,
            &mut [Dynamic::from_int(1), Dynamic::from_int(-1), Dynamic::FALSE],
        )?;
        removed = Some(super::take::<Vector<Text>>(&get(&tail).unwrap(), &witness)?);
        Ok(Dynamic::UNIT)
    })
    .unwrap();
    assert_eq!(values, ["same"]);
    assert_eq!(values.capacity(), 12);
    assert_eq!(values.as_ptr(), vector);
    let removed = removed.unwrap();
    assert_eq!(removed, ["last"]);
    assert_eq!(removed[0].as_ptr(), last);
}

#[test]
fn swapping_whole_referents_exchanges_the_original_allocations() {
    let mut left = Vec::with_capacity(8);
    left.push(text("left", 32));
    let mut right = Vec::with_capacity(16);
    right.push(text("right", 64));
    let (left_ptr, right_ptr) = (left.as_ptr(), right.as_ptr());
    let witness = strings();
    let _ = lend::<Vector<Text>>(&mut left, &witness, &mut |left| {
        lend::<Vector<Text>>(&mut right, &witness, &mut |right| {
            get(&left)
                .unwrap()
                .invoke(Operation::SwapValues, &mut [right])
        })
    })
    .unwrap();
    assert_eq!((left.capacity(), right.capacity()), (16, 8));
    assert_eq!((left.as_ptr(), right.as_ptr()), (right_ptr, left_ptr));
    assert_eq!((left[0].as_str(), right[0].as_str()), ("right", "left"));
}

#[test]
fn retained_native_ownership_accepts_the_next_generation_but_not_another_runner() {
    let registry = Registry::new();
    let ty = Type::array(
        Type::builtin("String", TypeId::of::<crate::ImmutableString>()),
        None,
    );
    let original = registry.witness_for(&ty);
    let next = registry.next_generation().witness_for(&ty);
    let foreign = Registry::new().witness_for(&ty);
    let value = owned::<Vector<Text>>(vec![text("retained", 32)], &original).unwrap();
    let place = get(&value).unwrap();
    assert!(super::take::<Vector<Text>>(&place, &foreign).is_err());
    let moved = super::take::<Vector<Text>>(&place, &next).unwrap();
    assert_eq!(moved, ["retained"]);
    assert_eq!(moved[0].capacity(), 32);
}

#[test]
fn native_slice_children_keep_the_parent_claim_and_expire_after_their_continuation() {
    let witness = strings();
    let mut values = Vec::with_capacity(8);
    values.extend([text("first", 32), text("second", 32), text("third", 32)]);
    let (vector, second) = (values.as_ptr(), values[1].as_ptr());
    let mut stale = None;
    let _ = lend::<Vector<Text>>(&mut values, &witness, &mut |value| {
        let root = get(&value).unwrap();
        let _ = root.with(
            Operation::Slice { mutable: true },
            &mut [Dynamic::from_int(0), Dynamic::from_int(2), false.into()],
            &mut |slice| {
                let slice = get(&slice).unwrap();
                stale = Some(slice.clone());
                assert_eq!(slice.invoke(Operation::Len, &mut [])?.as_int(), Ok(2));
                assert!(slice
                    .invoke(Operation::Push, &mut ["invalid".into()])
                    .is_err());
                assert!(root
                    .invoke(Operation::Push, &mut ["invalid".into()])
                    .is_err());
                slice.with(
                    Operation::Element { mutable: true },
                    &mut [Dynamic::from_int(1)],
                    &mut |child| {
                        get(&child)
                            .unwrap()
                            .invoke(Operation::PushStr, &mut [" changed".into()])
                    },
                )
            },
        )?;
        assert!(stale
            .as_ref()
            .unwrap()
            .invoke(Operation::Len, &mut [])
            .is_err());
        root.invoke(Operation::Push, &mut ["after".into()])
    })
    .unwrap();
    assert_eq!(values, ["first", "second changed", "third", "after"]);
    assert_eq!(values.as_ptr(), vector);
    assert_eq!(values[1].as_ptr(), second);
    assert_eq!(values.capacity(), 8);
}

#[test]
fn native_slice_reads_compare_real_elements_without_moving_their_storage() {
    let witness = strings();
    let values = vec![text("alpha", 48), text("beta", 64)];
    let pointers = values
        .iter()
        .map(|value| value.as_ptr())
        .collect::<Vec<_>>();
    let peer = owned::<Vector<Text>>(vec![text("alpha", 48), text("gamma", 64)], &witness).unwrap();
    let _ = super::slice::lend_ref::<Text>(&values, &witness, &mut |value| {
        let place = get(&value).unwrap();
        assert_eq!(
            place
                .invoke(Operation::Debug, &mut [])?
                .into_string()
                .unwrap(),
            "[\"alpha\", \"beta\"]"
        );
        assert_eq!(
            place
                .invoke(Operation::Equal { reversed: false }, &mut [value.clone()])?
                .as_bool(),
            Ok(true)
        );
        assert_eq!(
            place
                .invoke(
                    Operation::Equal { reversed: false },
                    &mut [Dynamic::from_array(vec!["alpha".into(), "beta".into()])]
                )?
                .as_bool(),
            Ok(true)
        );
        for (reversed, expected) in [(false, -1), (true, 1)] {
            let compared = place.invoke(
                Operation::Compare {
                    total: true,
                    reversed,
                },
                &mut [peer.clone()],
            )?;
            assert_eq!(
                crate::option::read(&compared).unwrap().unwrap().as_int(),
                Ok(expected)
            );
        }
        assert!(place
            .invoke(Operation::Push, &mut ["invalid".into()])
            .is_err());
        Ok(Dynamic::UNIT)
    })
    .unwrap();
    assert_eq!(
        values
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>(),
        pointers
    );
    assert_eq!(
        values.iter().map(String::capacity).collect::<Vec<_>>(),
        [48, 64]
    );
}
