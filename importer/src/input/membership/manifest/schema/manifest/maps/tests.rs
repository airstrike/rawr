use rawr_toml::document;

use crate::input::membership::manifest::{reference, session, value, Error, Step};

use super::{Features, Patch, Platforms, Replace, Targets};

mod pinned;
mod representations;
mod resources;

#[derive(Clone, Copy, Debug)]
enum Family {
    Features,
    Platforms,
    Patch,
    Replace,
    Targets,
}

impl Family {
    fn run<A: document::Admission>(
        self,
        input: value::Input<'_, '_>,
        session: &mut session::Session<'_, A>,
    ) -> Step<usize> {
        match self {
            Self::Features => {
                Features::interpret(input, session).map(|value| value.entries().len())
            }
            Self::Platforms => {
                Platforms::interpret(input, session).map(|value| value.entries().len())
            }
            Self::Patch => Patch::interpret(input, session).map(|value| value.entries().len()),
            Self::Replace => Replace::interpret(input, session).map(|value| value.entries().len()),
            Self::Targets => Targets::interpret(input, session).map(|value| value.entries().len()),
        }
    }

    fn interpret(
        self,
        input: value::Input<'_, '_>,
    ) -> Result<usize, Error<std::convert::Infallible>> {
        let mut account = reference::Account;
        let mut session = session::Session::new(&mut account);
        let result = self.run(input, &mut session);
        session.finish(result)
    }
}

fn parse(source: &str) -> document::Document<'_> {
    document::parse(source, 80, &mut reference::Account).unwrap()
}

fn input<'d>(document: &'d document::Document<'d>) -> value::Input<'d, 'd> {
    value::Input::raw(document.table().get("value").unwrap())
}
