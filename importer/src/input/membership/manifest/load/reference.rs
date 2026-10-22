//! Actual pinned Cargo constructor observations, not simulated output.

use std::path::Path;

use rawr_toml::document;

use super::{features, workspace, Context};
use crate::input::membership::manifest::{
    reference, schema::manifest::Original, session, value, Stop, Tag,
};

#[test]
fn initial_configuration_matches_every_executed_cargo_feature_case() {
    let rows = include_str!("../../../../../reference/manifest/feature-observations.jsonl");
    let mut count = 0;
    let mut accepted = 0;
    for line in rows.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let source = row["source"].as_str().unwrap();
        let document = document::parse(source, 80, &mut reference::Account).unwrap();
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let original = Original::interpret(value::Input::root(&document), &mut session).unwrap();
        let allow: Option<Vec<&str>> = row["allow"]
            .as_array()
            .map(|values| values.iter().map(|v| v.as_str().unwrap()).collect());
        let channel = row["channel"].as_str().unwrap();
        let context = Context {
            features: features::Context {
                nightly_features_allowed: matches!(channel, "nightly" | "dev"),
                is_local: true,
                channel,
                allow_features: allow.as_deref(),
            },
            cargo_lints: false,
            profiles: crate::input::membership::manifest::load::convert::profiles::Context::fixture(
            ),
        };
        let result = super::configure(
            &original,
            Path::new("/fixture/Cargo.toml"),
            false,
            context,
            &mut session,
        );
        match result {
            Ok(configured) => {
                assert!(row["tag"].is_null(), "{}", row["name"]);
                assert!(matches!(
                    configured.workspace,
                    workspace::Configuration::Root { .. }
                ));
                let activated = configured.features.activated(&mut session).unwrap();
                let actual: Vec<_> = activated.iter().map(|text| text.as_str()).collect();
                let expected: Vec<_> = row["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                assert_eq!(actual, expected, "{}", row["name"]);
                accepted += 1;
            }
            Err(Stop::Rejected(error)) => {
                let Tag::Feature(invalid) = error.tag else {
                    panic!("{}: {:?}", row["name"], error);
                };
                // Categories are independent executable observations. Associated
                // typed feature/index payloads have dedicated constructor tests.
                let category = match invalid {
                    features::Invalid::Unknown { .. } => "Unknown",
                    features::Invalid::UnknownUnderscore { .. } => "UnknownUnderscore",
                    features::Invalid::UnknownCli { .. } => "UnknownCli",
                    features::Invalid::Duplicate { .. } => "Duplicate",
                    features::Invalid::Channel { .. } => "Channel",
                    features::Invalid::Denied { .. } => "Denied",
                    features::Invalid::Removed { .. } => "Removed",
                    features::Invalid::Required { .. } => {
                        panic!("constructor returned a later require error")
                    }
                };
                assert_eq!(Some(category), row["tag"].as_str(), "{}", row["name"]);
            }
            Err(Stop::Refused) => panic!("unexpected unlimited fixture refusal: {}", row["name"]),
        }
        count += 1;
    }
    assert_eq!((count, accepted), (336, 158));
}
