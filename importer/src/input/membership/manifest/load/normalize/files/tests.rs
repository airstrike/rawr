use std::ffi::OsStr;

use super::*;
use crate::cargo;
use crate::input::membership::manifest::{schema, storage, Error, Stop};
use crate::input::membership::{observed, stored, Budget};
use crate::{input, node};

mod resources;

fn image(records: Vec<(&[u8], observed::Value)>) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records: Vec<_> = records
        .into_iter()
        .map(|(path, value)| {
            let path = observed::Path::from_bytes(path, &mut budget).unwrap();
            let query = if matches!(value, observed::Value::File { .. }) {
                observed::Query::File(path)
            } else {
                observed::Query::Metadata { path, follow: true }
            };
            observed::Record { query, value }
        })
        .collect();
    records.sort_by(|left, right| left.query.cmp(&right.query));
    stored::Stored::from_records(&records, &mut budget).unwrap()
}

fn metadata(kind: observed::Kind) -> observed::Value {
    observed::Value::Metadata(observed::Metadata {
        kind,
        device: 1,
        inode: 2,
        owner: 3,
        mode: 0o644,
        length: 4,
        modified_seconds: 0,
        modified_nanos: 0,
        changed_seconds: 0,
        changed_nanos: 0,
    })
}

fn check(
    image: &stored::Stored,
    paths: usize,
    operation: impl FnOnce(&mut Session<'_, host::Host<'_, '_>>) -> Step<()>,
) {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - paths)
        .unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    let mut allocation = input::Allocation::new(&limits);
    let mut host = host::Host::from_image(
        image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let mut session = Session::new(&mut host);
    let result = operation(&mut session);
    session.finish(result).unwrap();
    host.finish().unwrap();
}

#[test]
fn absent_build_uses_the_exact_followed_native_probe_and_all_file_outcomes() {
    for (value, selected) in [
        (metadata(observed::Kind::File), true),
        (metadata(observed::Kind::Directory), false),
        (metadata(observed::Kind::Symlink), false),
        (metadata(observed::Kind::Fifo), false),
        (metadata(observed::Kind::Socket), false),
        (metadata(observed::Kind::Character), false),
        (metadata(observed::Kind::Block), false),
        (metadata(observed::Kind::Unknown), false),
        (observed::Value::Absent, false),
        (observed::Value::Error(13), false),
    ] {
        let image = image(vec![(b"/native/\xff/./build.rs", value)]);
        check(&image, 1, |session| {
            let root = Path::new(OsStr::from_bytes(b"/native/\xff/."));
            match build(None, root, session)? {
                Build::Single(Text::Borrowed(text)) if selected => {
                    assert_eq!(text.as_str(), "build.rs");
                }
                Build::Auto(false) if !selected => {}
                result => panic!("wrong default build: {result:?}"),
            }
            Ok(())
        });
    }
}

#[test]
fn explicit_build_variants_preserve_normalized_or_unchanged_values_without_probes() {
    let image = image(vec![]);
    check(&image, 0, |session| {
        let root = Path::new("/not-observed");
        assert!(matches!(
            build(Some(&primitive::Build::Auto(false)), root, session)?,
            Build::Auto(false)
        ));
        let Build::Single(Text::Borrowed(text)) =
            build(Some(&primitive::Build::Auto(true)), root, session)?
        else {
            panic!("enabled build");
        };
        assert_eq!(text.as_str(), "build.rs");
        for (original, expected) in [
            ("", ""),
            ("./", ""),
            ("one/../two//build.rs", "two/build.rs"),
            ("../../build.rs", "../../build.rs"),
            ("/../abs/./build.rs", "/abs/build.rs"),
            ("src/🦀/../build.rs", "src/build.rs"),
            ("nul\0/build.rs", "nul\0/build.rs"),
        ] {
            let input = primitive::Build::Single(value::Text::Borrowed(original));
            let Build::Single(Text::Owned(text)) = build(Some(&input), root, session)? else {
                panic!("normalized single script");
            };
            assert_eq!(text, expected);
        }
        for entries in [&[][..], &["../raw//one", "", "/two/../raw", "nul\0"][..]] {
            let mut values = storage::Buffer::new();
            for entry in entries {
                values.push(value::Text::Borrowed(entry), session)?;
            }
            let pointer = values.as_slice().as_ptr();
            let input = primitive::Build::Multiple(values);
            let Build::Multiple(values) = build(Some(&input), root, session)? else {
                panic!("multiple scripts");
            };
            assert_eq!(values.as_ptr(), pointer);
            assert_eq!(
                values.iter().map(value::Text::as_str).collect::<Vec<_>>(),
                entries
            );
        }
        Ok(())
    });
}

#[test]
fn readme_defaults_probe_in_order_stop_at_the_first_file_and_preserve_absence() {
    let names = ["README.md", "README.txt", "README"];
    for selected in 0..=names.len() {
        let paths: Vec<_> = names
            .iter()
            .map(|name| format!("/package/{name}"))
            .collect();
        let count = (selected + 1).min(names.len());
        let records = paths[..count]
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let value = if index == selected {
                    metadata(observed::Kind::File)
                } else {
                    match index {
                        0 => observed::Value::Error(13),
                        1 => metadata(observed::Kind::Directory),
                        _ => observed::Value::Absent,
                    }
                };
                (path.as_bytes(), value)
            })
            .collect();
        let image = image(records);
        check(&image, count, |session| {
            let output = readme(Path::new("/package"), None, session)?;
            assert_eq!(
                output.as_ref().map(Text::as_str),
                names.get(selected).copied()
            );
            assert!(output
                .as_ref()
                .is_none_or(|text| matches!(text, Text::Borrowed(_))));
            Ok(())
        });
    }
}

