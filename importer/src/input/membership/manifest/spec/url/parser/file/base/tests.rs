use super::{BaseError, Url};
use crate::input::membership::manifest::{session::Session, Error, Step, Stop};
use rawr_toml::document;
use std::path::Path;

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn compare(source: &str, path: &Path) {
    let first = ::url::Url::parse(source);
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let accepted = Url::accepted(source, &mut session).unwrap();
    match (accepted, first) {
        (Err(actual), Err(expected)) => assert_eq!(actual.to_string(), expected.to_string()),
        (Ok(accepted), Ok(first)) => {
            assert_eq!(
                accepted
                    .url(&mut session)
                    .unwrap()
                    .as_str(&mut session)
                    .unwrap(),
                first.as_str()
            );
            let base = ::url::Url::from_file_path(path).unwrap();
            let expected = ::url::Url::options().base_url(Some(&base)).parse(source);
            let actual = accepted.with_file_base(path, &mut session).unwrap();
            match (actual, expected) {
                (Ok(actual), Ok(expected)) => assert_eq!(
                    actual.as_str(&mut session).unwrap(),
                    expected.as_str(),
                    "{source:?}, {path:?}"
                ),
                (Err(BaseError::Url(actual)), Err(expected)) => assert_eq!(
                    actual.to_string(),
                    expected.to_string(),
                    "{source:?}, {path:?}"
                ),
                (Err(actual), Ok(expected)) => {
                    panic!("{source:?}, {path:?}: rejected {actual:?}, expected {expected}")
                }
                (Ok(actual), Err(expected)) => panic!(
                    "{source:?}, {path:?}: got {}, expected {expected}",
                    actual.as_str(&mut session).unwrap()
                ),
                (Err(BaseError::Path), Err(expected)) => {
                    panic!("unexpected base error: {expected}")
                }
            }
        }
        (Err(actual), Ok(expected)) => {
            panic!("first rejected {source:?}: {actual}, expected {expected}")
        }
        (Ok(_), Err(expected)) => panic!("first accepted {source:?}, expected {expected}"),
    }
    session.finish(Ok(())).unwrap();
}

#[test]
fn explicit_registry_second_parse_matches_pinned_native_file_base() {
    for path in [
        "/tmp/config/truncated-by-url_with_base",
        "/C:/config/truncated-by-url_with_base",
        "/C:/",
        "/tmp/../A%2e/Σ/file",
        "/",
    ] {
        for source in [
            "file:",
            "file:?q='x'#f",
            "file:#f",
            "file:relative",
            "file:../other",
            "file:./other",
            "file:/other",
            "file:\\other",
            "file://other/share",
            "file:///other",
            "file:////other",
            "file:C:/other",
            "file:C|/other",
            "file:/D:/other",
            "file:/D|/other",
            "file:C:",
            "file:C:?q",
            "file:C#f",
            "file:C\\other",
            "file:C\t:\n/other",
            "file:/C\t:\n/other",
            "file:%2e%2e/other",
            "file:space and Σ?q#f",
            " \tFiLe:../x\n ",
            "https:example.com/A",
            "x:opaque",
            "x:/a",
            "git://github.com/X",
            "git@github.com:x",
            "relative",
            "?q",
            "#f",
        ] {
            compare(source, Path::new(path));
        }
    }
}

#[test]
fn native_base_is_constructed_before_non_file_second_parse() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let accepted = Url::accepted("https://example.com/index", &mut session)
        .unwrap()
        .unwrap();
    assert!(matches!(
        accepted
            .with_file_base(Path::new("relative-base"), &mut session)
            .unwrap(),
        Err(BaseError::Path)
    ));
    session.finish(Ok(())).unwrap();
}

#[test]
fn native_non_utf8_base_and_parent_components_are_not_reparsed() {
    use std::os::unix::ffi::OsStrExt;
    let path = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/\xff/%2e/../base"));
    for source in [
        "file:",
        "file:?q",
        "file:#f",
        "file:next",
        "file:../next",
        "https://example.com",
    ] {
        compare(source, path);
    }
}

fn exercise(session: &mut Session<'_, Account>) -> Step<()> {
    for source in ["file:/other?q#f", "file:../next", "https://example.com"] {
        let accepted = Url::accepted(source, session)?.unwrap();
        accepted.url(session)?.as_str(session)?;
        let second = accepted
            .with_file_base(Path::new("/C:/root/../config/base"), session)?
            .unwrap();
        second.as_str(session)?;
    }
    Ok(())
}

#[test]
fn all_second_parse_admissions_keep_original_account_and_terminal_refusal() {
    let prior = vec![document::Request::Items(7), document::Request::String(13)];
    let mut full = Account {
        requests: prior.clone(),
        refusal: None,
    };
    let mut session = Session::new(&mut full);
    let result = exercise(&mut session);
    session.finish(result).unwrap();
    for refusal in prior.len()..full.requests.len() {
        let mut account = Account {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        let mut session = Session::new(&mut account);
        assert!(
            matches!(exercise(&mut session), Err(Stop::Refused)),
            "{refusal}"
        );
        assert!(matches!(
            Url::accepted("https://next/", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests, full.requests[..=refusal]);
    }
}
