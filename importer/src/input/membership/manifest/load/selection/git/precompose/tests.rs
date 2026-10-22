use super::*;
use crate::input::membership::manifest::Error;
use rawr_toml::document::{Admission, Request};
use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::PathBuf};

#[derive(Default)]
struct Account {
    layouts: Vec<(usize, usize)>,
    calls: usize,
    refuse_layout: Option<usize>,
}
impl Admission for Account {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        self.calls += 1;
        if let Request::Allocation { bytes, alignment } = request {
            if self.refuse_layout == Some(bytes) {
                return Err(bytes);
            }
            self.layouts.push((bytes, alignment));
        }
        Ok(())
    }
}

fn transform<'a>(
    input: Cow<'a, Path>,
    account: &mut Account,
) -> Result<Cow<'a, Path>, Error<usize>> {
    let mut session = Session::new(account);
    let result = path(input, &mut session);
    session.finish(result)
}

#[test]
fn canonical_composition_covers_hangul_astral_order_and_blocked_marks() {
    for (input, expected) in [
        ("/Cafe\u{301}", "/Café"),
        ("/\u{212b}", "/Å"),
        ("/\u{1100}\u{1161}\u{11a8}", "/각"),
        ("/\u{c8e0}\u{11a7}", "/\u{c8e0}\u{11a7}"),
        ("/q\u{301}\u{323}", "/q\u{323}\u{301}"),
        ("/a\u{301}\u{300}", "/á\u{300}"),
        ("/\u{11099}\u{110ba}", "/\u{1109a}"),
    ] {
        let actual = transform(Cow::Borrowed(Path::new(input)), &mut Account::default()).unwrap();
        assert_eq!(actual, Path::new(expected), "{input:?}");
    }
}

#[test]
fn already_normalized_and_invalid_native_bytes_preserve_borrow_or_owned_storage() {
    for input in [
        b"/ASCII".as_slice(),
        "/café".as_bytes(),
        "/q\u{301}".as_bytes(),
        b"/\xff",
    ] {
        let source = Path::new(OsStr::from_bytes(input));
        let mut account = Account::default();
        assert!(matches!(
            transform(Cow::Borrowed(source), &mut account).unwrap(),
            Cow::Borrowed(_)
        ));
        let mut owned = PathBuf::with_capacity(input.len() + 100);
        owned.push(source);
        let pointer = owned.as_os_str().as_bytes().as_ptr();
        let capacity = owned.capacity();
        let result = transform(Cow::Owned(owned), &mut account).unwrap();
        assert!(matches!(result, Cow::Owned(_)));
        assert_eq!(result.as_os_str().as_bytes().as_ptr(), pointer);
        assert_eq!(result.into_owned().capacity(), capacity);
        assert!(account.layouts.is_empty());
    }
}

#[test]
fn long_marks_spill_both_buffers_preserve_equal_class_order_and_charge_sort_scratch() {
    // Descending classes make quick-check return No. q cannot compose these
    // marks, so both the decomposition and recomposition owners spill.
    let input = format!("/q{}{}", "\u{301}".repeat(300), "\u{323}".repeat(300));
    let expected = format!("/q{}{}", "\u{323}".repeat(300), "\u{301}".repeat(300));
    let mut account = Account::default();
    let result = transform(Cow::Borrowed(Path::new(&input)), &mut account).unwrap();
    assert_eq!(result, Path::new(&expected));
    assert!(account.layouts.contains(&(
        8 * std::mem::size_of::<(u8, char)>(),
        std::mem::align_of::<(u8, char)>()
    )));
    assert!(account.layouts.contains(&(
        8 * std::mem::size_of::<char>(),
        std::mem::align_of::<char>()
    )));
    assert!(account.layouts.contains(&(
        600 * std::mem::size_of::<(u8, char)>(),
        std::mem::align_of::<(u8, char)>()
    )));
}

#[test]
fn sort_scratch_refusal_stops_before_sort_and_latches_original_account() {
    let input = format!("/q{}{}", "\u{301}".repeat(300), "\u{323}".repeat(300));
    let scratch = 600 * std::mem::size_of::<(u8, char)>();
    let mut account = Account {
        refuse_layout: Some(scratch),
        ..Account::default()
    };
    let mut session = Session::new(&mut account);
    let result = path(Cow::Borrowed(Path::new(&input)), &mut session);
    assert!(result.is_err());
    assert!(session.items(1).is_err());
    assert!(matches!(session.finish(result), Err(Error::Admission(bytes)) if bytes == scratch));
    assert!(!account.layouts.iter().any(|(bytes, _)| *bytes == scratch));
}
