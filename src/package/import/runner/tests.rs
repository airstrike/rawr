use crate::{host, package::import::runner, Engine};

/// `text` with each facade proof written as `Fingerprint`: a held facade's
/// proof is the digest its image carries, a built one's its slices.
fn without_proofs(text: &str) -> String {
    let mut kept = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("Fingerprint {") {
        kept.push_str(&rest[..start]);
        kept.push_str("Fingerprint");
        let mut depth = 0;
        let mut end = start;
        for (offset, character) in rest[start..].char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + offset + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &rest[end..];
    }
    kept.push_str(rest);
    kept
}

fn items(items: &[host::item::Definition]) -> String {
    without_proofs(&format!("{items:?}"))
}

fn entry(entry: Option<host::Entry<'_>>) -> String {
    match entry {
        None => "none".to_owned(),
        Some(host::Entry::Module) => "module".to_owned(),
        Some(host::Entry::ModuleItem(found)) => format!("module {}", items(found)),
        Some(host::Entry::Type(ty)) => without_proofs(&format!("type {ty:?}")),
        Some(host::Entry::Item(found)) => format!("items {}", items(found)),
        Some(host::Entry::Builtin(builtin)) => format!("builtin {builtin}"),
        Some(host::Entry::TypeAlias(alias)) => without_proofs(&format!("alias {alias:?}")),
    }
}

/// What each lookup of `path` finds in `registry`.
fn lookups(registry: &host::Registry, path: &str) -> Vec<String> {
    let parsed = host::path::Path::parse(path).unwrap();
    let below = format!("{path}::__absent");
    vec![
        entry(registry.entry(path)),
        entry(registry.entry(&below)),
        format!("{:?}", registry.canonical(&parsed)),
        format!("{:?}", registry.std_alias(path)),
        format!("{:?}", registry.builtin_alias(&parsed)),
        format!("{:?}", registry.enum_variant(path)),
        format!("{:?}", registry.enum_variants(path)),
        format!(
            "{:?}",
            registry
                .type_owner(path)
                .map(host::facade::Dependency::root)
        ),
        format!(
            "{:?}",
            registry
                .item_owner(path)
                .map(host::facade::Dependency::root)
        ),
        format!(
            "{:?}",
            registry
                .pending_family(path)
                .map(|family| items(family.methods()))
        ),
        without_proofs(&format!("{:?}", registry.trait_header(&parsed))),
        format!("{:?}", registry.derive_macro(path)),
        format!("{:?}", registry.overloads(path).map(items)),
        without_proofs(&format!("{:?}", registry.type_(path))),
        without_proofs(&format!("{:?}", registry.source_type(path))),
    ]
}

/// What a glob import of the module at `path` binds, and the items under
/// the type at `path`: lookups that read the whole registry, made only
/// where they find something.
fn inside(registry: &host::Registry, path: &str) -> Vec<String> {
    vec![
        format!("{:?}", registry.children(path)),
        items(&registry.members(path).cloned().collect::<Vec<_>>()),
    ]
}

/// The paths of the functions named `name`, sorted.
fn functions(registry: &host::Registry, name: &str) -> Vec<String> {
    let mut functions = registry.functions_named(name).collect::<Vec<_>>();
    functions.sort();
    functions
}

/// The lookups of a registry `built` from a plan whole, and of one that
/// `held` holds the plan's image, find the same at every path and of every
/// identity the built one holds.
fn same_lookups(built: &host::Registry, held: &host::Registry) {
    let paths = built.every_path();
    for path in &paths {
        assert_eq!(lookups(built, path), lookups(held, path), "at `{path}`");
        if matches!(
            built.entry(path),
            Some(host::Entry::Module | host::Entry::ModuleItem(_) | host::Entry::Type(_))
        ) {
            assert_eq!(inside(built, path), inside(held, path), "inside `{path}`");
        }
    }
    let names = paths
        .iter()
        .filter_map(|path| path.rsplit("::").next())
        .collect::<std::collections::BTreeSet<_>>();
    for name in names {
        assert_eq!(
            functions(built, name),
            functions(held, name),
            "named `{name}`"
        );
    }
    let (types, traits) = built.every_identity();
    for identity in &types {
        let anywhere = |registry: &host::Registry| {
            registry
                .imported_type_paths_anywhere(identity)
                .collect::<Vec<_>>()
        };
        assert_eq!(anywhere(built), anywhere(held));
        for root in built
            .every_path()
            .iter()
            .filter(|path| !path.contains("::"))
        {
            let owner = |registry: &host::Registry| registry.type_owner(root).cloned();
            let (Some(built_owner), Some(held_owner)) = (owner(built), owner(held)) else {
                continue;
            };
            let found = |registry: &host::Registry, owner| {
                let imported = registry.imported_type(identity, owner, None);
                without_proofs(&format!(
                    "{:?}",
                    imported.map(|found| (found.definition.path(), found.alias.is_some()))
                ))
            };
            assert_eq!(found(built, &built_owner), found(held, &held_owner));
            let paths = |registry: &host::Registry, owner| {
                registry
                    .imported_type_paths(identity, owner)
                    .map(Iterator::collect::<Vec<_>>)
            };
            assert_eq!(paths(built, &built_owner), paths(held, &held_owner));
        }
    }
    for identity in traits {
        let impls = |registry: &host::Registry| {
            without_proofs(&format!(
                "{:?}",
                registry.impl_headers(identity).collect::<Vec<_>>()
            ))
        };
        assert_eq!(impls(built), impls(held));
    }
}

/// Install `plan`, a runner's plan, built whole and held in its image.
fn both(plan: &[u8], image: &'static [u8]) -> (Engine, Engine) {
    let mut built = Engine::new();
    runner::install_plan(&mut built, plan).unwrap();
    let mut held = Engine::new();
    runner::install(&mut held, image).unwrap();
    (built, held)
}

/// A runner's facades held in its image find at each path what the same
/// facades built whole do: modules, types, items, re-exported modules,
/// the paths of each crate type, and what a glob import binds.
#[test]
fn a_held_facade_finds_what_a_built_one_does() {
    for (plan, image) in [
        (
            rawr_import::test_fixture::runner_native_plan_json(),
            rawr_import::test_fixture::runner_native_facade_plan(),
        ),
        (
            rawr_import::test_fixture::runner_reexported_plan_json(),
            rawr_import::test_fixture::runner_reexported_facade_plan(),
        ),
        (
            rawr_import::test_fixture::runner_foreign_plan_json(),
            rawr_import::test_fixture::runner_foreign_facade_plan(),
        ),
    ] {
        let (built, held) = both(&plan, Box::leak(image));
        same_lookups(&built.host_registry, &held.host_registry);
    }
}

/// A runner's start reads its image's header and builds no entry: each is
/// built the first time a lookup reaches it, and once.
#[test]
fn a_held_facade_builds_an_entry_when_a_lookup_reaches_it_and_once() {
    let mut engine = Engine::new();
    let image = Box::leak(rawr_import::test_fixture::runner_native_facade_plan());
    let definitions = runner::install(&mut engine, image).unwrap();
    let native = definitions
        .iter()
        .find(|definition| definition.root() == "native")
        .unwrap();
    let held = native.held().unwrap();
    assert_eq!(held.built(), 0);
    let Some(host::Entry::Type(source)) = engine.host_registry.entry("native::Source") else {
        panic!("`native::Source` is the facade's type");
    };
    assert_eq!(held.built(), 1);
    let again = engine.host_registry.type_("native::Source").unwrap();
    assert!(std::ptr::eq(source, again));
    assert_eq!(held.built(), 1);
}
