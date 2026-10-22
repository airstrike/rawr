use super::*;
use crate::input::membership::manifest::{reference, value, Error};

fn original_warnings(source: &str) -> (Result<(), String>, Vec<String>) {
    let mut account = reference::Account;
    let document = document::parse(source, 80, &mut account).unwrap();
    let mut session = Session::new(&mut account);
    let input = crate::input::membership::manifest::schema::manifest::Original::interpret(
        value::Input::root(&document),
        &mut session,
    )
    .unwrap();
    let mut warnings = Warnings::new();
    let result = original(
        &input,
        std::path::Path::new("/p/Cargo.toml"),
        &mut warnings,
        &mut session,
    );
    let result = session.finish(result).unwrap();
    (result, warnings.as_slice().to_vec())
}

#[test]
fn original_maps_use_top_and_platform_specific_order_and_normalized_labels() {
    let (result, warnings) = original_warnings(
        r#"
[dependencies]
n={extra=1}
[dev-dependencies]
d={extra=1}
[dev_dependencies]
ignored={extra=1}
[build-dependencies]
b={extra=1}
[target.'cfg( all( unix, ) )'.dependencies]
n={extra=1}
[target.'cfg( all( unix, ) )'.build-dependencies]
b={extra=1}
[target.'cfg( all( unix, ) )'.dev-dependencies]
d={extra=1}
"#,
    );
    result.unwrap();
    assert_eq!(
        warnings,
        [
            "unused manifest key: dependencies.n.extra",
            "unused manifest key: dev-dependencies.d.extra",
            "unused manifest key: build-dependencies.b.extra",
            "unused manifest key: target.cfg(all(unix)).dependencies.n.extra",
            "unused manifest key: target.cfg(all(unix)).build-dependencies.b.extra",
            "unused manifest key: target.cfg(all(unix)).dev-dependencies.d.extra",
        ]
    );
}

#[test]
fn malformed_platform_stops_before_its_dependency_warnings() {
    let (result, warnings) = original_warnings(
        r#"
[dependencies]
n={extra=1}
[target.'cfg(not())'.dependencies]
bad={extra=1}
[target.zzz.dependencies]
later={extra=1}
"#,
    );
    assert!(result.unwrap_err().starts_with("failed to parse"));
    assert_eq!(warnings, ["unused manifest key: dependencies.n.extra"]);
}

#[test]
fn platform_attribute_warnings_precede_dependency_warnings_even_without_deps() {
    let (result, warnings) = original_warnings(
        r#"
[target.'cfg(test)']
[target.'cfg(proc_macro)'.dependencies]
n={extra=1}
"#,
    );
    result.unwrap();
    assert_eq!(warnings.len(), 3);
    assert!(warnings[0].starts_with("Found `proc_macro`"));
    assert_eq!(
        warnings[1],
        "unused manifest key: target.cfg(proc_macro).dependencies.n.extra"
    );
    assert!(warnings[2].starts_with("Found `test`"));
}

fn map<'d>(document: &'d document::Document<'d>) -> dependency::inheritable::Map<'d> {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let result = dependency::inheritable::Map::interpret(
        value::Input::raw(document.table().get("dependencies").unwrap()),
        &mut session,
    );
    session.finish(result).unwrap()
}

#[test]
fn sorted_original_names_and_extras_keep_exact_warning_text() {
    let document = document::parse(
        "[dependencies]\nz={workspace=true,zeta=1,alpha=2}\nb='1'\na={version='1',zeta=1,alpha=2}\n",
        80,
        &mut reference::Account,
    ).unwrap();
    let original = map(&document);
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let mut warnings = Warnings::new();
    let result = inspect(Some(&original), "dependencies", &mut warnings, &mut session);
    session.finish(result).unwrap();
    assert_eq!(
        warnings.as_slice(),
        [
            "unused manifest key: dependencies.a.alpha",
            "unused manifest key: dependencies.a.zeta",
            "unused manifest key: dependencies.z.alpha",
            "unused manifest key: dependencies.z.zeta",
        ]
    );
}

