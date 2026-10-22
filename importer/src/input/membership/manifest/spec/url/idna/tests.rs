use rawr_toml::document;

use super::{Ascii, Invalid};
use crate::input::membership::manifest::{session, Error};

mod reference;
mod resources;

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}

impl document::Admission for Account {
    type Error = usize;
    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn interpret<'input>(
    input: &'input [u8],
    account: &mut Account,
) -> Result<Result<Ascii<'input>, Invalid>, Error<usize>> {
    let mut session = session::Session::new(account);
    let result = Ascii::interpret(input, &mut session);
    session.finish(result)
}

#[test]
fn unchanged_ascii_keeps_the_original_borrow_in_both_prefix_tiers() {
    for input in [
        "",
        ".",
        "..",
        "abc.example",
        "a123.example",
        "0.example",
        "_srv._tcp",
        "a-.example",
        "ab--cd",
        "xn--bcher-kva.example",
    ] {
        let mut account = Account::default();
        let value = interpret(input.as_bytes(), &mut account).unwrap().unwrap();
        assert_eq!(value.as_bytes(), input.as_bytes(), "{input}");
        assert_eq!(
            value.as_bytes().as_ptr(),
            input.as_bytes().as_ptr(),
            "{input}"
        );
        assert!(value.is_borrowed(), "{input}");
        if input == "abc.example" || input.is_empty() || input == "." || input == ".." {
            assert_eq!(
                account.requests,
                [document::Request::String(input.len())],
                "fastest path checks the input extent without new storage: {input}"
            );
        }
    }
}

#[test]
fn transformed_domains_own_the_complete_ascii_result() {
    for (input, expected) in [
        ("EXAMPLE.com", "example.com"),
        ("prefix.BÜCHER.Example", "prefix.xn--bcher-kva.example"),
        ("a\u{3002}B\u{ff0e}c\u{ff61}", "a.b.c."),
        ("\u{00ad}", ""),
        ("XN--BCHER-KVA.Example", "xn--bcher-kva.example"),
    ] {
        let mut account = Account::default();
        let value = interpret(input.as_bytes(), &mut account).unwrap().unwrap();
        assert_eq!(value.as_bytes(), expected.as_bytes(), "{input}");
        assert!(!value.is_borrowed(), "{input}");
        assert!(value.as_bytes().is_ascii());
    }
}

#[test]
fn fixed_entry_keeps_empty_labels_hyphens_and_oversized_ascii_domains() {
    for input in [
        "",
        ".",
        "..",
        "-a",
        "a-",
        "ab--cd",
        "a..b.",
        "!$&'()*+,;=_`{}~",
    ] {
        assert!(
            interpret(input.as_bytes(), &mut Account::default())
                .unwrap()
                .is_ok(),
            "{input}"
        );
    }
    let long = "a".repeat(5000);
    let value = interpret(long.as_bytes(), &mut Account::default())
        .unwrap()
        .unwrap();
    assert!(value.is_borrowed());
    assert_eq!(value.as_bytes().len(), 5000);
    for input in [
        "xn--", "xn--a-", "a/b", "a b", "a%20b", "a:b", "a\0b", "a\u{7f}b",
    ] {
        assert!(
            interpret(input.as_bytes(), &mut Account::default())
                .unwrap()
                .is_err(),
            "{input}"
        );
    }
}

#[test]
fn prefix_and_later_labels_remain_subject_to_domain_bidi_rules() {
    for input in ["abc.א", "a1.א", "א.abc", "ا١", "ا1"] {
        assert!(
            interpret(input.as_bytes(), &mut Account::default())
                .unwrap()
                .is_ok(),
            "{input}"
        );
    }
    for input in [
        "1.א",
        "a-.א",
        "_.א",
        "א.1",
        "ا١1",
        "ا1١",
        "a\u{200c}b",
        "a\u{200d}b",
    ] {
        assert!(
            interpret(input.as_bytes(), &mut Account::default())
                .unwrap()
                .is_err(),
            "{input}"
        );
    }
}
