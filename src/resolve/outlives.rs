//! rustc's reports of a value a function returns whose lifetimes don't
//! relate to its own signature's as the signature's types require (T170):
//! "lifetime may not live long enough", and E0621.

use crate::{ImmutableString, Span};

/// rustc's report of a returned value that breaks what its function's
/// signature requires.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Report {
    /// "lifetime may not live long enough".
    NotLongEnough {
        /// The value returned.
        returned: Span,
        /// What the function is.
        item: Item,
        /// The lifetime that must outlive `shorter`.
        longer: Region,
        /// The lifetime `longer` must outlive.
        shorter: Region,
        /// Why the position the value is returned at is invariant, where
        /// that is why.
        variance: Option<Variance>,
    },
    /// E0621: the parameter's elided lifetime must be the output's.
    Explicit {
        /// The value returned.
        returned: Span,
        /// The parameter's name.
        parameter: ImmutableString,
        /// The output's lifetime, without its apostrophe.
        lifetime: ImmutableString,
        /// The parameter's reference whose lifetime is elided.
        reference: Reference,
    },
}

/// A lifetime a report names.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Region {
    /// A lifetime parameter.
    Named {
        /// Its name, without its apostrophe.
        name: ImmutableString,
        /// Where it is declared.
        declared: Span,
    },
    /// The elided lifetime of a reference, which rustc calls `'1`, `'2`
    /// and on in the order it names them.
    Anonymous(Reference),
    /// `'static`.
    Static,
}

/// A reference written in a signature.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Reference {
    /// Its `&`.
    pub ampersand: Span,
    /// It is a `&mut`.
    pub mutable: bool,
    /// It is the receiver's: `&self`.
    pub receiver: bool,
}

/// What rustc calls the function whose body breaks its signature.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Item {
    /// A free function.
    Function,
    /// An associated function with a receiver.
    Method,
    /// An associated function without one.
    AssociatedFunction,
}

impl Item {
    /// rustc's name for it.
    pub(crate) const fn noun(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::AssociatedFunction => "associated function",
        }
    }
}

/// Why a position of the output's type is invariant, as rustc's notes say
/// it, each type written with its lifetimes erased.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Variance {
    /// What a `&mut` refers to.
    Referent {
        /// Its type.
        referent: ImmutableString,
    },
    /// A generic argument of a type.
    Argument {
        /// The type: `Cell<&i32>`.
        ty: ImmutableString,
        /// Its argument: `&i32`.
        argument: ImmutableString,
        /// What declares the type: `struct` or `enum`.
        kind: &'static str,
        /// The type as declared: `Cell<T>`.
        declared: ImmutableString,
        /// The declaration's parameter the argument is: `T`.
        parameter: ImmutableString,
    },
}

impl Variance {
    /// rustc's help after its notes.
    pub(crate) const HELP: &'static str =
        "see <https://doc.rust-lang.org/nomicon/subtyping.html> for more information about \
         variance";

    /// rustc's notes on it, which [`Variance::HELP`] follows.
    pub(crate) fn notes(&self) -> [String; 2] {
        match self {
            Self::Referent { referent } => [
                format!("requirement occurs because of a mutable reference to `{referent}`"),
                "mutable references are invariant over their type parameter".to_string(),
            ],
            Self::Argument {
                ty,
                argument,
                kind,
                declared,
                parameter,
            } => [
                format!(
                    "requirement occurs because of the type `{ty}`, which makes the generic \
                     argument `{argument}` invariant"
                ),
                format!("the {kind} `{declared}` is invariant over the parameter `{parameter}`"),
            ],
        }
    }
}

/// rustc's help after E0621 for a `&mut` parameter.
const MUTABLE_HELP: &str = "see <https://doc.rust-lang.org/nomicon/borrow-splitting.html> for \
                            more information about lifetime errors related to mutable references";

/// What rustc suggests writing: `lifetime` after the reference's `&`,
/// under `message`.
pub(crate) struct Suggestion<'a> {
    pub(crate) message: String,
    pub(crate) reference: &'a Reference,
    pub(crate) lifetime: &'a str,
}

