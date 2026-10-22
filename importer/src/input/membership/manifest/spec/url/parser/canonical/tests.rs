use rawr_toml::document;

use super::{Canonical, Url};
use crate::input::membership::manifest::{session::Session, Error, Step, Stop};

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

// Source-shaped pinned Cargo CanonicalUrl::new, using the unchanged pinned URL
// dependency as differential reference for its parser and mutation operations.
fn reference(mut url: ::url::Url) -> Result<String, String> {
    if url.cannot_be_a_base() {
        return Err(format!(
            "invalid url `{url}`: cannot-be-a-base-URLs are not supported"
        ));
    }
    if url.path().ends_with('/') {
        url.path_segments_mut().unwrap().pop_if_empty();
    }
    if !url.scheme().contains('+') {
        if url.host_str() == Some("github.com") {
            url = format!("https{}", &url[::url::Position::AfterScheme..])
                .parse()
                .unwrap();
            let path = url.path().to_lowercase();
            url.set_path(&path);
        }
        if url.path().ends_with(".git") {
            let last = url.path_segments().unwrap().next_back().unwrap();
            let last = last[..last.len() - 4].to_owned();
            url.path_segments_mut().unwrap().pop().push(&last);
        }
    }
    Ok(url.as_str().to_owned())
}

fn compare(source: &str) {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    let original = Url::parse(source, &mut session).unwrap().unwrap();
    let original_text = original.as_str(&mut session).unwrap().to_owned();
    let actual = Canonical::from(&original, &mut session)
        .unwrap()
        .map(|url| url.as_str(&mut session).unwrap().to_owned());
    assert_eq!(
        actual,
        reference(::url::Url::parse(source).unwrap()),
        "{source:?}"
    );
    assert_eq!(original.as_str(&mut session).unwrap(), original_text);
    session.finish(Ok(())).unwrap();
}

#[test]
fn canonical_parser_and_setters_match_pinned_url_with_fetch_url_preserved() {
    for scheme in [
        "x",
        "git",
        "ssh",
        "http",
        "https",
        "ftp",
        "file",
        "ws",
        "wss",
        "git+https",
    ] {
        for authority in [
            "github.com",
            "GITHUB.COM",
            "github.com:443",
            "u:p@github.com:65535",
            "elsewhere",
        ] {
            for suffix in [
                "",
                "/",
                "//",
                "/A.GIT",
                "/A.git/",
                "/A.git//",
                "/.git",
                "/..git",
                "/...git",
                "/%2e.git",
                "/a%2Fb.git",
                "/a\\B.git",
                "/A/../B.git",
                "/ΣΟΣ/İ.git",
                "/A.git?q='X'#Frag",
            ] {
                let source = format!("{scheme}://{authority}{suffix}");
                if ::url::Url::parse(&source).is_ok() {
                    compare(&source);
                }
            }
        }
    }
    for source in [
        "x:opaque",
        "x:",
        "mailto:a@b",
        "x:/a.git",
        "x:/.//a.git",
        "file:///C:/.git",
        "file:///C:git.git",
        "x://github.com/A\\B.git?x='a'",
        "x://[::1]/foo.git",
    ] {
        compare(source);
    }
}

#[test]
fn canonical_native_file_url_does_not_reparse_original_path() {
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    for path in ["/tmp/a/../repo.git", "/tmp/a%2e.git", "/tmp/Σ.git", "/"] {
        let original = Url::from_file_path(std::path::Path::new(path), &mut session)
            .unwrap()
            .unwrap();
        let reference_url = ::url::Url::from_file_path(path).unwrap();
        let actual = Canonical::from(&original, &mut session).unwrap().unwrap();
        assert_eq!(
            actual.as_str(&mut session).unwrap(),
            reference(reference_url).unwrap(),
            "{path}"
        );
    }
    session.finish(Ok(())).unwrap();
}

fn exercise(session: &mut Session<'_, Account>) -> Step<()> {
    for source in ["git://u:p@github.com:443/A%2fΣ.git/?q='x'#F", "x:opaque"] {
        let original = Url::parse(source, session)?.unwrap();
        let canonical = Canonical::from(&original, session)?;
        if let Ok(canonical) = canonical {
            canonical.as_str(session)?;
        }
    }
    Ok(())
}

#[test]
fn every_refusal_is_terminal_and_preserves_original_account_history() {
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
            Url::parse("https://next/", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests, full.requests[..=refusal]);
    }
}
