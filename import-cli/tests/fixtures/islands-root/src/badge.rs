// rawr
//! A leaf module of an existing app, marked to hot-reload: it builds the
//! root's `Message` and calls the root's helper.

use crate::page;
use crate::Message;

pub fn message(pi: usize) -> Message {
    if pi == 1 {
        return crate::Message::Noop;
    }
    Some(crate::helper(pi))
        .map(move |opened| crate::Message::Page(pi, page::Message::Open(opened)))
        .unwrap_or(crate::Message::Page(pi, page::Message::Noop))
}

pub fn describe(kind: crate::Kind) -> String {
    let name: crate::Label<'_> = match kind {
        crate::Kind::Narrow => "narrow",
        crate::Kind::Wide => "wide",
    };
    format!("{name} x{}", super::SCALE)
}
