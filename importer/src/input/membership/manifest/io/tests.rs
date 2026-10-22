use std::{cell::RefCell, io::ErrorKind, path::Path};

use rawr_toml::document::{Admission, Request};

use super::{Custom, Error, Failure, Message, Owned, Session};

#[derive(Default)]
struct Count {
    layouts: Vec<(usize, usize)>,
    refuse: Option<usize>,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), Self::Error> {
        if let Request::Allocation { bytes, alignment } = request {
            let index = self.layouts.len();
            self.layouts.push((bytes, alignment));
            if self.refuse == Some(index) {
                return Err(index);
            }
        }
        Ok(())
    }
}

#[test]
fn formatted_io_error_retains_three_actual_allocations_and_first_refusal() {
    let mut count = Count::default();
    let mut session = Session::new(&mut count);
    let result = Failure::oversized(Path::new("/fixture"), 65537, &mut session);
    let error = session.finish(result).unwrap();
    assert_eq!(error.kind(), ErrorKind::Other);
    assert_eq!(
        error.message(),
        Some("Refusing to open files larger than 65536 bytes, '/fixture' was 65537 bytes large")
    );
    assert_eq!(
        count.layouts,
        [
            (error.message().unwrap().len(), 1),
            (
                std::mem::size_of::<Message>(),
                std::mem::align_of::<Message>()
            ),
            (
                std::mem::size_of::<Custom>(),
                std::mem::align_of::<Custom>()
            ),
        ]
    );
    for refused in 0..3 {
        let mut count = Count {
            refuse: Some(refused),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        let result = Failure::oversized(Path::new("/fixture"), 65537, &mut session);
        assert!(matches!(session.finish(result), Err(Error::Admission(index)) if index == refused));
        assert_eq!(count.layouts.len(), refused + 1);
    }
}

thread_local! { static DROPS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) }; }
fn inner(values: &mut Vec<Message>) {
    DROPS.with(|events| events.borrow_mut().push("inner"));
    super::retire_message(values);
    DROPS.with(|events| events.borrow_mut().push("inner retired"));
}
fn outer(values: &mut Vec<Custom>) {
    DROPS.with(|events| events.borrow_mut().push("outer"));
    super::retire_custom(values);
    DROPS.with(|events| events.borrow_mut().push("outer retired"));
}

#[test]
fn both_real_cleanup_callbacks_run_once_and_partial_outer_refusal_retires_inner() {
    let mut count = Count::default();
    let mut session = Session::new(&mut count);
    let Failure::Custom(mut owner) = Failure::empty(Path::new("/fixture"), &mut session).unwrap()
    else {
        panic!("custom error")
    };
    owner.0[0].error_drop = inner;
    owner.0[0].outer_drop = outer;
    DROPS.with(|events| events.borrow_mut().clear());
    drop(owner);
    DROPS.with(|events| {
        assert_eq!(
            *events.borrow(),
            ["outer", "inner", "inner retired", "outer retired"]
        )
    });

    let mut count = Count {
        refuse: Some(1),
        ..Count::default()
    };
    let mut session = Session::new(&mut count);
    let payload = super::retain(Message("already owned".into()), &mut session).unwrap();
    let custom = Custom {
        kind: ErrorKind::Other,
        error: payload,
        error_drop: inner,
        outer_drop: outer,
    };
    DROPS.with(|events| events.borrow_mut().clear());
    let result = super::retain(custom, &mut session).map(Owned);
    assert!(matches!(session.finish(result), Err(Error::Admission(1))));
    DROPS.with(|events| assert_eq!(*events.borrow(), ["inner", "inner retired"]));
}