#[test]
fn absent_simple_and_known_fields_are_quiet() {
    let document = document::parse(
        "[dependencies]\na='1'\nb={version='1',optional=true,features=['f']}\nc={workspace=true,optional=true}",
        80,
        &mut reference::Account,
    ).unwrap();
    let original = map(&document);
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let mut warnings = Warnings::new();
    inspect(None, "dependencies", &mut warnings, &mut session).unwrap();
    let result = inspect(Some(&original), "dependencies", &mut warnings, &mut session);
    session.finish(result).unwrap();
    assert!(warnings.as_slice().is_empty());
}

#[test]
fn simple_dependency_still_formats_label_before_empty_key_collection() {
    struct Account;
    impl document::Admission for Account {
        type Error = &'static str;
        fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
            if matches!(request, document::Request::String(12)) {
                Err("table label")
            } else {
                Ok(())
            }
        }
    }
    let document = document::parse("[dependencies]\na='1'", 80, &mut reference::Account).unwrap();
    let original = map(&document);
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut warnings = Warnings::new();
    let result = inspect(Some(&original), "dependencies", &mut warnings, &mut session);
    assert!(matches!(
        session.finish(result),
        Err(Error::Admission("table label"))
    ));
    assert!(warnings.as_slice().is_empty());
}

#[test]
fn supplied_platform_label_is_preserved_verbatim() {
    let document =
        document::parse("[dependencies]\na={extra=1}", 80, &mut reference::Account).unwrap();
    let original = map(&document);
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let mut warnings = Warnings::new();
    let result = inspect(
        Some(&original),
        "target.cfg(unix).dev-dependencies",
        &mut warnings,
        &mut session,
    );
    session.finish(result).unwrap();
    assert_eq!(
        warnings.as_slice(),
        ["unused manifest key: target.cfg(unix).dev-dependencies.a.extra"]
    );
}

#[test]
fn later_key_collection_refusal_preserves_only_prior_dependency_warnings() {
    use std::cell::Cell;
    struct Account<'a>(&'a Cell<bool>);
    impl document::Admission for Account<'_> {
        type Error = &'static str;
        fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
            if self.0.get() && matches!(request, document::Request::String(7)) {
                Err("later key")
            } else {
                Ok(())
            }
        }
    }
    let enabled = Cell::new(false);
    let mut account = Account(&enabled);
    let document = document::parse(
        "[dependencies]\na={extra=1}\nb={alpha=1,zzzzzzz=2}",
        80,
        &mut account,
    )
    .unwrap();
    let mut session = Session::new(&mut account);
    let original = dependency::inheritable::Map::interpret(
        value::Input::raw(document.table().get("dependencies").unwrap()),
        &mut session,
    )
    .unwrap();
    enabled.set(true);
    let mut warnings = Warnings::new();
    let result = inspect(Some(&original), "dependencies", &mut warnings, &mut session);
    assert!(result.is_err());
    assert!(inspect(None, "dependencies", &mut warnings, &mut session).is_err());
    assert!(matches!(
        session.finish(result),
        Err(Error::Admission("later key"))
    ));
    assert_eq!(
        warnings.as_slice(),
        ["unused manifest key: dependencies.a.extra"]
    );
}

#[test]
fn every_helper_admission_failure_latches_without_later_work() {
    #[derive(Default)]
    struct Account {
        calls: usize,
        refused: Option<usize>,
    }
    impl document::Admission for Account {
        type Error = usize;
        fn admit(&mut self, _: document::Request) -> Result<(), usize> {
            let call = self.calls;
            self.calls += 1;
            if self.refused == Some(call) {
                Err(call)
            } else {
                Ok(())
            }
        }
    }
    let document = document::parse(
        "[dependencies]\na={a=1,z=2}\nb={workspace=true,extra=1}",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let original = map(&document);
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let mut warnings = Warnings::new();
    let result = inspect(Some(&original), "dependencies", &mut warnings, &mut session);
    session.finish(result).unwrap();
    for index in 0..baseline.calls {
        let mut account = Account {
            refused: Some(index),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let mut warnings = Warnings::new();
        let result = inspect(Some(&original), "dependencies", &mut warnings, &mut session);
        assert!(result.is_err());
        assert!(inspect(None, "dependencies", &mut warnings, &mut session).is_err());
        assert!(matches!(session.finish(result),Err(Error::Admission(at)) if at==index));
        assert_eq!(account.calls, index + 1);
    }
}
