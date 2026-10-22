use super::*;
use crate::input::membership::manifest::{reference, Stop};
use std::path::Path;

#[derive(Default)]
struct Account {
    requests: usize,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, _: document::Request) -> Result<(), usize> {
        let index = self.requests;
        self.requests += 1;
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn definition(input: &serde_json::Value) -> Definition<'_> {
    match input[0].as_str().unwrap() {
        "builtin" => Definition::BuiltIn,
        "file" => Definition::Path(Path::new(input[1].as_str().unwrap())),
        "env" => Definition::Environment(input[1].as_str().unwrap()),
        "cli" => Definition::Cli(input[1].as_str().map(Path::new)),
        _ => panic!("invalid fixture"),
    }
}

fn origin(definition: Definition<'_>) -> serde_json::Value {
    match definition {
        Definition::BuiltIn => serde_json::json!(["builtin", null]),
        Definition::Path(path) => serde_json::json!(["file", path]),
        Definition::Environment(key) => serde_json::json!(["env", key]),
        Definition::Cli(path) => serde_json::json!(["cli", path]),
    }
}

pub(super) fn rendered(tree: &Tree<'_, '_>, index: usize) -> serde_json::Value {
    let node = &tree.nodes.as_slice()[index];
    let kind = node.kind.as_ref().unwrap();
    let data = match kind {
        Kind::String(text) => serde_json::json!(text),
        Kind::Integer(value) => serde_json::json!(value),
        Kind::Boolean(value) => serde_json::json!(value),
        Kind::List(elements) => elements
            .as_slice()
            .iter()
            .map(|element| rendered(tree, element.node))
            .collect(),
        Kind::Table(table) => {
            let mut rows = Vec::new();
            let mut account = Account::default();
            let mut session = Session::new(&mut account);
            let mut iter = table.iter(&mut session).unwrap();
            while let Some((key, index)) = iter.next(&mut session).unwrap() {
                rows.push((key, rendered(tree, index)));
            }
            rows.sort_by_key(|(key, _)| *key);
            rows.into_iter()
                .map(|(key, value)| serde_json::json!([key, value]))
                .collect()
        }
    };
    serde_json::json!([kind.desc(), origin(node.definition), data])
}

