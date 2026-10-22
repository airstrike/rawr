//! Persisted preflight selects bytes; all observations are rebuilt in the Layout.
use super::*;

#[derive(serde::Serialize)]
struct Preflight {
    schema: u32,
    request: [u8; 32],
    universe: [u8; 32],
    manifest: [u8; 32],
    source: [u8; 32],
    lock: [u8; 32],
    metadata: [u8; 32],
    unit: [u8; 32],
    build: [u8; 32],
    input: [u8; 32],
    complete: [u8; 32],
}

fn exercise(tamper: bool) {
    let mut fixture = Fixture::open();
    let limits = Limits::standard();
    let request = &mut fixture.request;
    let cached = cargo::CachedToolchain {
        context: fixture.context.clone(),
        attestation: fixture.context.cached_toolchain().unwrap(),
    };
    let legacy = request.cache_root.join(cargo::HARNESS_DIRECTORY);
    Fixture::reserve_parts(&fixture.context, &fixture.store, |layout, native| {
        let (final_bytes, units) = documents(request, layout.final_form());
        let (provisional, _) = documents(request, layout.provisional());
        bind_universe(request, &cached);
        let candidate = final_bytes;
        let steps = vec![
            expected_reply(
                request,
                native,
                layout.provisional(),
                Command::Metadata(false),
                true,
                move |_| test_launch::Reply::stdout(provisional),
            ),
            expected_reply(
                request,
                native,
                layout.final_form(),
                Command::Metadata(false),
                true,
                move |_| test_launch::Reply::stdout(candidate),
            ),
            expected_reply(
                request,
                native,
                layout.final_form(),
                Command::Units,
                true,
                move |_| test_launch::Reply::stdout(units),
            ),
        ];
        let mut allocation = request.allocation;
        let harness = super::super::script(layout, steps)
            .run(|| {
                request.prepare_harness_in(
                    layout,
                    &cached,
                    &limits,
                    &limits,
                    &mut allocation,
                    unit::Allocation::begin(&limits),
                )
            })
            .unwrap()?;
        assert!(harness.universe.is_some());
        let manifest = fs::read(layout.final_form().manifest()).unwrap();
        let source = fs::read(layout.final_form().source()).unwrap();
        let lock = fs::read(layout.final_form().lock()).unwrap();
        fs::create_dir_all(legacy.join("src")).unwrap();
        fs::write(legacy.join(cargo::HARNESS_MANIFEST), &manifest).unwrap();
        fs::write(legacy.join("src/lib.rs"), &source).unwrap();
        fs::write(legacy.join("Cargo.lock"), &lock).unwrap();
        let definition = request.universe.as_ref().unwrap().current();
        let provisional = cargo::render_manifest(request, None, &limits)?;
        // Only preflight identity is fixture-authenticated. Zero completion
        // fields deliberately cannot serve as a portable old completed receipt.
        let receipt = Preflight {
            schema: 2,
            request: *cargo::versioned_request_identity(request, definition, &provisional)
                .as_bytes(),
            universe: *definition.identity(),
            manifest: *cargo::digest(&manifest).as_bytes(),
            source: *cargo::digest(&source).as_bytes(),
            lock: *cargo::digest(&lock).as_bytes(),
            metadata: [0; 32],
            unit: [0; 32],
            build: [0; 32],
            input: [0; 32],
            complete: [0; 32],
        };
        fs::write(
            legacy.join(cargo::UNIVERSE_RECEIPT),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        Ok(())
    });
    if tamper {
        fs::write(legacy.join("Cargo.lock"), b"tampered").unwrap();
    }
    let names = [
        cargo::HARNESS_MANIFEST,
        "src/lib.rs",
        "Cargo.lock",
        cargo::UNIVERSE_RECEIPT,
    ];
    let retained: Vec<_> = names
        .iter()
        .map(|name| fs::read(legacy.join(name)).unwrap())
        .collect();
    Fixture::reserve_parts(&fixture.context, &fixture.store, |layout, native| {
        let (metadata, units) = documents(request, layout.final_form());
        let mut allocation = request.allocation;
        if tamper {
            let result = super::super::script(layout, Vec::new())
                .run(|| {
                    request.prepare_harness_in(
                        layout,
                        &cached,
                        &limits,
                        &limits,
                        &mut allocation,
                        unit::Allocation::begin(&limits),
                    )
                })
                .unwrap();
            assert!(result.is_err());
            assert!(!layout.final_form().manifest().exists());
        } else {
            // The compatibility constructor still rejects Persisted before commands.
            assert!(request
                .prepare_fresh_harness_in(
                    layout,
                    &cached,
                    &limits,
                    &limits,
                    &mut allocation,
                    unit::Allocation::begin(&limits)
                )
                .is_err());
            let steps = vec![
                expected_reply(
                    request,
                    native,
                    layout.final_form(),
                    Command::Metadata(true),
                    true,
                    move |_| test_launch::Reply::stdout(metadata),
                ),
                expected_reply(
                    request,
                    native,
                    layout.final_form(),
                    Command::Units,
                    true,
                    move |_| test_launch::Reply::stdout(units),
                ),
            ];
            let harness = super::super::script(layout, steps)
                .run(|| {
                    request.prepare_harness_in(
                        layout,
                        &cached,
                        &limits,
                        &limits,
                        &mut allocation,
                        unit::Allocation::begin(&limits),
                    )
                })
                .unwrap()?;
            assert_eq!(harness.harness_root, layout.final_form().root());
            assert_eq!(fs::read(layout.final_form().lock()).unwrap(), retained[2]);
            assert!(!layout.provisional().manifest().exists());
            assert!(!layout.receipt().exists());
        }
        for (name, expected) in names.iter().zip(&retained) {
            assert_eq!(&fs::read(legacy.join(name)).unwrap(), expected);
        }
        Ok(())
    });
    fixture.retire();
}

#[test]
fn persisted_inputs_reconstruct_with_only_locked_layout_commands() {
    exercise(false);
}

#[test]
fn invalid_persisted_lock_rejects_before_layout_mutation() {
    exercise(true);
}
