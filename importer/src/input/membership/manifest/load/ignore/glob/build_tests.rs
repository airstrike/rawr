use super::*;
use crate::input::membership::manifest::reference::Account;

#[test]
fn prepared_rules_construct_globs_before_classification() {
    for (line, actual, regex, expected) in [
        (
            "*.rs",
            "**/*.rs",
            "(?-u)^(?:/?|.*/)[^/]*\\.rs$",
            "extension",
        ),
        ("!/cache/", "cache", "(?-u)^cache$", "literal"),
        ("src/**", "src/**/*", "(?-u)^src(?:/|/.*/)[^/]*$", "regex"),
    ] {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let prepared = super::super::rule::prepare(line, &mut session)
            .unwrap()
            .unwrap();
        let Ok(built) = prepared.build(&mut session).unwrap() else {
            panic!("build");
        };
        assert_eq!(built.glob(), actual);
        assert_eq!(built.regex(), regex.as_bytes());
        let kind = match built.parsed.strategy(&mut session).unwrap() {
            strategy::Strategy::Extension(_) => "extension",
            strategy::Strategy::Literal(_) => "literal",
            strategy::Strategy::Regex => "regex",
            _ => "other",
        };
        assert_eq!(kind, expected);
    }
}

struct Trace {
    calls: usize,
    refuse: usize,
}
impl Admission for Trace {
    type Error = ();
    fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
        let call = self.calls;
        self.calls += 1;
        if call == self.refuse {
            Err(())
        } else {
            Ok(())
        }
    }
}
#[test]
fn complete_build_keeps_failures_and_refusal_terminal() {
    for pattern in ["a{b,{é,},}*", "[z-a]", "{unclosed"] {
        let mut trace = Trace {
            calls: 0,
            refuse: usize::MAX,
        };
        let result = build(pattern, &mut Session::new(&mut trace)).unwrap();
        match (pattern, result) {
            ("[z-a]", Err(failure)) => {
                assert_eq!(failure.glob, pattern);
                assert_eq!(failure.kind, Kind::InvalidRange('z', 'a'));
            }
            ("{unclosed", Err(failure)) => {
                assert_eq!(failure.glob, pattern);
                assert_eq!(failure.kind, Kind::UnclosedAlternates);
            }
            ("a{b,{é,},}*", Ok(_)) => {}
            _ => panic!("unexpected build result"),
        }
        for refuse in 0..trace.calls {
            let mut trace = Trace { calls: 0, refuse };
            let mut session = Session::new(&mut trace);
            assert!(matches!(build(pattern, &mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(trace.calls, refuse + 1);
        }
    }
}
