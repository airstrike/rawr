//! Reads std's written types and declarations as the facade's host types
//! and signatures.
//!
//! A type rawr represents is its host type: a primitive, `String`,
//! `Option`, `Result`, `Vec`, a slice, a reference, a tuple, a `Box` of
//! what it holds, a [`Held`] type, and a `Cow` as the type its `ToOwned`
//! makes. A value an item gives of any other type std implements
//! `Iterator` for is rawr's iterator of its items. Any other type is one
//! the facade can't declare an item with. rawr has no borrows of its own,
//! so a borrow of a borrow is one borrow.
//!
//! A generic parameter is each type std's impls give for its bounds, which
//! the facade declares as an overload of its own: `fs::read<P:
//! AsRef<Path>>` takes a `String`, an `OsString`, a `PathBuf` or a borrow
//! of any of them.

use super::solve::{self, Binding, Slot};
use crate::host::signature;
use crate::host::type_ref::{Builtin, Shape, Type};
use crate::receiver;
use crate::std_facade::held::Held;
use crate::std_table::{self, Declared, Written};

/// A declaration's form the facade declares: how it takes `self`, if it
/// does, and its signature without it.
pub(super) struct Form {
    pub receiver: Option<receiver::Mode>,
    pub signature: signature::Definition,
}

/// The facade's reading of std's types in one declaration's scope.
#[derive(Clone)]
pub(super) struct Reader<'a> {
    held: &'a [Held],
    /// What each name the declaration writes stands for: `Self`, its
    /// trait's parameters, its own.
    names: Binding,
    /// The associated types `Self::Name` stands for: `Owned = PathBuf`.
    associated: Binding,
    /// A type alias's parameters, which stay parameters: `T` of
    /// `io::Result<T>`.
    parameters: &'a [&'static str],
    /// Whether the types read are what an item gives.
    output: bool,
}

impl<'a> Reader<'a> {
    pub(super) fn new(held: &'a [Held]) -> Self {
        Self {
            held,
            names: Binding::new(),
            associated: Binding::new(),
            parameters: &[],
            output: false,
        }
    }

    /// The reader of a declaration whose names stand for `names` and whose
    /// `Self::Name` types are `associated`.
    pub(super) fn scoped(&self, names: Binding, associated: Binding) -> Self {
        Self {
            names,
            associated,
            ..self.clone()
        }
    }

