//! Cargo util/toml/mod.rs:2678–2763, including warning and first-error order.

use rawr_toml::document;

use super::super::{schema, session, Diagnostic, Step, Tag};
use super::Warnings;

const SUPPORTED: [&str; 4] = ["cargo", "clippy", "rust", "rustdoc"];

/// Values must come from the eventual observed Cargo configuration.
#[derive(Clone, Copy)]
pub(super) struct Context {
    pub(super) cargo_lints: bool,
    pub(super) nightly_features_allowed: bool,
}

pub(super) fn verify<A: document::Admission>(
    table: Option<&schema::lints::Table<'_>>,
    context: Context,
    warnings: &mut Warnings,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    session.check()?;
    let Some(table) = table else {
        return Ok(());
    };
    for (tool, rules) in table.tools() {
        let tool = tool.text.as_str();
        if !SUPPORTED.contains(&tool) {
            warnings.push(&[
                "unrecognized lint tool `lints.", tool,
                "`, specifying unrecognized tools may break in the future.\nsupported tools: cargo, clippy, rust, rustdoc",
            ], session)?;
            continue;
        }
        if tool == "cargo" && !context.cargo_lints {
            warnings.push(&[
                "unused manifest key `lints.cargo` (may be supported in a future version)",
                if context.nightly_features_allowed {
                    "\n\nconsider passing `-Zcargo-lints` to enable this feature."
                } else {
                    "\n\nthis Cargo does not support nightly features, but if you\nswitch to nightly channel you can pass\n`-Zcargo-lints` to enable this feature."
                },
            ], session)?;
        }
        for (name, rule) in rules.rules() {
            let text = name.text.as_str();
            if let Some((prefix, _suffix)) = text.split_once("::") {
                return session.reject(Diagnostic {
                    tag: Tag::LintName,
                    span: name.span,
                    expected: Some(
                        if tool == prefix || tool == "rust" && SUPPORTED.contains(&prefix) {
                            "unprefixed lint in its tool table"
                        } else {
                            "lint name without ::"
                        },
                    ),
                });
            }
            if let schema::lints::Lint::Config(config) = rule {
                for (key, _) in config.extras() {
                    let key = key.text.as_str();
                    if (tool, text, key) != ("rust", "unexpected_cfgs", "check-cfg") {
                        warnings.push(
                            &[
                                "unused manifest key: `lints.",
                                tool,
                                ".",
                                text,
                                ".",
                                key,
                                "`",
                            ],
                            session,
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}
