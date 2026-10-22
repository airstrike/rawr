use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};

#[test]
fn rule_collection_keeps_order_and_failed_rule_out_of_both_lists() {
    let mut rules = Rules::new();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    for line in ["#comment", " ", "*.rs", "!/cache/"] {
        assert!(rules.add(line, &mut session).unwrap().is_ok());
    }
    for (line, expected) in [
        ("[z-a]", "invalid range; 'z' > 'a'"),
        ("oops\\", "dangling '\\'"),
        (
            "}",
            "unopened alternate group; missing '{' (maybe escape '}' with '[}]'?)",
        ),
        (
            "{",
            "unclosed alternate group; missing '}' (maybe escape '{' with '[{]'?)",
        ),
    ] {
        let Err(failure) = rules.add(line, &mut session).unwrap() else {
            panic!("missing error");
        };
        assert_eq!(failure.original, line);
        assert_eq!(failure.error, expected);
        assert_eq!(rules.patterns.len(), 2);
        assert_eq!(rules.originals.len(), 2);
    }
    assert!(rules.add("src/**", &mut session).unwrap().is_ok());
    assert_eq!(rules.patterns.len(), 3);
    assert_eq!(rules.originals.len(), 3);
    for (index, (original, actual, whitelist, directory)) in [
        ("*.rs", "**/*.rs", false, false),
        ("!/cache/", "cache", true, true),
        ("src/**", "src/**/*", false, false),
    ]
    .into_iter()
    .enumerate()
    {
        let prepared = &rules.originals.as_slice()[index];
        assert_eq!(
            (
                &*prepared.original,
                &*prepared.actual,
                prepared.whitelist,
                prepared.directory
            ),
            (original, actual, whitelist, directory)
        );
        assert_eq!(rules.patterns.as_slice()[index].glob(), actual);
    }
}
struct Count {
    calls: usize,
    refuse: usize,
}
impl Admission for Count {
    type Error = ();
    fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
        let position = self.calls;
        self.calls += 1;
        if position == self.refuse {
            Err(())
        } else {
            Ok(())
        }
    }
}
#[test]
fn refusal_during_add_prevents_any_subsequent_rule_work() {
    for line in ["*.é", "[z-a]", "!/cache/"] {
        let mut complete = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        let _ = Rules::new()
            .add(line, &mut Session::new(&mut complete))
            .unwrap();
        for refuse in 0..complete.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            let mut rules = Rules::new();
            assert!(matches!(rules.add(line, &mut session), Err(Stop::Refused)));
            assert!(matches!(
                rules.add("later", &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}

#[test]
fn builder_owns_root_before_collecting_rules() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = Builder::prepare(Path::new("./root"), &mut session).unwrap();
    assert!(builder.add("*.rs", &mut session).unwrap().is_ok());
    assert!(builder.add("!/cache/", &mut session).unwrap().is_ok());
    assert_eq!(builder.rules.patterns.len(), 2);
    assert_eq!(
        builder
            .root
            .strip(Path::new("./root/file.rs"), &mut session)
            .unwrap(),
        Path::new("file.rs")
    );
}

#[test]
fn builder_refusal_covers_root_and_rule_collection_together() {
    fn collect<A: Admission>(
        session: &mut Session<'_, A>,
    ) -> crate::input::membership::manifest::Step<()> {
        let mut builder = Builder::prepare(Path::new("./root"), session)?;
        let result = builder.add("*.rs", session)?;
        assert!(result.is_ok());
        Ok(())
    }
    let mut complete = Count {
        calls: 0,
        refuse: usize::MAX,
    };
    collect(&mut Session::new(&mut complete)).unwrap();
    for refuse in 0..complete.calls {
        let mut count = Count { calls: 0, refuse };
        let mut session = Session::new(&mut count);
        assert!(matches!(collect(&mut session), Err(Stop::Refused)));
        assert!(matches!(session.check(), Err(Stop::Refused)));
        assert_eq!(count.calls, refuse + 1);
    }
}
