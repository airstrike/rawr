//! The value rustc writes for a type where it suggests giving one: `42`
//! for an integer, `None` for an `Option`, `Default::default()` for a
//! struct that implements `Default` (rustc's `ty_kind_suggestion`).

use super::*;

impl Parser<'_> {
    /// The value rustc suggests for `ty`, as `return 42;` and `let x: i32
    /// = 42;` write it: `None` for a type it suggests no value for, `!`
    /// and a type inference hasn't settled.
    pub(super) fn stand_in(&self, ty: &Ty) -> Option<String> {
        let ty = self.inference.resolve(ty);
        Some(match &ty {
            Ty::Never | Ty::Infer(_) | Ty::Unknown => return None,
            Ty::Bool => "false".into(),
            Ty::Char => "'x'".into(),
            Ty::Int(_) => "42".into(),
            Ty::Float(_) => "3.14159".into(),
            Ty::Unit => "()".into(),
            Ty::Array {
                kind: sequence::Kind::Slice,
                ..
            } => "[]".into(),
            Ty::Array {
                kind: sequence::Kind::Vec,
                ..
            } => "vec![]".into(),
            Ty::Array {
                element,
                kind: sequence::Kind::Array(length),
            } => match length.fixed() {
                Some(0) => "[]".into(),
                Some(length)
                    if length == 1 || self.type_provides(element, capability::Kind::Copy) =>
                {
                    format!("[{}; {length}]", self.stand_in(element)?)
                }
                _ => "/* value */".into(),
            },
            Ty::Boxed(inner) => format!("Box::new({})", self.stand_in(inner)?),
            Ty::Option(_) => "None".into(),
            Ty::Result(ok, _) => format!("Ok({})", self.stand_in(ok)?),
            Ty::Str => "\"\"".into(),
            Ty::Reference { mutable, target } => {
                let mutable = if *mutable { "mut " } else { "" };
                format!("&{mutable}{}", self.stand_in(target)?)
            }
            Ty::Tuple(members) => {
                let members = members
                    .iter()
                    .map(|member| self.stand_in(member))
                    .collect::<Option<Vec<_>>>()?;
                let comma = if members.len() == 1 { "," } else { "" };
                format!("({}{comma})", members.join(", "))
            }
            // rustc's `Default::default()` is for a struct or an enum.
            ty if nominal(ty) && self.type_provides(ty, capability::Kind::Default) => {
                "Default::default()".into()
            }
            _ => "/* value */".into(),
        })
    }
}

/// Whether `ty` is a struct or an enum, std's or the program's, as rustc's
/// `ty::Adt` is: not a scalar, a reference, a tuple, an array or a function.
const fn nominal(ty: &Ty) -> bool {
    !matches!(
        ty,
        Ty::Bool
            | Ty::Char
            | Ty::Int(_)
            | Ty::Float(_)
            | Ty::Unit
            | Ty::Str
            | Ty::UnsizedStr
            | Ty::Tuple(_)
            | Ty::Reference { .. }
            | Ty::Array {
                kind: sequence::Kind::Array(_) | sequence::Kind::Slice,
                ..
            }
            | Ty::Fn { .. }
            | Ty::FnItem { .. }
            | Ty::FnChoice { .. }
            | Ty::Callable { .. }
            | Ty::Dyn(..)
            | Ty::Param(_)
            | Ty::Projection(..)
            | Ty::Const(_)
            | Ty::Iter(..)
    )
}
