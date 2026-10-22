use super::*;
use crate::input::membership::manifest::reference;

#[test]
fn copied_platform_retains_links_and_raw_spelling_after_original_drops() {
    for text in [
        "",
        "x86_64-apple-darwin",
        "cfg(all(r#feature = \"x\", not(any(unix, r#target_os=\"macos\"))))",
    ] {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let original = parse(text, &mut session).unwrap().unwrap();
        let expected = original.to_string();
        let copied = original.copy(&mut session).unwrap();
        assert_eq!(copied.nodes.len(), original.nodes.len());
        for (left, right) in copied
            .nodes
            .as_slice()
            .iter()
            .zip(original.nodes.as_slice())
        {
            assert_eq!(
                (left.child, left.next, left.parent),
                (right.child, right.next, right.parent)
            );
        }
        drop(original);
        assert_eq!(copied.to_string(), expected);
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn platform_copy_refusal_is_terminal_at_every_admission() {
    #[derive(Default)]
    struct Account {
        calls: usize,
        refusal: Option<usize>,
    }
    impl document::Admission for Account {
        type Error = usize;
        fn admit(&mut self, _: document::Request) -> Result<(), usize> {
            let at = self.calls;
            self.calls += 1;
            if self.refusal == Some(at) {
                Err(at)
            } else {
                Ok(())
            }
        }
    }
    for text in [
        "x86_64-apple-darwin",
        "cfg(all(unix, not(r#feature=\"x\"), any(a,b)))",
    ] {
        let mut setup = reference::Account;
        let mut session = Session::new(&mut setup);
        let original = parse(text, &mut session).unwrap().unwrap();
        session.finish(Ok(())).unwrap();
        let mut baseline = Account::default();
        let mut session = Session::new(&mut baseline);
        let copied = original.copy(&mut session);
        session.finish(copied).unwrap();
        for refusal in 0..baseline.calls {
            let mut account = Account {
                refusal: Some(refusal),
                ..Account::default()
            };
            let mut session = Session::new(&mut account);
            let copied = original.copy(&mut session);
            assert!(copied.is_err());
            assert!(original.copy(&mut session).is_err());
            assert!(
                matches!(session.finish(copied), Err(crate::input::membership::manifest::Error::Admission(at)) if at == refusal)
            );
            assert_eq!(account.calls, refusal + 1);
        }
    }
}

fn parsed(text: &str) -> Result<String, String> {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let result = parse(text, &mut session);
    session
        .finish(result)
        .unwrap()
        .map(|platform| platform.to_string())
}

#[test]
fn pinned_success_spellings_preserve_raw_identifiers_and_normalized_display() {
    for (input, display) in [
        ("", ""),
        ("x86_64-apple-darwin", "x86_64-apple-darwin"),
        ("目标.1", "目标.1"),
        ("cfg( unix )", "cfg(unix)"),
        ("cfg(all())", "cfg(all())"),
        ("cfg(any(a,b,))", "cfg(any(a, b))"),
        (
            "cfg(not(all(unix, target_os=\"macos\")))",
            "cfg(not(all(unix, target_os = \"macos\")))",
        ),
        ("cfg(r#all)", "cfg(r#all)"),
        ("cfg(r#true)", "cfg(true)"),
        ("cfg(r#false)", "cfg(false)"),
        ("cfg(r#false=\"x\")", "cfg(r#false = \"x\")"),
        ("cfg(all(not(a),any(),b))", "cfg(all(not(a), any(), b))"),
        ("cfg(feature=\"a\\b\")", "cfg(feature = \"a\\b\")"),
    ] {
        assert_eq!(parsed(input), Ok(display.into()), "{input}");
    }
}

#[test]
fn exact_pinned_error_classes_and_inner_versus_outer_original_spelling() {
    for (input,original,cause) in [
        ("cfg()","","expected start of a cfg expression, but cfg expression ended"),
        ("cfg(all)","all","expected `(`, but cfg expression ended"),
        ("cfg(not())","not()","expected identifier, found `)`"),
        ("cfg(not(a,b))","not(a,b)","expected `)`, found `,`"),
        ("cfg(a=word)","a=word","expected a string, found an identifier"),
        ("cfg(a=)","a=","expected a string, but cfg expression ended"),
        ("cfg(a=\"x)","a=\"x","unterminated string in cfg"),
        ("cfg(r#)","r#","expected identifier, but cfg expression ended"),
        ("cfg(r#1)","r#1","unexpected character `1` in cfg, expected parens, a comma, an identifier, or a string"),
        ("cfg(a b)","a b","unexpected content `b` found after cfg expression"),
        ("cfg(a\t)","a\t","unexpected content `\t` found after cfg expression"),
        ("cfg(\t)","\t","unexpected character `\t` in cfg, expected parens, a comma, an identifier, or a string"),
        ("cfg (unix)","cfg (unix)","invalid target specifier: unexpected `(` character, cfg expressions must start with `cfg(`"),
        ("name/target","name/target","invalid target specifier: unexpected character / in target name"),
        ("cfg(unix) ","cfg(unix) ","invalid target specifier: unexpected `(` character, cfg expressions must start with `cfg(`"),
    ] {assert_eq!(parsed(input),Err(format!("failed to parse `{original}` as a cfg expression: {cause}")),"{input}");}
}

#[test]
fn warning_passes_keep_leaf_order_repetition_raw_behavior_and_keyword_text() {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let platform=parse("cfg(all(true=\"yes\",r#test,not(feature=\"x\"),debug_assertions,test,r#false=\"n\",false,proc_macro))",&mut session).unwrap().ok().unwrap();
    let mut warnings = Warnings::new();
    check_attributes(&platform, &mut warnings, &mut session).unwrap();
    check_keywords(
        &platform,
        Path::new("/p/Cargo.toml"),
        &mut warnings,
        &mut session,
    )
    .unwrap();
    session.finish(Ok(())).unwrap();
    assert_eq!(warnings.as_slice().len(), 6);
    for (index, prefix) in [
        "Found `r#test`",
        "Found `feature = ...`",
        "Found `debug_assertions`",
        "Found `test`",
        "Found `proc_macro`",
    ]
    .into_iter()
    .enumerate()
    {
        assert!(warnings.as_slice()[index].starts_with(prefix));
    }
    assert_eq!(warnings.as_slice()[5],"[/p/Cargo.toml] future-incompatibility: `cfg(true = \"yes\")` is deprecated as `true` is a keyword and not an identifier and should not have have been accepted in this position.\n | this was previously accepted by Cargo but is being phased out; it will become a hard error in a future release!\n |\n | help: use raw-idents instead: `cfg(r#true)`");
}

#[test]
fn deep_parse_display_warning_and_drop_use_flat_storage() {
    let count = 2048;
    let input = format!("cfg({}test{})", "not(".repeat(count), ")".repeat(count));
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let platform = parse(&input, &mut session).unwrap().ok().unwrap();
    assert_eq!(platform.to_string(), input);
    let mut warnings = Warnings::new();
    check_attributes(&platform, &mut warnings, &mut session).unwrap();
    assert_eq!(warnings.as_slice().len(), 1);
    session.finish(Ok(())).unwrap();
    drop(platform);
}

#[derive(Default)]
struct Account {
    calls: usize,
    refused: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, _: document::Request) -> Result<(), usize> {
        let at = self.calls;
        self.calls += 1;
        if self.refused == Some(at) {
            Err(at)
        } else {
            Ok(())
        }
    }
}
#[test]
fn refusal_at_every_reached_parser_admission_wins_over_semantic_failure() {
    for text in ["cfg(all(a,not(b)))", "cfg(all(a,not(b,)))", "bad/name"] {
        let mut baseline = Account::default();
        let mut session = Session::new(&mut baseline);
        let result = parse(text, &mut session);
        let _ = session.finish(result).unwrap();
        for refused in 0..baseline.calls {
            let mut account = Account {
                refused: Some(refused),
                ..Account::default()
            };
            let mut session = Session::new(&mut account);
            let result = parse(text, &mut session);
            assert!(result.is_err());
            assert!(parse("valid", &mut session).is_err());
            assert!(
                matches!(session.finish(result),Err(super::super::super::super::Error::Admission(at)) if at==refused)
            );
            assert_eq!(account.calls, refused + 1);
        }
    }
}

#[test]
fn cfg_parse_retains_no_eager_display_string() {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let platform = parse("cfg(all(a,b))", &mut session).unwrap().ok().unwrap();
    assert!(platform.name.is_none());
    assert_eq!(platform.nodes.len(), 3);
    session.finish(Ok(())).unwrap();
}