#[test]
#[cfg(all(
    target_arch = "aarch64",
    target_pointer_width = "64",
    target_endian = "little"
))]
fn complete_values_origins_and_first_failure_match_pinned_merge() {
    for line in include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-merge/observations.jsonl"
    ))
    .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let steps = row["steps"].as_array().unwrap();
        let documents: Vec<_> = steps
            .iter()
            .map(|step| {
                document::parse(
                    step["source"].as_str().unwrap(),
                    80,
                    &mut reference::Account,
                )
                .unwrap()
            })
            .collect();
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let mut tree = None;
        let mut failure = None;
        let mut expected_span = None;
        for (step, document) in steps.iter().zip(&documents) {
            let values =
                Values::from_document(document, definition(&step["definition"]), &mut session)
                    .unwrap();
            let incoming = Tree::from_values(values, &mut session).unwrap();
            if let Some(path) = row["failure"]["path"].as_array() {
                // Test-only lookup locates the independently selected source
                // failure's span; it does not steer the implementation merge.
                let mut index = 0;
                for part in path {
                    let Some(Kind::Table(table)) = incoming.nodes.as_slice()[index].kind.as_ref()
                    else {
                        break;
                    };
                    let mut probe = Account::default();
                    let mut probe_session = Session::new(&mut probe);
                    let Some(child) = table
                        .get(part.as_str().unwrap(), &mut probe_session)
                        .unwrap()
                    else {
                        break;
                    };
                    index = child;
                }
                expected_span = incoming.nodes.as_slice()[index].span;
            }
            tree = match tree.take() {
                None => Some(incoming),
                Some(old) => {
                    match old.merge(incoming, step["force"].as_bool().unwrap(), &mut session) {
                        Ok(next) => Some(next),
                        Err(cause) => {
                            failure = Some(cause);
                            break;
                        }
                    }
                }
            };
        }
        if let Some(expected) = row.get("failure") {
            let Some(Stop::Rejected(actual)) = failure else {
                panic!("missing expected failure: {row}");
            };
            assert_eq!(actual.tag, Tag::ConfigMerge, "{row}");
            assert_eq!(actual.expected, expected["expected"].as_str(), "{row}");
            assert_eq!(actual.span, expected_span, "{row}");
        } else {
            assert!(failure.is_none(), "{row}");
            assert_eq!(rendered(tree.as_ref().unwrap(), 0), row["value"], "{row}");
        }
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn every_original_admission_refusal_stops_conversion_join_and_merge() {
    let sources = [
        "a=[1,{x='old'}]\ndoc.browser=['old']\nhost.runner=['old']\nx.y=1",
        "a=[2,{x='new'}]\ndoc.browser=['new']\nhost.runner=['new']\nx.z=true",
    ];
    let documents: Vec<_> = sources
        .iter()
        .map(|source| document::parse(source, 80, &mut reference::Account).unwrap())
        .collect();
    fn run<'a>(
        documents: &'a [document::Document<'a>],
        session: &mut Session<'_, Account>,
    ) -> Step<()> {
        let left = Values::from_document(
            &documents[0],
            Definition::Path(Path::new("/project/.cargo/old")),
            session,
        )?;
        let left = Tree::from_values(left, session)?;
        let right = Values::from_document(&documents[1], Definition::Cli(None), session)?;
        let right = Tree::from_values(right, session)?;
        left.merge(right, false, session)?;
        Ok(())
    }
    let mut baseline = Account::default();
    let mut session = Session::new(&mut baseline);
    let result = run(&documents, &mut session);
    session.finish(result).unwrap();
    for refusal in 0..baseline.requests {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(run(&documents, &mut session), Err(Stop::Refused)));
        assert!(matches!(run(&documents, &mut session), Err(Stop::Refused)));
        assert!(matches!(session.finish(Ok(())),Err(Error::Admission(index)) if index==refusal));
        assert_eq!(account.requests, refusal + 1);
    }
}

#[test]
fn deep_merge_and_partial_teardown_use_flat_storage() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            fn chain(
                length: usize,
                integer: i64,
                session: &mut Session<'_, Account>,
            ) -> Tree<'static, 'static> {
                let mut nodes = storage::Buffer::new();
                for index in 0..length {
                    let kind = if index + 1 == length {
                        Kind::Integer(integer)
                    } else {
                        let mut table = table::Table::new();
                        table.insert("child", index + 1, session).unwrap();
                        Kind::Table(table)
                    };
                    nodes
                        .push(
                            Node {
                                kind: Some(kind),
                                definition: Definition::BuiltIn,
                                span: None,
                            },
                            session,
                        )
                        .unwrap();
                }
                Tree { nodes }
            }
            let mut account = Account::default();
            let mut session = Session::new(&mut account);
            let left = chain(4096, 1, &mut session);
            let right = chain(4096, 2, &mut session);
            let merged = left.merge(right, true, &mut session).unwrap();
            assert!(matches!(
                merged.nodes.as_slice()[4095].kind,
                Some(Kind::Integer(2))
            ));
            drop(merged);
            let left = chain(4096, 1, &mut session);
            let mut right = chain(4096, 2, &mut session);
            // Reject at the leaf with every parent continuation still live.
            right.nodes.as_mut_slice()[4095].kind = Some(Kind::List(storage::Buffer::new()));
            assert!(matches!(
                left.merge(right, true, &mut session),
                Err(Stop::Rejected(Diagnostic {
                    tag: Tag::ConfigMerge,
                    ..
                }))
            ));
            session.finish(Ok(())).unwrap();
        })
        .unwrap()
        .join()
        .unwrap();
}
