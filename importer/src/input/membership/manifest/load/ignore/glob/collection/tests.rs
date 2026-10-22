use super::*;
use crate::input::membership::manifest::{
    load::ignore::paths::Candidate, reference::Account, storage::Buffer, Stop,
};
use rawr_toml::document::Request;
use std::path::Path;

#[derive(Default)]
struct Count {
    requests: Vec<Request>,
    refuse: Option<usize>,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn built(globs: &[&str]) -> Vec<Built> {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    globs
        .iter()
        .map(|glob| {
            super::super::build(glob, &mut session)
                .unwrap()
                .ok()
                .unwrap()
        })
        .collect()
}

fn ids(strategy: &literal::Strategy, path: &str) -> Vec<usize> {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let candidate = Candidate::prepare(Path::new(path), &mut session).unwrap();
    let mut found = Buffer::new();
    strategy
        .matches_into(&candidate, &mut found, &mut session)
        .unwrap();
    found.as_slice().to_vec()
}

#[test]
fn collection_keeps_all_source_buckets_component_suffix_and_owned_regexes() {
    let mut empty = Count::default();
    assert!(Collection::new(&[], &mut Session::new(&mut empty))
        .unwrap()
        .into_rules()
        .is_none());
    assert_eq!(empty.requests, [Request::Items(1)]);

    let input = built(&[
        "plain",
        "**/leaf",
        "**/*.rs",
        "prefix/**",
        "**/é/x",
        "*.rs",
        "a*b",
    ]);
    let rules = Collection::new(&input, &mut Session::new(&mut Account))
        .unwrap()
        .into_rules()
        .unwrap();
    assert_eq!(rules.len, 7);
    assert_eq!(ids(&rules.literals, "plain"), [0]);
    assert_eq!(ids(&rules.literals, "é/x"), [4]);
    assert_eq!(ids(&rules.basenames, "dir/leaf"), [1]);
    assert_eq!(ids(&rules.extensions, "dir/file.rs"), [2]);
    assert_eq!(rules.prefixes.literals.as_slice(), ["prefix/"]);
    assert_eq!(rules.prefixes.map.as_slice(), [3]);
    assert_eq!(rules.prefixes.longest, 7);
    assert_eq!(rules.suffixes.literals.as_slice(), ["/é/x"]);
    assert_eq!(rules.suffixes.map.as_slice(), [4]);
    assert_eq!(rules.suffixes.longest, "/é/x".len());
    assert_eq!(rules.regexes.map.as_slice(), [6]);
    assert_eq!(
        rules.regexes.literals.as_slice()[0].as_bytes(),
        input[6].regex()
    );
    assert_ne!(
        rules.regexes.literals.as_slice()[0].as_ptr(),
        input[6].regex().as_ptr()
    );
    assert!(!rules.required.is_empty());
    let mut groups = rules
        .required
        .into_iter(&mut Session::new(&mut Account))
        .unwrap();
    let (extension, texts) = groups
        .next(&mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_eq!(extension, b".rs");
    let mut texts = texts.into_iter(&mut Session::new(&mut Account)).unwrap();
    let (index, text) = texts
        .next(&mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_eq!(index, 5);
    assert_eq!(text.as_bytes(), input[5].regex());
    assert_ne!(text.as_ptr(), input[5].regex().as_ptr());
    assert!(texts
        .next(&mut Session::new(&mut Account))
        .unwrap()
        .is_none());
    assert!(groups
        .next(&mut Session::new(&mut Account))
        .unwrap()
        .is_none());

    // The ordinary strategy literal transfers its existing byte allocation;
    // only the safe UTF-8 validation scan is added at the representation join.
    let input = built(&["é"]);
    let strategy::Strategy::Literal(literal) = input[0]
        .parsed
        .strategy(&mut Session::new(&mut Account))
        .unwrap()
    else {
        panic!("expected literal")
    };
    let allocation = literal.bytes().as_ptr();
    let mut conversion = Count::default();
    let text = literal
        .into_string(&mut Session::new(&mut conversion))
        .unwrap();
    assert_eq!(text, "é");
    assert_eq!(text.as_ptr(), allocation);
    assert_eq!(conversion.requests, [Request::Items("é".len())]);
}

#[test]
fn collection_refusal_never_returns_partial_builders_or_consumes_borrowed_rules() {
    let input = built(&["**/é/x", "a*b"]);
    let expected: Vec<_> = input.iter().map(|glob| glob.regex().to_vec()).collect();
    let mut complete = Count::default();
    drop(Collection::new(&input, &mut Session::new(&mut complete)).unwrap());
    for refuse in 0..complete.requests.len() {
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            Collection::new(&input, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            Collection::new(&[], &mut session),
            Err(Stop::Refused)
        ));
        assert!(
            matches!(session.finish::<()>(Err(Stop::Refused)), Err(Error::Admission(cause)) if cause == refuse)
        );
        assert_eq!(count.requests, complete.requests[..=refuse]);
        for (glob, bytes) in input.iter().zip(&expected) {
            assert_eq!(glob.regex(), bytes);
        }
    }
}
