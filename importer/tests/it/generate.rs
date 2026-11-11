use crate::p3;

use std::fs;
use std::path::PathBuf;

#[test]
fn exact_operation_forms_emit_rooted_normalized_thunks() {
    let manifest = p3::generated();
    let runner = manifest
        .files()
        .find(|file| file.path() == "src/rawr_runner.rs")
        .expect("the generated runner has its module");
    let runner = std::str::from_utf8(runner.bytes()).expect("generated Rust is UTF-8");
    assert!(runner.contains("static ADAPTER_MODULES: &[&[Adapter]]"));
    assert!(
        !runner.contains("fn adapter_"),
        "adapters live in their crate's modules"
    );
    let source = manifest
        .files()
        .filter(|file| file.path().ends_with(".rs"))
        .map(|file| std::str::from_utf8(file.bytes()).expect("generated Rust is UTF-8"))
        .collect::<String>();
    let source = source.as_str();

    assert!(source.contains("pub static ADAPTERS: &[Adapter]"));
    assert!(source.contains("use super::*;"));
    assert!(source.contains("facade_a::Record::new("));
    assert!(source.contains("let receiver: facade_a::Record = take_native"));
    assert!(source.contains("arena.shared::<facade_a::Record, _>"));
    assert!(source.contains("arena.mutable::<facade_a::Record, _>"));
    assert!(source.contains("receiver.owned("));
    assert!(source.contains("facade_a::Forms::Tuple { 0: "));
    assert!(source.contains("struct Arena"));
    // The adapters hold native values in the arena, never behind a lock or
    // a count. The runtime counts and queues its handles' releases (D513.7).
    let adapters = manifest
        .files()
        .filter(|file| is_adapter_module(file.path()))
        .map(|file| std::str::from_utf8(file.bytes()).expect("generated Rust is UTF-8"))
        .collect::<String>();
    assert!(!adapters.is_empty());
    assert!(!adapters.contains("Arc<"));
    assert!(!adapters.contains("Mutex<"));
    assert!(!source.as_bytes().contains(&b'\r'));
}

/// Whether `path` is an adapter module, in its root's crate (#1670).
fn is_adapter_module(path: &str) -> bool {
    path.starts_with("rawr-crates/adapters-") && path.contains("/src/rawr_adapters_")
}

/// The adapters are one crate per imported root, one module per identity
/// bucket. Generation is deterministic, and dropping one adapter changes
/// exactly one module file, so syncing the second generation over the first
/// rewrites only that file; every other file keeps its mtime.
#[test]
fn one_adapter_change_rewrites_only_its_module_file() {
    let project = rawr_import::store::TestProject::new("rawr-p3-modules");
    let sources = |surface: p3::Surface| {
        p3::request_at(project.path(), b"exact p3 surface", surface)
            .0
            .generate()
            .expect("the generated manifest seals")
            .files()
            .map(|file| (file.path().to_owned(), file.bytes().to_vec()))
            .collect::<Vec<_>>()
    };
    let before = sources(p3::Surface::Complete);
    assert_eq!(
        before,
        sources(p3::Surface::Complete),
        "same inputs, same bytes"
    );
    let modules = before
        .iter()
        .filter(|(path, _)| is_adapter_module(path))
        .count();
    assert!(modules > 1, "adapters split into modules");

    // The first outcome whose removal leaves its module in place.
    let (after, module) = (0..32)
        .find_map(|skip| {
            let after = sources(p3::Surface::Without(skip));
            let changed = before
                .iter()
                .zip(&after)
                .filter(|(left, right)| left != right)
                .map(|(left, _)| left.0.clone())
                .collect::<Vec<_>>();
            // A demand change also restamps `src/rawr_surface.rs`;
            // every other source stays byte-identical.
            match changed.as_slice() {
                [module, stamps]
                    if after.len() == before.len()
                        && is_adapter_module(module)
                        && stamps == "src/rawr_surface.rs" =>
                {
                    Some((after, module.clone()))
                }
                _ => None,
            }
        })
        .expect("one adapter's removal changes exactly one adapter module");

    let work = project.path().join("work");
    let sync = |files: &[(String, Vec<u8>)]| {
        rawr_import::bridge::sync_sources(
            &work,
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        )
        .expect("the sources sync")
    };
    assert_eq!(sync(&before).len(), before.len());
    let modified = |path: &str| {
        fs::metadata(work.join(path))
            .and_then(|metadata| metadata.modified())
            .expect("a synced file")
    };
    let stamps = before
        .iter()
        .map(|(path, _)| modified(path))
        .collect::<Vec<_>>();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let surface = "src/rawr_surface.rs";
    assert_eq!(
        sync(&after),
        [PathBuf::from(&module), PathBuf::from(surface)]
    );
    for ((path, _), stamp) in before.iter().zip(stamps) {
        assert_eq!(
            modified(path) != stamp,
            *path == module || path == surface,
            "{path}"
        );
    }
}
