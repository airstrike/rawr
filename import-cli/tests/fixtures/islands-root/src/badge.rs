// rawr
//! A leaf module of an existing app, marked to hot-reload: it builds the
//! root's `Message` and calls the root's helper.

use crate::page;
use crate::Message;

pub fn message(pi: usize) -> String {
    format!("{:?}", build(pi))
}

fn build(pi: usize) -> Message {
    if pi == 1 {
        return crate::Message::Noop;
    }
    Some(crate::helper(pi))
        .map(move |opened| crate::Message::Page(pi, page::Message::Open(opened)))
        .unwrap_or(crate::Message::Page(pi, page::Message::Noop))
}

pub fn describe(wide: bool) -> String {
    let kind = if wide {
        crate::Kind::Wide
    } else {
        crate::Kind::Narrow
    };
    let event: crate::Event = page::Message::Open(super::SCALE);
    let name = match kind {
        crate::Kind::Narrow => "narrow",
        crate::Kind::Wide => "wide",
    };
    match event {
        page::Message::Open(scale) => format!("{name} x{scale}"),
        page::Message::Noop => name.to_string(),
    }
}
