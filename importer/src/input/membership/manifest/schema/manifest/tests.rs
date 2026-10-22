//! Complete Original comparisons with the executed 130-case pinned Cargo run.
//!
//! The reference hashes and transport contract are retained in
//! `importer/reference/manifest/{README.md,provenance.json}`. Every accepted
//! typed field is compared, including caller-populated unused keys; rejected
//! rows compare the first phase, bounded category and applicable source span.

mod errors;
mod project;

use rawr_toml::document;
use serde_json::{json, Value};

use crate::input::membership::manifest::{reference, session, value, Error};

use super::Original;
use project::Project;

#[test]
fn complete_original_matches_every_pinned_manifest_field_and_rejection() {
    let mut accepted = 0;
    let mut typed_rejections = 0;
    let mut document_rejections = 0;
    for row in reference::observations() {
        let id = reference::id(&row);
        let source = row["source"].as_str().expect("reference source");
        let mut account = reference::Account;
        let document = match document::parse(source, 80, &mut account) {
            Ok(document) => {
                assert_ne!(
                    row["error"]["phase"], "document",
                    "{id}: document accepted unexpectedly"
                );
                document
            }
            Err(document::Error::Invalid(diagnostic)) => {
                assert_eq!(
                    row["accepted"], false,
                    "{id}: unexpected document rejection"
                );
                assert_eq!(
                    row["error"]["phase"], "document",
                    "{id}: document-first rejection"
                );
                assert_eq!(
                    diagnostic.tag,
                    document::Tag::Syntax,
                    "{id}: document error category"
                );
                assert_eq!(
                    json!(diagnostic.span.map(|span| [span.start, span.end])),
                    row["error"]["span"],
                    "{id}: document first-error span",
                );
                document_rejections += 1;
                continue;
            }
            Err(error) => panic!("{id}: unexpected document resource error: {error:?}"),
        };
        let mut session = session::Session::new(&mut account);
        let result = Original::interpret(value::Input::root(&document), &mut session);
        match session.finish(result) {
            Ok(original) => {
                assert_eq!(row["accepted"], true, "{id}: accepted a pinned rejection");
                assert_eq!(
                    original.project(id),
                    row["typed"],
                    "{id}: complete typed manifest"
                );
                assert_eq!(
                    json!(original.unused().ordered().collect::<Vec<_>>()),
                    row["unused_callbacks"],
                    "{id}: independent ordered callbacks",
                );
                assert_eq!(
                    json!(original.unused().unique().collect::<Vec<_>>()),
                    row["observed_unused"],
                    "{id}: caller-populated unused set",
                );
                accepted += 1;
            }
            Err(Error::Invalid(diagnostic)) => {
                assert_eq!(
                    row["accepted"], false,
                    "{id}: rejected an accepted manifest"
                );
                errors::compare(&row, diagnostic);
                typed_rejections += 1;
            }
            Err(error) => panic!("{id}: unexpected terminal error: {error:?}"),
        }
    }
    assert_eq!(
        (accepted, typed_rejections, document_rejections),
        (80, 48, 2)
    );
}

#[test]
fn full_projection_keeps_aliases_and_callback_order_distinct() {
    let rows = reference::observations();
    for id in [
        "alias-same-order-a",
        "alias-same-order-b",
        "unknown-order",
        "complete-families",
    ] {
        let row = rows.iter().find(|row| reference::id(row) == id).unwrap();
        let mut account = reference::Account;
        let document = reference::parse(row, &mut account);
        let mut session = session::Session::new(&mut account);
        let result = Original::interpret(value::Input::root(&document), &mut session);
        let original = session
            .finish(result)
            .unwrap_or_else(|error| panic!("{id}: {error:?}"));
        let output = original.project(id);
        assert_eq!(
            output["fields"].as_object().unwrap().len(),
            23,
            "{id}: all22 fields plus unused"
        );
        if id.starts_with("alias-same-order-") {
            assert_ne!(
                output["fields"]["dev_dependencies"],
                output["fields"]["dev_dependencies2"]
            );
            assert_eq!(
                output["fields"]["dev_dependencies"],
                row["typed"]["fields"]["dev_dependencies"]
            );
            assert_eq!(
                output["fields"]["dev_dependencies2"],
                row["typed"]["fields"]["dev_dependencies2"]
            );
        }
        if id == "unknown-order" {
            assert_eq!(original.unused().ordered().collect::<Vec<_>>(), ["z", "a"]);
            assert_eq!(original.unused().unique().collect::<Vec<_>>(), ["a", "z"]);
            assert_eq!(output["fields"]["_unused_keys"], json!(["a", "z"]));
        }
        assert_eq!(output, row["typed"], "{id}: complete structural comparison");
        assert_ne!(output, Value::Null);
    }
}
