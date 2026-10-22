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

fn tree<'a>(document: &'a document::Document<'a>, definition: Definition<'a>) -> Tree<'a, 'a> {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let values = Values::from_document(document, definition, &mut session).unwrap();
    Tree::from_values(values, &mut session).unwrap()
}

fn at(tree: &Tree<'_, '_>, parts: &[&str]) -> usize {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut index = 0;
    for part in parts {
        let Some(Kind::Table(table)) = tree.nodes.as_slice()[index].kind.as_ref() else {
            panic!("expected table");
        };
        index = table.get(part, &mut session).unwrap().unwrap();
    }
    index
}

#[test]
fn legacy_token_keeps_its_origin_and_existing_registry_always_wins() {
    for registry in [
        "",
        "registry = false",
        "registry = 3",
        "registry = 'existing'",
        "registry = []",
        "registry = {}",
    ] {
        let text = format!("token = ['legacy']\n{registry}\n");
        let document = document::parse(&text, 80, &mut reference::Account).unwrap();
        let definition = Definition::Path(Path::new("/home/.cargo/credentials"));
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let tree = tree(&document, definition)
            .normalize_credentials(&mut session)
            .unwrap();
        let Some(Kind::Table(root)) = tree.nodes.as_slice()[0].kind.as_ref() else {
            panic!()
        };
        assert!(root.get("token", &mut session).unwrap().is_none());
        let registry_index = at(&tree, &["registry"]);
        assert_eq!(tree.nodes.as_slice()[registry_index].definition, definition);
        if registry.is_empty() {
            let token = at(&tree, &["registry", "token"]);
            assert!(matches!(
                tree.nodes.as_slice()[token].kind,
                Some(Kind::List(_))
            ));
            assert_eq!(tree.nodes.as_slice()[token].definition, definition);
        } else {
            // Normalization creates no wrapper for any occupied registry type.
            assert_eq!(tree.nodes.len(), 4);
        }
    }
}

#[test]
fn overlays_only_credentials_and_starts_each_merge_key_empty() {
    let ordinary = document::parse("untouched = 'base'\n[registry]\ncredential-provider = ['base']\n[doc]\nbrowser = ['old']\n", 80, &mut reference::Account).unwrap();
    let credentials = document::parse("new = 'credential'\n[registry]\ncredential-provider = ['credential']\n[doc]\nbrowser = ['new']\n", 80, &mut reference::Account).unwrap();
    let base = tree(&ordinary, Definition::BuiltIn);
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let overlay = tree(&credentials, Definition::BuiltIn)
        .normalize_credentials(&mut session)
        .unwrap()
        .merge_credentials(&base, &mut session)
        .unwrap();
    let Some(Kind::Table(root)) = overlay.nodes.as_slice()[0].kind.as_ref() else {
        panic!()
    };
    assert!(root.get("untouched", &mut session).unwrap().is_none());
    for path in [["registry", "credential-provider"], ["doc", "browser"]] {
        let index = at(&overlay, &path);
        let Some(Kind::List(list)) = overlay.nodes.as_slice()[index].kind.as_ref() else {
            panic!()
        };
        assert_eq!(list.len(), 2, "selected-root name must not enter merge key");
    }
    assert!(matches!(
        base.nodes.as_slice()[at(&base, &["untouched"])].kind,
        Some(Kind::String("base"))
    ));
}

#[test]
fn unmatched_ordinary_subtrees_are_not_visited_or_copied() {
    let ordinary = document::parse(
        "untouched = 'base'\nmatched = 'old'",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let credentials = document::parse("matched = 'new'", 80, &mut reference::Account).unwrap();
    let mut base = tree(&ordinary, Definition::BuiltIn);
    let untouched = at(&base, &["untouched"]);
    // An unreachable poison node detects any full-arena/full-root copy.
    base.nodes.as_mut_slice()[untouched].kind = None;
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let overlay = tree(&credentials, Definition::BuiltIn)
        .merge_credentials(&base, &mut session)
        .unwrap();
    assert_eq!(overlay.nodes.len(), 3);
    assert!(matches!(
        overlay.nodes.as_slice()[at(&overlay, &["matched"])].kind,
        Some(Kind::String("new"))
    ));
}

#[test]
fn every_transform_refusal_latches_before_further_work() {
    let ordinary =
        document::parse("[registry]\nvalue = ['old']\n", 80, &mut reference::Account).unwrap();
    let credentials = document::parse(
        "token = 'secret'\n[registry]\nvalue = ['new']\n",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let base = tree(&ordinary, Definition::BuiltIn);
    let mut complete = Account::default();
    tree(&credentials, Definition::BuiltIn)
        .normalize_credentials(&mut Session::new(&mut complete))
        .unwrap()
        .merge_credentials(&base, &mut Session::new(&mut complete))
        .unwrap();
    for refusal in 0..complete.requests {
        let mut account = Account {
            requests: 0,
            refusal: Some(refusal),
        };
        let mut session = Session::new(&mut account);
        let result = tree(&credentials, Definition::BuiltIn)
            .normalize_credentials(&mut session)
            .and_then(|tree| tree.merge_credentials(&base, &mut session));
        assert!(matches!(result, Err(Stop::Refused)));
        assert!(matches!(session.check(), Err(Stop::Refused)));
        assert!(matches!(session.items(1), Err(Stop::Refused)));
        assert_eq!(account.requests, refusal + 1);
    }
}

#[test]
fn deeply_nested_copy_and_cleanup_use_flat_storage() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let mut base = Tree::empty(Definition::BuiltIn, &mut session).unwrap();
    let mut parent = 0;
    for _ in 0..12_000 {
        let child = base
            .nodes
            .push(
                Node {
                    kind: Some(Kind::Table(table::Table::new())),
                    definition: Definition::BuiltIn,
                    span: None,
                },
                &mut session,
            )
            .unwrap();
        let Some(Kind::Table(table)) = base.nodes.as_mut_slice()[parent].kind.as_mut() else {
            panic!()
        };
        table.insert("nested", child, &mut session).unwrap();
        parent = child;
    }
    let document = document::parse("nested = {}", 80, &mut reference::Account).unwrap();
    let overlay = tree(&document, Definition::BuiltIn)
        .merge_credentials(&base, &mut session)
        .unwrap();
    assert_eq!(overlay.nodes.len(), 12_002);
    drop(overlay);
    drop(base);
}
