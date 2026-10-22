use rawr_toml::document::{Admission, Request};

use super::prepare;
use crate::input::membership::manifest::{session::Session, Error, Stop};

#[derive(Default)]
pub(super) struct Count {
    pub(super) requests: Vec<Request>,
    pub(super) refuse: Option<usize>,
    pub(super) calls: usize,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let index = self.calls;
        self.calls += 1;
        if self.refuse == Some(index) {
            return Err(index);
        }
        self.requests.push(request);
        Ok(())
    }
}

#[test]
fn constructor_allocation_refusals_preserve_the_original_prefix_and_terminal_cause() {
    let patterns = vec!["abcdef".to_owned()];
    let mut baseline = Count::default();
    assert!(prepare(&patterns, &mut Session::new(&mut baseline))
        .unwrap()
        .is_ok());
    let allocations: Vec<_> = baseline
        .requests
        .iter()
        .enumerate()
        .filter_map(|(index, request)| {
            matches!(request, Request::Allocation { .. }).then_some(index)
        })
        .collect();
    // Constructor entry, actual internal growth and the final compacting
    // allocation. Each is reached with its original nonzero prior work.
    for &index in &[
        allocations[0],
        allocations[allocations.len() / 2],
        *allocations.last().unwrap(),
    ] {
        let mut account = Count {
            refuse: Some(index),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            prepare(&patterns, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(prepare(&[], &mut session), Err(Stop::Refused)));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(at)) if at == index));
        assert_eq!(account.requests, baseline.requests[..index]);
        assert_eq!(account.calls, index + 1);
    }
}