impl Report {
    /// rustc's message.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::NotLongEnough { .. } => "lifetime may not live long enough".to_string(),
            Self::Explicit { parameter, .. } => {
                format!("explicit lifetime required in the type of `{parameter}`")
            }
        }
    }

    /// What the report is about: the parameter whose type needs a lifetime,
    /// or what rustc says of the lifetime.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::NotLongEnough { .. } => "lifetime may not live long enough",
            Self::Explicit { parameter, .. } => parameter,
        }
    }

    /// rustc's code, or [`super::borrowck::LIFETIME`] for its "lifetime may
    /// not live long enough".
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::NotLongEnough { .. } => super::borrowck::LIFETIME,
            Self::Explicit { .. } => "E0621",
        }
    }

    /// The value returned.
    pub(crate) const fn returned(&self) -> Span {
        match self {
            Self::NotLongEnough { returned, .. } | Self::Explicit { returned, .. } => *returned,
        }
    }

    /// The report with the value returned at `returned`.
    pub(crate) fn returned_at(mut self, at: Span) -> Self {
        match &mut self {
            Self::NotLongEnough { returned, .. } | Self::Explicit { returned, .. } => {
                *returned = at;
            }
        }
        self
    }

    /// rustc's label on the returned value.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::NotLongEnough {
                item,
                longer,
                shorter,
                ..
            } => {
                let [(_, longer_name), (_, shorter_name)] = names(longer, shorter);
                match shorter {
                    Region::Static => format!(
                        "returning this value requires that `{longer_name}` must outlive \
                         `'static`"
                    ),
                    _ => format!(
                        "{} was supposed to return data with lifetime `{shorter_name}` but it \
                         is returning data with lifetime `{longer_name}`",
                        item.noun()
                    ),
                }
            }
            Self::Explicit { lifetime, .. } => format!("lifetime `'{lifetime}` required"),
        }
    }

    /// rustc's labels on the lifetimes it names: where each named one is
    /// declared, and each elided one's reference.
    pub(crate) fn lifetimes(&self) -> Vec<(Span, String)> {
        let Self::NotLongEnough {
            longer, shorter, ..
        } = self
        else {
            return Vec::new();
        };
        names(longer, shorter)
            .into_iter()
            .filter_map(|(region, name)| match region {
                Region::Named { declared, .. } => {
                    Some((*declared, format!("lifetime `{name}` defined here")))
                }
                Region::Anonymous(reference) => Some((
                    reference.ampersand,
                    format!("let's call the lifetime of this reference `{name}`"),
                )),
                Region::Static => None,
            })
            .collect()
    }

    /// rustc's `= help:` before its notes: the bound that would relate two
    /// named lifetimes, or where to read about a `&mut` parameter's.
    pub(crate) fn help(&self) -> Option<String> {
        match self {
            Self::NotLongEnough {
                longer: longer @ Region::Named { .. },
                shorter: shorter @ Region::Named { .. },
                ..
            } => {
                let [(_, longer), (_, shorter)] = names(longer, shorter);
                Some(format!(
                    "consider adding the following bound: `{longer}: {shorter}`"
                ))
            }
            Self::NotLongEnough { .. } => None,
            Self::Explicit { reference, .. } => reference.mutable.then(|| MUTABLE_HELP.to_string()),
        }
    }

    /// The lifetime rustc suggests writing on a reference: the output's on
    /// the parameter's, or a named one on `&self`.
    pub(crate) fn suggestion(&self) -> Option<Suggestion<'_>> {
        match self {
            Self::Explicit {
                parameter,
                lifetime,
                reference,
                ..
            } if !reference.mutable => Some(Suggestion {
                message: format!(
                    "add explicit lifetime `'{lifetime}` to the type of `{parameter}`"
                ),
                reference,
                lifetime,
            }),
            Self::NotLongEnough {
                longer: Region::Anonymous(reference),
                shorter: Region::Named { name, .. },
                ..
            } if reference.receiver => Some(Suggestion {
                message: "consider reusing a named lifetime parameter and update trait if needed"
                    .to_string(),
                reference,
                lifetime: name,
            }),
            Self::NotLongEnough { .. } | Self::Explicit { .. } => None,
        }
    }

    /// Why the position the value is returned at is invariant, which
    /// rustc's notes say.
    pub(crate) const fn variance(&self) -> Option<&Variance> {
        match self {
            Self::NotLongEnough { variance, .. } => variance.as_ref(),
            Self::Explicit { .. } => None,
        }
    }
}

/// `longer` and `shorter`, each with the name rustc gives it: an elided
/// one is `'1`, then `'2`.
fn names<'a>(longer: &'a Region, shorter: &'a Region) -> [(&'a Region, String); 2] {
    let mut anonymous = 0;
    [longer, shorter].map(|region| {
        let name = match region {
            Region::Named { name, .. } => format!("'{name}"),
            Region::Anonymous(_) => {
                anonymous += 1;
                format!("'{anonymous}")
            }
            Region::Static => "'static".to_string(),
        };
        (region, name)
    })
}
