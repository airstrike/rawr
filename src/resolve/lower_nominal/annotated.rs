//! The type rustc suggests annotating a local with when nothing settles
//! its type: the type as far as it is known, each unsettled part named
//! by the type parameter it stands for, as `Vec<T>` of `Vec<_>`.

use super::*;

impl Parser<'_> {
    /// The annotation rustc suggests for a local of type `ty`, part of
    /// which nothing settled: the type with each unsettled part the name
    /// of the parameter it fills, where it fills one, and the first such
    /// name. `None` when nothing of the type is known.
    pub(super) fn annotated(&self, ty: &Ty) -> Option<(String, Option<ImmutableString>)> {
        let ty = self.inference.resolve(ty);
        if matches!(ty, Ty::Infer(_) | Ty::Unknown) {
            return None;
        }
        let mut parameters = Vec::new();
        self.unsettled_parameters(&ty, None, &mut parameters);
        let first = parameters.iter().flatten().next().cloned();
        let mut parameters = parameters.into_iter();
        Some((
            placeholders(&ty.name(&self.models), || parameters.next().flatten()),
            first,
        ))
    }

    /// The parameter each unsettled part of `ty` fills, in the order its
    /// name writes them, `parameter` being the one `ty` itself fills.
    fn unsettled_parameters(
        &self,
        ty: &Ty,
        parameter: Option<ImmutableString>,
        found: &mut Vec<Option<ImmutableString>>,
    ) {
        let ty = self.inference.resolve(ty);
        let named = |names: &[&str], arguments: &[&Ty], found: &mut Vec<_>| {
            for (index, argument) in arguments.iter().enumerate() {
                let name = names.get(index).map(|name| ImmutableString::from(*name));
                self.unsettled_parameters(argument, name, found);
            }
        };
        match &ty {
            Ty::Infer(_) => found.push(parameter),
            Ty::Array {
                element,
                kind: sequence::Kind::Vec | sequence::Kind::Deque | sequence::Kind::Slice,
            } => named(&["T"], &[element], found),
            Ty::Option(inner)
            | Ty::Boxed(inner)
            | Ty::Counted(_, inner)
            | Ty::Weak(_, inner)
            | Ty::Cell(_, inner)
            | Ty::Reverse(inner)
            | Ty::Thread(_, inner) => named(&["T"], &[inner], found),
            Ty::Cow(inner) => named(&["B"], &[inner], found),
            Ty::Result(ok, err) => named(&["T", "E"], &[ok, err], found),
            Ty::Keyed(keyed, arguments) => {
                let names: &[&str] = match keyed {
                    Keyed::HashMap | Keyed::BTreeMap => &["K", "V"],
                    Keyed::HashSet | Keyed::BTreeSet => &["T"],
                };
                named(names, &arguments.iter().collect::<Vec<_>>(), found);
            }
            Ty::Script(id, arguments) => {
                let names = self
                    .models
                    .definition(*id)
                    .generics()
                    .iter()
                    .map(|generic| generic.name.text.as_str())
                    .collect::<Vec<_>>();
                named(&names, &arguments.iter().collect::<Vec<_>>(), found);
            }
            // A part no parameter names, as a tuple's member, stays `_`.
            Ty::Array { element, .. } => named(&[], &[element], found),
            Ty::Reference { target, .. } => named(&[], &[target], found),
            Ty::Tuple(members) => named(&[], &members.iter().collect::<Vec<_>>(), found),
            Ty::Host { arguments, .. } => {
                named(&[], &arguments.iter().collect::<Vec<_>>(), found);
            }
            _ => {}
        }
    }
}

/// `written` with each `_` that stands alone, a placeholder, replaced by
/// the name `next` gives it, or left `_` where it gives none.
fn placeholders(written: &str, mut next: impl FnMut() -> Option<ImmutableString>) -> String {
    let identifier = |c: char| c.is_alphanumeric() || c == '_';
    let characters = written.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(written.len());
    for (index, &character) in characters.iter().enumerate() {
        let alone = character == '_'
            && !index
                .checked_sub(1)
                .is_some_and(|before| identifier(characters[before]))
            && !characters.get(index + 1).copied().is_some_and(identifier);
        match alone.then(&mut next).flatten() {
            Some(name) => output.push_str(&name),
            None => output.push(character),
        }
    }
    output
}
