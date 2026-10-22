use super::*;
use crate::input::membership::manifest::reference;

fn inspected(root: &Path, values: &[(&Path, &str, &'static str)]) -> Vec<String> {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let mut warnings = Warnings::new();
    let result = inspect_inputs(
        values.iter().map(|&(path, name, description)| Input {
            path,
            name,
            description,
        }),
        root,
        &mut warnings,
        &mut session,
    );
    session.finish(result).unwrap();
    warnings.as_slice().to_vec()
}

#[test]
fn equal_native_paths_keep_first_spelling_and_target_encounter_order() {
    let warnings = inspected(
        Path::new("/p"),
        &[
            (Path::new("src//same.rs"), "library", "lib"),
            (Path::new("/p/src/./same.rs"), "test", "integration-test"),
            (
                Path::new("src/same.rs"),
                "build-script-same",
                "build-script",
            ),
            (Path::new("src/other.rs"), "other", "bin"),
        ],
    );
    assert_eq!(warnings,["file `/p/src//same.rs` found to be present in multiple build targets:\n  * `lib` target `library`\n  * `integration-test` target `test`\n  * `build-script` target `build-script-same`"]);
}

#[test]
fn parent_components_are_not_canonicalized_and_nonconflicting_input_is_quiet() {
    assert!(inspected(Path::new("/p"), &[]).is_empty());
    assert!(inspected(
        Path::new("/p"),
        &[
            (Path::new("a/../same.rs"), "one", "example"),
            (Path::new("same.rs"), "two", "bench"),
        ]
    )
    .is_empty());
}

#[test]
fn second_map_insertion_order_controls_conflict_warning_order() {
    let owned = (0..25)
        .map(|index| format!("dir/file{index}.rs"))
        .collect::<Vec<_>>();
    let mut values = Vec::new();
    for path in &owned {
        values.push((Path::new(path), "first", "bin"));
    }
    for (index, path) in owned.iter().enumerate() {
        if index % 3 != 0 {
            values.push((Path::new(path), "second", "example"));
        }
    }
    let actual = inspected(Path::new("/p"), &values);
    // The separately qualified PathTable is the iteration oracle; this test
    // checks the required two-map composition, not the hash implementation.
    let paths = owned
        .iter()
        .map(|path| Path::new("/p").join(path))
        .collect::<Vec<_>>();
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let mut first = PathTable::new();
    for (index, path) in paths.iter().enumerate() {
        first.merge(path, index, &mut session).unwrap();
    }
    let mut second = PathTable::new();
    let mut first = first.into_iter(&mut session).unwrap();
    while let Some((path, index)) = first.next(&mut session).unwrap() {
        if index % 3 != 0 {
            second.insert(path, index, &mut session).unwrap();
        }
    }
    let mut expected = Vec::new();
    let mut second = second.iter(&mut session).unwrap();
    while let Some((path, _)) = second.next(&mut session).unwrap() {
        expected.push(format!("file `{}` found to be present in multiple build targets:\n  * `bin` target `first`\n  * `example` target `second`",path.display()));
    }
    session.finish(Ok(())).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn raw_native_path_display_is_preserved_without_unicode_filtering() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let path = Path::new(OsStr::from_bytes(b"/native/\xff.rs"));
    let actual = inspected(
        Path::new("/p"),
        &[(path, "one", "lib"), (path, "two", "bin")],
    );
    assert_eq!(actual,["file `/native/�.rs` found to be present in multiple build targets:\n  * `lib` target `one`\n  * `bin` target `two`"]);
}

#[test]
fn every_admission_refusal_remains_terminal_without_later_warning_work() {
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
    let values = [
        (Path::new("a.rs"), "a", "lib"),
        (Path::new("a.rs"), "b", "bin"),
    ];
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let mut warnings = Warnings::new();
    let result = inspect_inputs(
        values.iter().map(|&(path, name, description)| Input {
            path,
            name,
            description,
        }),
        Path::new("/p"),
        &mut warnings,
        &mut session,
    );
    session.finish(result).unwrap();
    for index in 0..baseline.calls {
        let mut account = Account {
            refused: Some(index),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        let mut warnings = Warnings::new();
        let result = inspect_inputs(
            values.iter().map(|&(path, name, description)| Input {
                path,
                name,
                description,
            }),
            Path::new("/p"),
            &mut warnings,
            &mut session,
        );
        assert!(result.is_err());
        assert!(inspect_inputs(
            std::iter::empty(),
            Path::new("/p"),
            &mut warnings,
            &mut session
        )
        .is_err());
        assert!(matches!(session.finish(result),Err(Error::Admission(at)) if at==index));
        assert_eq!(account.calls, index + 1);
    }
}
