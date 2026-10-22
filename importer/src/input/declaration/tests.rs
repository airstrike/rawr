use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{scan, Declarations};
use crate::cargo::{self, toolchain};
use crate::input::{Allocation, Usage, INPUT_BYTES_RESOURCE, INPUT_ITEMS_RESOURCE};
use crate::{Error, Result};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-declaration-scanner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("build/package-hash/out")).expect("saved-output fixture");
        Self(fs::canonicalize(root).expect("canonical fixture"))
    }

    fn out(&self) -> PathBuf {
        self.0.join("build/package-hash/out")
    }

    fn output(&self) -> PathBuf {
        self.0.join("build/package-hash/output")
    }

    fn read(&self, bytes: &[u8], allocation: &mut Allocation) -> Result<Declarations> {
        fs::write(self.output(), bytes).expect("saved-output bytes");
        Declarations::read_accounted(
            toolchain::supported_cargo_commit(),
            "fixture-package",
            &self.out(),
            &cargo::Limits::standard(),
            allocation,
        )
    }

    fn base_usage(&self, bytes: &[u8]) -> Usage {
        let out = self.out();
        let output = self.output();
        Usage {
            items: 5,
            bytes: "fixture-package".len()
                + out.as_os_str().len()
                + out.parent().expect("OUT_DIR parent").as_os_str().len()
                + 1
                + "output".len()
                + output.as_os_str().len()
                + bytes.len(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn events(bytes: &[u8]) -> Result<Vec<scan::Event<'_>>> {
    let mut events = Vec::new();
    scan::visit(bytes, |event| -> Result<()> {
        events.push(event);
        Ok(())
    })?;
    Ok(events)
}

fn code(error: Error) -> &'static str {
    match error {
        Error::Input { code, .. } => code,
        other => panic!("expected input error, got {other}"),
    }
}

#[test]
fn scanner_borrows_ordered_duplicate_payloads_without_normalizing_them() {
    let bytes = b"ignored output\n\
        cargo:rerun-if-changed=src\n\
        cargo::rerun-if-env-changed=KEY\n\
        cargo:rerun-if-changed=src\n\
        cargo:rerun-if-env-changed=KEY\n\
        cargo:rustc-cfg=flag\r\n\
        cargo::metadata=raw\0value\n\
        cargo:::opaque=value\n\
        cargo:=\n";
    let actual = events(bytes).expect("borrowed grammar");
    assert_eq!(
        actual,
        [
            scan::Event::Directive("rerun-if-changed=src"),
            scan::Event::Path("src"),
            scan::Event::Directive("rerun-if-env-changed=KEY"),
            scan::Event::Environment("KEY"),
            scan::Event::Directive("rerun-if-changed=src"),
            scan::Event::Path("src"),
            scan::Event::Directive("rerun-if-env-changed=KEY"),
            scan::Event::Environment("KEY"),
            scan::Event::Directive("rustc-cfg=flag\r"),
            scan::Event::Directive("metadata=raw\0value"),
            scan::Event::Directive(":opaque=value"),
            scan::Event::Directive("="),
        ]
    );
    let start = bytes.as_ptr() as usize;
    let end = start + bytes.len();
    for event in actual {
        let value = match event {
            scan::Event::Directive(value)
            | scan::Event::Path(value)
            | scan::Event::Environment(value) => value,
        };
        let address = value.as_ptr() as usize;
        assert!(address >= start && address + value.len() <= end);
    }
    assert!(events(b"").expect("empty output").is_empty());
}

#[test]
fn completion_then_whole_file_utf8_precede_every_event() {
    for (bytes, expected) in [
        (&b"\xff"[..], "truncated-saved-output"),
        (&b"cargo:x=y\n\xff"[..], "truncated-saved-output"),
        (&b"cargo:x=y\n\xff\n"[..], "saved-output-utf8"),
        (&b"cargo:invalid\n\xff\n"[..], "saved-output-utf8"),
    ] {
        let mut calls = 0;
        let result = scan::visit(bytes, |_| -> Result<()> {
            calls += 1;
            Ok(())
        });
        assert_eq!(
            code(result.expect_err("incomplete or non-UTF8 file")),
            expected
        );
        assert_eq!(calls, 0);
    }
}

#[test]
fn malformed_payloads_follow_the_directive_event_but_malformed_instructions_do_not() {
    for (bytes, expected) in [
        (&b"cargo:rerun-if-changed=\n"[..], 1),
        (&b"cargo:rerun-if-env-changed=\n"[..], 1),
        (&b"cargo::rerun-if-env-changed=A=B\n"[..], 1),
        (&b"cargo:rerun-if-env-changed=A\0B\n"[..], 1),
        (&b"cargo:\n"[..], 0),
        (&b"cargo::missing-equals\n"[..], 0),
    ] {
        let mut observed = Vec::new();
        let result = scan::visit(bytes, |event| -> Result<()> {
            observed.push(event);
            Ok(())
        });
        assert_eq!(
            code(result.expect_err("malformed output")),
            "malformed-saved-output"
        );
        assert_eq!(observed.len(), expected);
        assert!(observed
            .iter()
            .all(|event| matches!(event, scan::Event::Directive(_))));
    }
}

#[test]
fn callback_failure_stops_before_later_payload_checks_and_events() {
    let bytes =
        b"cargo:rerun-if-changed=src\ncargo:rerun-if-env-changed=KEY\ncargo:rerun-if-changed=\n";
    for stop in 0..5 {
        let mut calls = 0;
        let result = scan::visit(bytes, |_| -> Result<()> {
            let current = calls;
            calls += 1;
            if current == stop {
                Err(Error::InputAccountRefused)
            } else {
                Ok(())
            }
        });
        assert!(matches!(result, Err(Error::InputAccountRefused)));
        assert_eq!(calls, stop + 1);
    }
}

#[test]
fn scanner_never_consults_the_string_target() {
    // `string_bytes` is an accounting target the scanner never
    // consults; the directive is emitted and the caller's account measures it.
    let mut calls = 0;
    scan::visit(b"cargo:x=y\n", |event| -> Result<()> {
        assert_eq!(event, scan::Event::Directive("x=y"));
        calls += 1;
        Ok(())
    })
    .expect("the scanner never refuses an accounting target");
    assert_eq!(calls, 1);
}

#[test]
fn legacy_collector_matches_events_and_exact_original_charge_order() {
    let fixture = Fixture::new();
    let bytes = b"noise\ncargo:rerun-if-changed=src\ncargo::rerun-if-env-changed=KEY\ncargo:rustc-cfg=flag\n";
    let expected = events(bytes).expect("shared parser observations");
    let mut usage = fixture.base_usage(bytes);
    let mut paths = Vec::new();
    let mut environment = Vec::new();
    let mut directives = Vec::new();
    for event in expected {
        let (values, text) = match event {
            scan::Event::Directive(text) => (&mut directives, text),
            scan::Event::Path(text) => (&mut paths, text),
            scan::Event::Environment(text) => (&mut environment, text),
        };
        values.push(Box::<str>::from(text));
        usage.items += 2;
        usage.bytes += text.len();
    }
    let prior = Usage {
        items: 7,
        bytes: 31,
    };
    let mut allocation = Allocation::with_limits(
        prior.items + usage.items,
        prior.bytes + usage.bytes,
        cargo::Limits::standard().string_bytes,
    );
    allocation.items(prior.items).expect("prior items");
    allocation.bytes(prior.bytes).expect("prior bytes");
    let actual = fixture.read(bytes, &mut allocation).expect("exact account");
    assert_eq!(actual.paths.as_ref(), paths);
    assert_eq!(actual.environment.as_ref(), environment);
    assert_eq!(actual.directives.as_ref(), directives);
    assert_eq!(actual.package_id(), "fixture-package");
    assert_eq!(actual.output, fixture.output());
    assert_eq!(
        allocation.usage(),
        Usage {
            items: prior.items + usage.items,
            bytes: prior.bytes + usage.bytes,
        }
    );
    assert!(!allocation.exceedances().is_exceeded());
    // One item (or byte) under the exact total is an accounting
    // exceedance, not a refusal. The collector still completes, the recorded
    // axis has a high water equal to the exact charge total, and nothing latches.
    for (item_floor, byte_floor) in [(1, 0), (0, 1)] {
        let mut allocation = Allocation::with_limits(
            prior.items + usage.items - item_floor,
            prior.bytes + usage.bytes - byte_floor,
            cargo::Limits::standard().string_bytes,
        );
        allocation.items(prior.items).expect("prior items");
        allocation.bytes(prior.bytes).expect("prior bytes");
        fixture
            .read(bytes, &mut allocation)
            .expect("one under the exact account still collects");
        assert_eq!(
            allocation.usage(),
            Usage {
                items: prior.items + usage.items,
                bytes: prior.bytes + usage.bytes,
            }
        );
        let (exceeded, untouched, total) = if item_floor == 1 {
            (
                INPUT_ITEMS_RESOURCE,
                INPUT_BYTES_RESOURCE,
                prior.items + usage.items,
            )
        } else {
            (
                INPUT_BYTES_RESOURCE,
                INPUT_ITEMS_RESOURCE,
                prior.bytes + usage.bytes,
            )
        };
        let axis = *allocation
            .exceedances()
            .axis(exceeded)
            .expect("the one-under axis records the exact charge total");
        assert_eq!(
            (axis.target, axis.high_water),
            ((total - 1) as u64, total as u64)
        );
        assert!(allocation.exceedances().axis(untouched).is_none());
        allocation
            .items(0)
            .expect("an accounting exceedance never latches the account");
    }
}

#[test]
fn legacy_collector_charges_directive_before_empty_payload_failure() {
    let fixture = Fixture::new();
    let bytes = b"cargo:rerun-if-changed=\n";
    let base = fixture.base_usage(bytes);
    let text = "rerun-if-changed=";
    let mut allocation = Allocation::new(&cargo::Limits::standard());
    let error = fixture
        .read(bytes, &mut allocation)
        .err()
        .expect("empty rerun path");
    assert_eq!(code(error), "malformed-saved-output");
    assert_eq!(
        allocation.usage(),
        Usage {
            items: base.items + 2,
            bytes: base.bytes + text.len(),
        }
    );
    assert!(!allocation.exceedances().is_exceeded());
    // With room for one item less, the directive is still charged before
    // the empty payload fails the grammar: the item axis records the exact
    // directive charge (two items) past its target while the byte total stays
    // exact, the grammar failure is still returned and nothing latches.
    let mut allocation = Allocation::with_limits(
        base.items + 1,
        base.bytes + text.len(),
        cargo::Limits::standard().string_bytes,
    );
    let error = fixture
        .read(bytes, &mut allocation)
        .err()
        .expect("empty rerun path still fails the grammar");
    assert_eq!(code(error), "malformed-saved-output");
    assert_eq!(
        allocation.usage(),
        Usage {
            items: base.items + 2,
            bytes: base.bytes + text.len(),
        }
    );
    let axis = *allocation
        .exceedances()
        .axis(INPUT_ITEMS_RESOURCE)
        .expect("the directive item charge records the item axis");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (
            (base.items + 1) as u64,
            (base.items + 2) as u64,
            (base.items + 2) as u64
        )
    );
    assert!(allocation
        .exceedances()
        .axis(INPUT_BYTES_RESOURCE)
        .is_none());
    allocation
        .bytes(0)
        .expect("an accounting exceedance never latches the account");
}
