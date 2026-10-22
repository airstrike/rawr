// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Cargo 59800466 util/toml/mod.rs verify_lints, after source consistency.
use crate::input::membership::manifest::{
    load::{normalize::target, Warnings},
    schema::lints,
    session::Session,
    Step,
};
use rawr_toml::document;
pub(in super::super) mod check_cfg;
pub(in super::super) mod flags;

/// Full lint verification and compiler flags, after dependency source checks.
pub(in super::super) fn convert<A: document::Admission>(
    table: Option<&lints::Table<'_>>,
    cargo_lints: bool,
    nightly: bool,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<flags::Flags, String>> {
    if let Err(error) = verify(table, cargo_lints, nightly, warnings, session)? {
        return Ok(Err(error));
    }
    let flags = flags::order(table, session)?.into_flags(session)?;
    if let Some(table) = table {
        for (tool, rules) in table.tools() {
            session.items(1)?;
            session.string(tool.text.as_str().len())?;
            if tool.text.as_str() != "rust" {
                continue;
            }
            for (name, config) in rules.rules() {
                session.items(1)?;
                session.string(name.text.as_str().len())?;
                if name.text.as_str() != "unexpected_cfgs" {
                    continue;
                }
                if let lints::Lint::Config(config) = config {
                    for (key, value) in config.extras() {
                        session.items(1)?;
                        session.string(key.text.as_str().len())?;
                        if key.text.as_str() == "check-cfg" {
                            let owned = super::value::copy(value, session)?;
                            let values = match owned.strings(session)? {
                                Ok(values) => values,
                                Err(()) => return check_cfg::invalid(session),
                            };
                            return flags.append(values, session).map(Ok);
                        }
                    }
                }
                break;
            }
            break;
        }
    }
    Ok(Ok(flags))
}

pub(in super::super) fn verify<A: document::Admission>(
    table: Option<&lints::Table<'_>>,
    cargo_lints: bool,
    nightly: bool,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.check()?;
    let Some(table) = table else {
        return Ok(Ok(()));
    };
    let supported = ["cargo", "clippy", "rust", "rustdoc"];
    for (tool, rules) in table.tools() {
        session.items(1)?;
        let tool = tool.text.as_str();
        session.string(tool.len())?;
        if !supported.contains(&tool) {
            warnings.push(&["unrecognized lint tool `lints.", tool, "`, specifying unrecognized tools may break in the future.\nsupported tools: cargo, clippy, rust, rustdoc"], session)?;
            continue;
        }
        if tool == "cargo" && !cargo_lints {
            let advice = if nightly {
                "\n\nconsider passing `-Zcargo-lints` to enable this feature."
            } else {
                "\n\nthis Cargo does not support nightly features, but if you\nswitch to nightly channel you can pass\n`-Zcargo-lints` to enable this feature."
            };
            warnings.push(
                &[
                    "unused manifest key `lints.cargo` (may be supported in a future version)",
                    advice,
                ],
                session,
            )?;
        }
        for (name, config) in rules.rules() {
            session.items(1)?;
            let name = name.text.as_str();
            session.string(name.len())?;
            if let Some((prefix, suffix)) = name.split_once("::") {
                return if tool == prefix || (tool == "rust" && supported.contains(&prefix)) {
                    target::formatted(format_args!("`lints.{tool}.{name}` is not valid lint name; try `lints.{prefix}.{suffix}`"), session).map(Err)
                } else {
                    target::formatted(
                        format_args!("`lints.{tool}.{name}` is not a valid lint name"),
                        session,
                    )
                    .map(Err)
                };
            }
            if let lints::Lint::Config(config) = config {
                for (key, _) in config.extras() {
                    session.items(1)?;
                    let key = key.text.as_str();
                    session.string(key.len())?;
                    if !(tool == "rust" && name == "unexpected_cfgs" && key == "check-cfg") {
                        warnings.push(
                            &[
                                "unused manifest key: `lints.",
                                tool,
                                ".",
                                name,
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
    Ok(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, value};
    #[derive(Default)]
    struct Account {
        calls: usize,
        refusal: Option<usize>,
    }
    impl document::Admission for Account {
        type Error = ();
        fn admit(&mut self, _: document::Request) -> Result<(), ()> {
            let index = self.calls;
            self.calls += 1;
            if self.refusal == Some(index) {
                Err(())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn complete_lint_conversion_refusal_is_terminal_at_every_admission() {
        use crate::input::membership::manifest::Stop;
        for value in ["['cfg(x)',1979-05-27]", "['cfg(x)',{deep=['later']}]"] {
            let text = format!("[lints.rust.unexpected_cfgs]\nlevel='warn'\ncheck-cfg={value}");
            let mut source = reference::Account;
            let document = document::parse(&text, 100, &mut source).unwrap();
            let mut session = Session::new(&mut source);
            let table = lints::Table::interpret(
                value::Input::raw(document.table().get("lints").unwrap()),
                &mut session,
            )
            .unwrap();
            let mut baseline = Account::default();
            let mut session = Session::new(&mut baseline);
            let _ = convert(Some(&table), true, true, &mut Warnings::new(), &mut session).unwrap();
            for refusal in 0..baseline.calls {
                let mut account = Account {
                    refusal: Some(refusal),
                    ..Account::default()
                };
                let mut session = Session::new(&mut account);
                assert!(matches!(
                    convert(Some(&table), true, true, &mut Warnings::new(), &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(
                    convert(None, true, true, &mut Warnings::new(), &mut session),
                    Err(Stop::Refused)
                ));
                assert_eq!(account.calls, refusal + 1);
            }
        }
    }
    #[test]
    fn complete_lints_convert_flags_and_check_cfg_after_verification() {
        for (check, valid) in [
            ("['cfg(x)',1979-05-27,'cfg(x)']", true),
            ("['cfg(x)',{nested=['later']} ]", false),
        ] {
            let source = format!("[lints.rust]\nunused='deny'\n[lints.rust.unexpected_cfgs]\nlevel='warn'\npriority=-1\ncheck-cfg={check}");
            let mut account = reference::Account;
            let document = document::parse(&source, 100, &mut account).unwrap();
            let mut session = Session::new(&mut account);
            let table = lints::Table::interpret(
                value::Input::raw(document.table().get("lints").unwrap()),
                &mut session,
            )
            .unwrap();
            let mut warnings = Warnings::new();
            let result = convert(Some(&table), false, true, &mut warnings, &mut session).unwrap();
            assert!(warnings.as_slice().is_empty());
            if valid {
                assert_eq!(
                    result.ok().unwrap().as_slice(),
                    [
                        "--warn=unexpected_cfgs",
                        "--deny=unused",
                        "--check-cfg",
                        "cfg(x)",
                        "--check-cfg",
                        "1979-05-27",
                        "--check-cfg",
                        "cfg(x)"
                    ]
                );
            } else {
                assert_eq!(
                    result.err().unwrap(),
                    "`lints.rust.unexpected_cfgs.check-cfg` must be a list of string"
                );
            }
        }
    }
    #[test]
    fn lint_verification_preserves_tool_warning_and_name_error_order() {
        for nightly in [false, true] {
            let text = "[lints.aaa]\n'bad::name'='warn'\n[lints.cargo]\nfoo='warn'\n[lints.rust]\n'clippy::bad'='deny'";
            let mut account = reference::Account;
            let document = document::parse(text, 100, &mut account).unwrap();
            let mut session = Session::new(&mut account);
            let table = lints::Table::interpret(
                value::Input::raw(document.table().get("lints").unwrap()),
                &mut session,
            )
            .unwrap();
            let mut warnings = Warnings::new();
            let error = verify(Some(&table), false, nightly, &mut warnings, &mut session)
                .unwrap()
                .unwrap_err();
            assert_eq!(
                error,
                "`lints.rust.clippy::bad` is not valid lint name; try `lints.clippy.bad`"
            );
            assert_eq!(warnings.as_slice().len(), 2);
            assert_eq!(warnings.as_slice()[0], "unrecognized lint tool `lints.aaa`, specifying unrecognized tools may break in the future.\nsupported tools: cargo, clippy, rust, rustdoc");
            assert_eq!(warnings.as_slice()[1].contains("consider passing"), nightly);
        }
    }
    #[test]
    fn expected_check_cfg_is_retained_while_other_config_warns() {
        let mut account = reference::Account;
        let document = document::parse(
            "[lints.rust.unexpected_cfgs]\nlevel='warn'\ncheck-cfg=['cfg(x)']\nunknown=true",
            100,
            &mut account,
        )
        .unwrap();
        let mut session = Session::new(&mut account);
        let table = lints::Table::interpret(
            value::Input::raw(document.table().get("lints").unwrap()),
            &mut session,
        )
        .unwrap();
        let mut warnings = Warnings::new();
        verify(Some(&table), true, true, &mut warnings, &mut session)
            .unwrap()
            .unwrap();
        assert_eq!(
            warnings.as_slice(),
            ["unused manifest key: `lints.rust.unexpected_cfgs.unknown`"]
        );
    }
}