    /// The reader of a type alias with the type parameters `parameters`.
    pub(super) fn with_parameters(&self, parameters: &'a [&'static str]) -> Self {
        Self {
            parameters,
            ..self.clone()
        }
    }

    /// The reader of what an item gives.
    fn given(&self) -> Self {
        Self {
            output: true,
            ..self.clone()
        }
    }

    /// The host type `written` is, when rawr represents it.
    pub(super) fn host(&self, written: &Written) -> Option<Type> {
        match written {
            Written::Path {
                path, arguments, ..
            } => self.path(path, arguments),
            Written::Reference { mutable, target } => {
                let target = self.host(target)?;
                match target.shape() {
                    Shape::Reference { mutable: inner, .. } if *mutable || !*inner => Some(target),
                    Shape::Reference { target, .. } => {
                        Some(Type::reference(false, (**target).clone()))
                    }
                    _ => Some(Type::reference(*mutable, target)),
                }
            }
            Written::Slice(element) => Some(Type::slice(self.host(element)?)),
            Written::Tuple(members) if members.is_empty() => Some(Type::unit()),
            Written::Tuple(members) => Type::tuple(
                members
                    .iter()
                    .map(|member| self.host(member))
                    .collect::<Option<Vec<_>>>()?,
            )
            .ok(),
            // An `impl Trait` input stands for what its parameter is.
            Written::Other(text) => {
                let (_, bound) = self.names.iter().find(|(name, _)| name == text)?;
                self.host(bound)
            }
            Written::Never => Some(Type::builtin(Builtin::Never)),
            Written::Array(..) => None,
        }
    }

    fn path(&self, path: &'static str, arguments: &[Written]) -> Option<Type> {
        if arguments.is_empty() {
            if let Some((_, bound)) = self.names.iter().find(|(name, _)| *name == path) {
                return self.host(bound);
            }
            if self.parameters.contains(&path) {
                return Type::parameter(path).ok();
            }
        }
        if let Some(name) = path.strip_prefix("Self::") {
            let (_, bound) = self.associated.iter().find(|(bound, _)| *bound == name)?;
            return self.host(bound);
        }
        if path == "str" {
            return Some(Type::builtin(Builtin::String));
        }
        if let Some(builtin) = Builtin::primitive(path) {
            return Some(Type::builtin(builtin));
        }
        let argument = |index: usize| self.host(arguments.get(index)?);
        let definition = solve::definition(path);
        match definition {
            "core::option::Option" => return Some(Type::option(argument(0)?)),
            "core::result::Result" => return Some(Type::result(argument(0)?, argument(1)?)),
            "alloc::vec::Vec" => return Some(Type::vec(argument(0)?)),
            "alloc::string::String" => return Some(Type::builtin(Builtin::String)),
            "core::time::Duration" => return Some(Type::builtin(Builtin::Duration)),
            "std::time::Instant" => return Some(Type::builtin(Builtin::Instant)),
            "alloc::boxed::Box" => return Some(Type::boxed(argument(0)?)),
            // What a `Cow` holds is copied out: the type its `ToOwned`
            // makes.
            "alloc::borrow::Cow" => {
                let owned =
                    solve::associated("alloc::borrow::ToOwned", arguments.first()?, "Owned")?;
                return self.host(&owned);
            }
            _ => {}
        }
        for held in self.held {
            match held {
                Held::Value(value)
                    if value
                        .forms()
                        .any(|form| solve::definition(form) == definition) =>
                {
                    return Type::registered(super::held::Value::facade(value.std)).ok();
                }
                Held::Text(text) if solve::definition(text) == definition => {
                    return Some(Type::builtin(Builtin::String));
                }
                _ => {}
            }
        }
        if !self.output {
            return None;
        }
        let written = Written::Path {
            path,
            arguments: arguments.to_vec(),
            bindings: Vec::new(),
        };
        let item = solve::associated("core::iter::traits::iterator::Iterator", &written, "Item")?;
        Some(Type::iterator(self.host(&item)?))
    }

    /// Each form of the function `declaration` the facade declares: one
    /// for each type its generic parameters' bounds give them.
    pub(super) fn forms(&self, declaration: &'static str) -> Vec<Form> {
        let Some(declared) = Declared::parse(declaration) else {
            return Vec::new();
        };
        let mut inputs = std_table::split_list(declared.inputs);
        let receiver = inputs.first().and_then(|first| receiver(first));
        if receiver.is_some() {
            inputs.remove(0);
        }
        let mut parameters = Vec::<(&'static str, Vec<&'static str>)>::new();
        for generic in std_table::split_list(declared.generics) {
            if generic.starts_with('\'') {
                continue;
            }
            if generic.starts_with("const ") {
                return Vec::new();
            }
            let (name, bounds) = match std_table::top_level(generic, ": ") {
                Some(at) => (&generic[..at], std_table::split_bounds(&generic[at + 2..])),
                None => (generic, Vec::new()),
            };
            parameters.push((name, bounds));
        }
        let mut types = Vec::new();
        for input in inputs {
            let Some(at) = std_table::top_level(input, ": ") else {
                return Vec::new();
            };
            let ty = &input[at + 2..];
            // `impl AsRef<Path>` is a parameter without a name; its text
            // names it.
            if let Some(bounds) = ty.strip_prefix("impl ") {
                parameters.push((ty, std_table::split_bounds(bounds)));
            }
            types.push(solve::read(ty));
        }
        for clause in std_table::split_list(declared.wheres) {
            let Some(at) = std_table::top_level(clause, ": ") else {
                return Vec::new();
            };
            let (subject, bounds) = (&clause[..at], std_table::split_bounds(&clause[at + 2..]));
            match parameters.iter_mut().find(|(name, _)| *name == subject) {
                Some((_, own)) => own.extend(bounds),
                None if !self.proves(subject, &bounds) => return Vec::new(),
                None => {}
            }
        }
        let output = declared.output.map(solve::read);
        let mut forms = Vec::<Form>::new();
        for names in self.expansions(&parameters) {
            let reader = self.scoped(names, self.associated.clone());
            let Some(inputs) = types
                .iter()
                .map(|ty| reader.host(ty))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let output = match &output {
                Some(output) => match reader.given().host(output) {
                    Some(output) => output,
                    None => continue,
                },
                None => Type::unit(),
            };
            let mut builder = signature::Builder::new();
            for input in inputs {
                builder.parameter(input);
            }
            builder.output(output);
            let signature = builder.finish();
            let repeated = forms.iter().any(|form| {
                form.receiver == receiver && form.signature.same_inputs(&signature).unwrap_or(true)
            });
            if !repeated {
                forms.push(Form {
                    receiver,
                    signature,
                });
            }
        }
        forms
    }

    /// The bindings of the declaration's names with each of its parameters
    /// bound to each type its bounds give it.
    fn expansions(&self, parameters: &[(&'static str, Vec<&'static str>)]) -> Vec<Binding> {
        let mut expansions = vec![self.names.clone()];
        for (name, bounds) in parameters {
            let types = self.types(bounds);
            expansions = expansions
                .iter()
                .flat_map(|names| {
                    types.iter().map(|ty| {
                        let mut names = names.clone();
                        names.push((*name, ty.clone()));
                        names
                    })
                })
                .collect();
        }
        expansions
    }

    /// The types a parameter with `bounds` takes: those std's impls give
    /// for its first bound that meet the others, and that are sized unless
    /// it is `?Sized`. The argument is only read, so its `&mut` form is its
    /// `&` one, and a `Box` is the value it holds, as rawr holds it.
    fn types(&self, bounds: &[&'static str]) -> Vec<Written> {
        let sized = !bounds.contains(&"?Sized");
        let mut traits = bounds
            .iter()
            .filter(|bound| **bound != "?Sized" && !bound.starts_with('\''))
            .map(|bound| self.bound(bound));
        let Some(Some((trait_, arguments))) = traits.next() else {
            return Vec::new();
        };
        let Some(rest) = traits.collect::<Option<Vec<_>>>() else {
            return Vec::new();
        };
        let slots = arguments.into_iter().map(Slot::Known).collect::<Vec<_>>();
        let mut types = Vec::<Written>::new();
        for ty in solve::solve(trait_, &Slot::Unknown, &slots, solve::DEPTH) {
            let ty = match ty {
                Written::Reference { target, .. } => Written::Reference {
                    mutable: false,
                    target: Box::new(unboxed(*target)),
                },
                ty => unboxed(ty),
            };
            let fits = (!sized || self.sized(&ty))
                && rest
                    .iter()
                    .all(|(trait_, arguments)| solve::holds(trait_, &ty, arguments));
            if fits && !types.iter().any(|known| solve::same(known, &ty)) {
                types.push(ty);
            }
        }
        types
    }

    /// The trait a bound names, with its arguments in this scope.
    fn bound(&self, bound: &'static str) -> Option<(&'static str, Vec<Written>)> {
        let Written::Path {
            path, arguments, ..
        } = solve::read(bound)
        else {
            return None;
        };
        let trait_ = std_table::definition_of(path)?;
        let arguments = arguments
            .iter()
            .map(|argument| solve::substitute(argument, &self.names))
            .collect();
        Some((trait_, arguments))
    }

    /// Whether a where-clause on a type the scope fixes holds.
    fn proves(&self, subject: &'static str, bounds: &[&'static str]) -> bool {
        let subject = solve::substitute(&solve::read(subject), &self.names);
        bounds
            .iter()
            .filter(|bound| **bound != "?Sized" && !bound.starts_with('\''))
            .all(|bound| {
                self.bound(bound)
                    .is_some_and(|(trait_, arguments)| solve::holds(trait_, &subject, &arguments))
            })
    }

    /// Whether a value of `ty` has a size: not a `str`, a slice, a `dyn`
    /// type or a type a held type's borrow stands for.
    fn sized(&self, ty: &Written) -> bool {
        match ty {
            Written::Slice(_) | Written::Other(_) => false,
            Written::Path { path, .. } => {
                let definition = solve::definition(path);
                definition != "str"
                    && !self.held.iter().any(|held| match held {
                        Held::Value(value) => value
                            .lent
                            .iter()
                            .any(|lent| solve::definition(lent) == definition),
                        Held::Text(_) => false,
                    })
            }
            _ => true,
        }
    }
}

/// The value a `Box` holds, or `ty` when it is no `Box`.
fn unboxed(ty: Written) -> Written {
    match ty {
        Written::Path {
            path,
            mut arguments,
            ..
        } if solve::definition(path) == "alloc::boxed::Box" && !arguments.is_empty() => {
            arguments.swap_remove(0)
        }
        ty => ty,
    }
}

/// How a function's first input takes `self`, when it does.
fn receiver(input: &str) -> Option<receiver::Mode> {
    let reference = input.strip_prefix('&');
    // `&'a self` takes `self` as `&self` does.
    let unnamed = match reference {
        Some(rest) if rest.starts_with('\'') => rest.split_once(' ').map_or(rest, |(_, rest)| rest),
        Some(rest) => rest,
        None => input,
    };
    match (reference.is_some(), unnamed) {
        (true, "self") => Some(receiver::Mode::Shared),
        (true, "mut self") => Some(receiver::Mode::Mutable),
        (false, "self" | "mut self") => Some(receiver::Mode::Owned),
        _ if input.starts_with("self:") || input.starts_with("mut self:") => {
            Some(receiver::Mode::Owned)
        }
        _ => None,
    }
}