#[test]
fn explicit_readme_is_borrowed_verbatim_without_path_or_file_checks() {
    let image = image(vec![]);
    check(&image, 0, |session| {
        let root = Path::new(OsStr::from_bytes(b"/unobserved/\xff"));
        assert!(readme(
            root,
            Some(&primitive::StringOrBool::Boolean(false)),
            session
        )?
        .is_none());
        assert_eq!(
            readme(root, Some(&primitive::StringOrBool::Boolean(true)), session)?
                .unwrap()
                .as_str(),
            "README.md"
        );
        for original in [
            "",
            "../docs//README",
            "/absolute/../file",
            "read\0me",
            "🦀\nREADME",
        ] {
            let input = primitive::StringOrBool::String(value::Text::Borrowed(original));
            let Some(Text::Borrowed(text)) = readme(root, Some(&input), session)? else {
                panic!("borrowed readme");
            };
            assert_eq!(text.as_str(), original);
            assert_eq!(text.as_str().as_ptr(), original.as_ptr());
        }
        Ok(())
    });
}

#[test]
fn actual_document_and_complete_original_feed_both_helpers_without_reinterpretation() {
    let image = image(vec![(b"/Cargo.toml", observed::Value::File {
        bytes: b"[package]\nname='example'\nbuild='src/../custom//build.rs'\nreadme=\"docs/../read\\u006de.md\"\n".to_vec(),
        end: observed::End::Eof,
    })]);
    check(&image, 1, |session| {
        let document = session
            .document(b"/Cargo.toml", &cargo::Limits::standard())?
            .unwrap();
        let original =
            schema::manifest::Original::interpret(value::Input::root(&document), session)?;
        let Some(schema::manifest::Value::Package(package)) =
            original.get(schema::manifest::Field::Package)
        else {
            panic!("package");
        };
        let Some(schema::package::Value::Local(primitive::Value::Build(input))) =
            package.get(schema::package::Field::Build)
        else {
            panic!("build");
        };
        let Build::Single(output) = build(Some(input), Path::new("/"), session)? else {
            panic!("single build");
        };
        assert_eq!(output.as_str(), "custom/build.rs");
        let Some(schema::package::Value::Local(primitive::Value::StringOrBool(input))) =
            package.get(schema::package::Field::Readme)
        else {
            panic!("readme");
        };
        let primitive::StringOrBool::String(original) = input else {
            panic!("string readme");
        };
        let Some(Text::Borrowed(output)) = readme(Path::new("/"), Some(input), session)? else {
            panic!("readme output");
        };
        assert_eq!(output.as_str(), "docs/../readme.md");
        assert_eq!(output.as_str().as_ptr(), original.as_str().as_ptr());
        Ok(())
    });
}
