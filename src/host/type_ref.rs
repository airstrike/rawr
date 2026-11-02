//! Structural types used by static host signatures.

use crate::host;

/// A Rawr builtin available in host signature metadata.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Builtin {
    /// `bool`.
    Bool,
    /// `char`.
    Char,
    /// `f64`.
    F64,
    /// `i64`.
    I64,
    /// `u64`, an integer at runtime.
    U64,
    /// `u32`, an integer at runtime.
    U32,
    /// `RangeInclusive<i64>`.
    InclusiveRange,
    /// `RangeInclusive<f64>`.
    FloatInclusiveRange,
    /// `String`.
    String,
    /// `i8`.
    I8,
    /// `i16`.
    I16,
    /// `i32`.
    I32,
    /// `isize`.
    Isize,
    /// `u8`.
    U8,
    /// `u16`.
    U16,
    /// `usize`.
    Usize,
    /// `f32`.
    F32,
    /// `std::time::Duration`, Rawr's own `Duration` at runtime.
    Duration,
    /// `std::time::Instant`, Rawr's own `Instant` at runtime.
    Instant,
}

impl Builtin {
    /// The Rust name of a builtin `primitive` reads, or `None` for a range.
    pub(crate) const fn name(self) -> Option<&'static str> {
        Some(match self {
            Self::Bool => "bool",
            Self::Char => "char",
            Self::F64 => "f64",
            Self::I64 => "i64",
            Self::U64 => "u64",
            Self::U32 => "u32",
            Self::String => "String",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::Isize => "isize",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::Usize => "usize",
            Self::F32 => "f32",
            Self::InclusiveRange | Self::FloatInclusiveRange | Self::Duration | Self::Instant => {
                return None
            }
        })
    }

    /// The builtin a Rust primitive name is.
    #[must_use]
    pub fn primitive(name: &str) -> Option<Self> {
        Some(match name {
            "bool" => Self::Bool,
            "char" => Self::Char,
            "f64" => Self::F64,
            "i64" => Self::I64,
            "String" => Self::String,
            "i8" => Self::I8,
            "i16" => Self::I16,
            "i32" => Self::I32,
            "isize" => Self::Isize,
            "u8" => Self::U8,
            "u16" => Self::U16,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "usize" => Self::Usize,
            "f32" => Self::F32,
            _ => return None,
        })
    }
}

/// Which of Rust's sequence types a host's array is.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum Sequence {
    /// `[T; N]`, of its length.
    Array(usize),
    /// `Vec<T>`.
    Vec,
    /// `[T]`, whose elements a call copies.
    Slice,
}

/// A validated structural type in a static host signature.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Type(Shape);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum Shape {
    Unit,
    Tuple(Box<[Type]>),
    /// `[T; N]`, `Vec<T>` or `[T]`, as `sequence` says: an array at
    /// runtime.
    Array {
        element: Box<Type>,
        sequence: Sequence,
    },
    /// Rust's `Option<T>`, Rawr's builtin generic enum.
    Option(Box<Type>),
    /// An imported input `&T` or `&mut T`, as ratatui's
    /// `render_stateful_widget` takes `&mut ListState`: a script reference to
    /// the value, whose slot the call leases (D558.5).
    Reference {
        mutable: bool,
        target: Box<Type>,
    },
    /// An iterator of `T` (`impl Iterator<Item = T>`): the array of its
    /// items at runtime, made when the call returns.
    Iter(Box<Type>),
    /// Rust's `std::ops::Range<T>` of a float: a `Range<f64>` at runtime.
    Range(Box<Type>),
    /// Rust's `Box<T>`: rawr's `Box`, its value at runtime (D711 step 5).
    Boxed(Box<Type>),
    /// Rust's `std::ops::RangeInclusive<T>` of a type parameter `T`:
    /// Rawr's `RangeInclusive<f64>` or `RangeInclusive<i64>` at runtime as
    /// `T` is a float or an integer.
    RangeInclusive(Box<Type>),
    /// Rust's `Result<T, E>`, Rawr's builtin generic enum.
    Result(Box<Type>, Box<Type>),
    Callable {
        input: Box<Type>,
        output: Box<Type>,
    },
    #[cfg(feature = "rust-import")]
    ImportedFunctionItem {
        input: Box<Type>,
        output: Box<Type>,
    },
    Builtin(Builtin),
    Registered {
        path: host::path::Path,
        arguments: Box<[Type]>,
    },
    #[cfg(feature = "rust-import")]
    Imported {
        identity: host::type_def::import::Identity,
        arguments: Box<[Type]>,
    },
    External {
        export: Box<host::type_def::Export>,
        arguments: Box<[Type]>,
    },
    Parameter(host::path::Path),
}

/// The crate types a facade's items name: each it declares, with its
/// parameter count, and each another facade declares, with that facade's
/// export, in identity order.
#[cfg(feature = "rust-import")]
pub(crate) struct ImportedTypes {
    declared: Box<[(host::type_def::import::Identity, usize)]>,
    foreign: Box<[(host::type_def::import::Identity, host::type_def::Export)]>,
}

#[cfg(not(feature = "rust-import"))]
pub(crate) struct ImportedTypes;

impl Type {
    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>().checked_add(self.dynamic_bytes()?)
    }

    #[cfg(feature = "rust-import")]
    fn dynamic_bytes(&self) -> Option<usize> {
        fn slice(types: &[Type]) -> Option<usize> {
            types
                .iter()
                .try_fold(0_usize, |bytes, ty| bytes.checked_add(ty.retained_bytes()?))
        }

        match &self.0 {
            Shape::Unit | Shape::Builtin(_) => Some(0),
            Shape::Array { element, .. }
            | Shape::Option(element)
            | Shape::Reference {
                target: element, ..
            }
            | Shape::Iter(element)
            | Shape::Boxed(element)
            | Shape::Range(element)
            | Shape::RangeInclusive(element) => element.retained_bytes(),
            Shape::Result(ok, err) => ok
                .retained_bytes()
                .and_then(|bytes| bytes.checked_add(err.retained_bytes()?)),
            Shape::Tuple(types)
            | Shape::Imported {
                arguments: types, ..
            } => slice(types),
            Shape::Callable { input, output } | Shape::ImportedFunctionItem { input, output } => {
                input
                    .retained_bytes()
                    .and_then(|bytes| bytes.checked_add(output.retained_bytes()?))
            }
            Shape::Registered { path, arguments } => path
                .retained_bytes()
                .and_then(|bytes| bytes.checked_sub(std::mem::size_of::<host::path::Path>()))
                .and_then(|bytes| bytes.checked_add(slice(arguments)?)),
            Shape::External { export, arguments } => export
                .retained_bytes()
                .and_then(|bytes| bytes.checked_add(slice(arguments)?)),
            Shape::Parameter(path) => path
                .retained_bytes()
                .and_then(|bytes| bytes.checked_sub(std::mem::size_of::<host::path::Path>())),
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        fn slice(types: &[Type]) -> Result<Box<[Type]>, rawr_import::surface::Error> {
            let mut cloned = Vec::new();
            cloned
                .try_reserve_exact(types.len())
                .map_err(|_| rawr_import::surface::Error::Allocation)?;
            for ty in types {
                cloned.push(ty.try_clone()?);
            }
            Ok(cloned.into_boxed_slice())
        }

        Ok(Self(match &self.0 {
            Shape::Unit => Shape::Unit,
            Shape::Tuple(types) => Shape::Tuple(slice(types)?),
            Shape::Array { element, sequence } => Shape::Array {
                element: Box::new(element.try_clone()?),
                sequence: *sequence,
            },
            Shape::Option(element) => Shape::Option(Box::new(element.try_clone()?)),
            Shape::Reference { mutable, target } => Shape::Reference {
                mutable: *mutable,
                target: Box::new(target.try_clone()?),
            },
            Shape::Iter(element) => Shape::Iter(Box::new(element.try_clone()?)),
            Shape::Boxed(element) => Shape::Boxed(Box::new(element.try_clone()?)),
            Shape::Range(element) => Shape::Range(Box::new(element.try_clone()?)),
            Shape::RangeInclusive(element) => Shape::RangeInclusive(Box::new(element.try_clone()?)),
            Shape::Result(ok, err) => {
                Shape::Result(Box::new(ok.try_clone()?), Box::new(err.try_clone()?))
            }
            Shape::Callable { input, output } => Shape::Callable {
                input: Box::new(input.try_clone()?),
                output: Box::new(output.try_clone()?),
            },
            Shape::ImportedFunctionItem { input, output } => Shape::ImportedFunctionItem {
                input: Box::new(input.try_clone()?),
                output: Box::new(output.try_clone()?),
            },
            Shape::Builtin(builtin) => Shape::Builtin(*builtin),
            Shape::Registered { path, arguments } => Shape::Registered {
                path: path.try_clone()?,
                arguments: slice(arguments)?,
            },
            Shape::Imported {
                identity,
                arguments,
            } => Shape::Imported {
                identity: *identity,
                arguments: slice(arguments)?,
            },
            Shape::External { export, arguments } => Shape::External {
                export: Box::new(export.try_clone()?),
                arguments: slice(arguments)?,
            },
            Shape::Parameter(path) => Shape::Parameter(path.try_clone()?),
        }))
    }

    /// Construct the unit type.
    #[must_use]
    pub const fn unit() -> Self {
        Self(Shape::Unit)
    }

    /// Construct an ordered structural tuple with at least two members.
    pub fn tuple(members: impl IntoIterator<Item = Self>) -> Result<Self, host::Error> {
        let members = collect_types(members)?;
        if members.len() < 2 {
            return Err(host::Error::InvalidTupleArity {
                actual: members.len(),
            });
        }
        Ok(Self(Shape::Tuple(members)))
    }

    /// Construct an array `[element; length]`.
    #[must_use]
    pub fn array(element: Self, length: usize) -> Self {
        Self(Shape::Array {
            element: Box::new(element),
            sequence: Sequence::Array(length),
        })
    }

    /// Construct a `Vec<element>`: an array at runtime.
    #[must_use]
    pub fn vec(element: Self) -> Self {
        Self(Shape::Array {
            element: Box::new(element),
            sequence: Sequence::Vec,
        })
    }

    /// Construct a slice `[element]` of any length: an array at runtime.
    #[must_use]
    pub fn slice(element: Self) -> Self {
        Self(Shape::Array {
            element: Box::new(element),
            sequence: Sequence::Slice,
        })
    }

    /// Construct Rust's `Option<element>`, a Rawr option at runtime.
    #[must_use]
    pub fn option(element: Self) -> Self {
        Self(Shape::Option(Box::new(element)))
    }

    /// `&target`, or `&mut target`, an imported input borrows.
    #[must_use]
    pub fn reference(mutable: bool, target: Self) -> Self {
        Self(Shape::Reference {
            mutable,
            target: Box::new(target),
        })
    }

    /// Construct an iterator of `element` (`impl Iterator<Item = element>`),
    /// the array of its items at runtime.
    #[must_use]
    pub fn iterator(element: Self) -> Self {
        Self(Shape::Iter(Box::new(element)))
    }

    /// Construct Rust's `Box<element>`, rawr's `Box`, which holds its
    /// value as itself at runtime.
    #[must_use]
    pub fn boxed(element: Self) -> Self {
        Self(Shape::Boxed(Box::new(element)))
    }

    /// Construct Rust's `std::ops::Range<element>` of a float, a
    /// `Range<f64>` at runtime.
    #[must_use]
    pub fn range(element: Self) -> Self {
        Self(Shape::Range(Box::new(element)))
    }

    /// Construct Rust's `RangeInclusive<element>` of a type parameter, a
    /// Rawr float or integer range at runtime as the parameter is a float
    /// or an integer.
    #[must_use]
    pub fn range_inclusive(element: Self) -> Self {
        Self(Shape::RangeInclusive(Box::new(element)))
    }

    /// Rust's `RangeInclusive<element>` as Rawr holds it: its float range
    /// for a float, its integer range for an integer, and a range of a
    /// type parameter otherwise.
    #[cfg(feature = "rust-import")]
    pub(crate) fn range_inclusive_of(element: Self) -> Self {
        match element.shape() {
            Shape::Builtin(Builtin::F32 | Builtin::F64) => {
                Self::builtin(Builtin::FloatInclusiveRange)
            }
            Shape::Builtin(
                Builtin::I8
                | Builtin::I16
                | Builtin::I32
                | Builtin::I64
                | Builtin::Isize
                | Builtin::U8
                | Builtin::U16
                | Builtin::U32
                | Builtin::U64
                | Builtin::Usize,
            ) => Self::builtin(Builtin::InclusiveRange),
            _ => Self::range_inclusive(element),
        }
    }

    /// Construct Rust's `Result<ok, err>`, a Rawr result at runtime.
    #[must_use]
    pub fn result(ok: Self, err: Self) -> Self {
        Self(Shape::Result(Box::new(ok), Box::new(err)))
    }

    /// Construct an exact symbolic callable with one owned input and one output.
    #[must_use]
    pub fn callable(input: Self, output: Self) -> Self {
        Self(Shape::Callable {
            input: Box::new(input),
            output: Box::new(output),
        })
    }

    /// Construct one compiler-only imported function-item operand.
    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_function_item(input: Self, output: Self) -> Self {
        Self(Shape::ImportedFunctionItem {
            input: Box::new(input),
            output: Box::new(output),
        })
    }

    /// Construct a builtin type.
    #[must_use]
    pub const fn builtin(builtin: Builtin) -> Self {
        Self(Shape::Builtin(builtin))
    }

    /// Construct a reference to a non-generic type declared by this facade.
    pub fn registered(path: impl Into<crate::ImmutableString>) -> Result<Self, host::Error> {
        Ok(Self(Shape::Registered {
            path: host::path::Path::parse(path)?,
            arguments: Box::default(),
        }))
    }

    /// Construct an applied type declared by this facade.
    pub fn applied(
        path: impl Into<crate::ImmutableString>,
        arguments: impl IntoIterator<Item = Self>,
    ) -> Result<Self, host::Error> {
        Ok(Self(Shape::Registered {
            path: host::path::Path::parse(path)?,
            arguments: collect_types(arguments)?,
        }))
    }

    #[cfg(all(test, feature = "rust-import"))]
    pub(crate) fn imported(
        identity: host::type_def::import::Identity,
        arguments: impl IntoIterator<Item = Self>,
    ) -> Self {
        Self(Shape::Imported {
            identity,
            arguments: collect_types(arguments).expect("test imported type arguments fit"),
        })
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_boxed(
        identity: host::type_def::import::Identity,
        arguments: Box<[Self]>,
    ) -> Self {
        Self(Shape::Imported {
            identity,
            arguments,
        })
    }

    /// Construct a type reference authenticated by another finished facade.
    pub fn external(
        export: host::type_def::Export,
        arguments: impl IntoIterator<Item = Self>,
    ) -> Result<Self, host::Error> {
        // The item that names it checks its arity when its facade is
        // finished, where the error can name the item.
        let arguments = collect_types(arguments)?;
        Ok(Self(Shape::External {
            export: Box::new(export),
            arguments,
        }))
    }

    /// Construct a reference to a signature type parameter.
    pub fn parameter(name: impl Into<crate::ImmutableString>) -> Result<Self, host::Error> {
        let path = host::path::Path::parse(name)?;
        if path.segments().len() != 1 {
            return Err(host::Error::InvalidTypeParameter(path.to_string().into()));
        }
        Ok(Self(Shape::Parameter(path)))
    }

    pub(crate) fn resolve(
        &self,
        root: &host::path::Path,
        types: &[host::type_def::Definition],
        parameters: &[(host::path::Path, crate::capability::Set)],
        _imported: &ImportedTypes,
        item: &host::path::Path,
    ) -> Result<Self, host::Error> {
        match &self.0 {
            Shape::Unit => Ok(Self::unit()),
            Shape::Tuple(members) => Ok(Self(Shape::Tuple(resolve_types(
                members, root, types, parameters, _imported, item,
            )?))),
            Shape::Array { element, sequence } => Ok(Self(Shape::Array {
                element: Box::new(element.resolve(root, types, parameters, _imported, item)?),
                sequence: *sequence,
            })),
            Shape::Reference { mutable, target } => Ok(Self::reference(
                *mutable,
                target.resolve(root, types, parameters, _imported, item)?,
            )),
            Shape::Option(element) => Ok(Self::option(
                element.resolve(root, types, parameters, _imported, item)?,
            )),
            Shape::Iter(element) => Ok(Self::iterator(
                element.resolve(root, types, parameters, _imported, item)?,
            )),
            Shape::Boxed(element) => Ok(Self::boxed(
                element.resolve(root, types, parameters, _imported, item)?,
            )),
            Shape::Range(element) => Ok(Self::range(
                element.resolve(root, types, parameters, _imported, item)?,
            )),
            Shape::RangeInclusive(element) => Ok(Self::range_inclusive(
                element.resolve(root, types, parameters, _imported, item)?,
            )),
            Shape::Result(ok, err) => Ok(Self::result(
                ok.resolve(root, types, parameters, _imported, item)?,
                err.resolve(root, types, parameters, _imported, item)?,
            )),
            Shape::Callable { input, output } => Ok(Self(Shape::Callable {
                input: Box::new(input.resolve(root, types, parameters, _imported, item)?),
                output: Box::new(output.resolve(root, types, parameters, _imported, item)?),
            })),
            #[cfg(feature = "rust-import")]
            Shape::ImportedFunctionItem { input, output } => {
                Ok(Self(Shape::ImportedFunctionItem {
                    input: Box::new(input.resolve(root, types, parameters, _imported, item)?),
                    output: Box::new(output.resolve(root, types, parameters, _imported, item)?),
                }))
            }
            Shape::Builtin(builtin) => Ok(Self::builtin(*builtin)),
            Shape::Parameter(name) => {
                if parameters
                    .binary_search_by(|(candidate, _)| candidate.cmp(name))
                    .is_err()
                {
                    return Err(host::Error::UnknownTypeParameter {
                        item: item.to_string().into(),
                        parameter: name.to_string().into(),
                    });
                }
                Ok(self.clone())
            }
            Shape::Registered { path, arguments } => {
                let qualified = host::path::Path::qualify(root, path);
                let Some(definition) = types
                    .binary_search_by(|definition| definition.path_ref().cmp(&qualified))
                    .ok()
                    .map(|index| &types[index])
                else {
                    return Err(host::Error::UnknownFacadeType {
                        item: item.to_string().into(),
                        ty: qualified.to_string().into(),
                    });
                };
                if definition.parameters().len() != arguments.len() {
                    return Err(host::Error::InvalidFacadeTypeArguments {
                        item: item.to_string().into(),
                        ty: qualified.to_string().into(),
                        expected: definition.parameters().len(),
                        actual: arguments.len(),
                    });
                }
                let arguments = resolve_types(arguments, root, types, parameters, _imported, item)?;
                Ok(Self(Shape::Registered {
                    path: qualified,
                    arguments,
                }))
            }
            #[cfg(feature = "rust-import")]
            Shape::Imported {
                identity,
                arguments,
            } => {
                // The importer declares each crate type its bindings' items
                // name.
                let expected = _imported.parameters(identity).unwrap_or_else(|| {
                    panic!("the bindings declare each crate type `{item}`'s signature names")
                });
                // The importer writes each of a crate type's uses with its
                // declared parameters.
                assert_eq!(
                    expected,
                    arguments.len(),
                    "a crate type's use in `{item}` gives `{}` its declared parameters",
                    types
                        .iter()
                        .find(|definition| definition.identity().imported() == Some(identity))
                        .map_or_else(|| "?".to_owned(), host::type_def::Definition::path)
                );
                Ok(Self(Shape::Imported {
                    identity: *identity,
                    arguments: resolve_types(arguments, root, types, parameters, _imported, item)?,
                }))
            }
            Shape::External { export, arguments } => {
                if export.parameters().len() != arguments.len() {
                    return Err(host::Error::InvalidFacadeTypeArguments {
                        item: item.to_string().into(),
                        ty: export.path().into(),
                        expected: export.parameters().len(),
                        actual: arguments.len(),
                    });
                }
                Ok(Self(Shape::External {
                    #[cfg(feature = "rust-import")]
                    export: Box::new(export.try_clone().map_err(|_| host::Error::Allocation)?),
                    #[cfg(not(feature = "rust-import"))]
                    export: export.clone(),
                    arguments: resolve_types(arguments, root, types, parameters, _imported, item)?,
                }))
            }
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn resolve_imported<A: host::ImportAllocation + ?Sized>(
        &mut self,
        root: &host::path::Path,
        types: &[host::type_def::Definition],
        parameters: &[(host::path::Path, crate::capability::Set)],
        imported: &ImportedTypes,
        item: &host::path::Path,
        allocation: &mut A,
    ) -> Result<(), host::Error> {
        match &mut self.0 {
            Shape::Unit | Shape::Builtin(_) => {}
            Shape::Tuple(members) => {
                for member in members {
                    member.resolve_imported(root, types, parameters, imported, item, allocation)?;
                }
            }
            Shape::Array { element, .. }
            | Shape::Option(element)
            | Shape::Reference {
                target: element, ..
            }
            | Shape::Iter(element)
            | Shape::Boxed(element)
            | Shape::Range(element)
            | Shape::RangeInclusive(element) => {
                element.resolve_imported(root, types, parameters, imported, item, allocation)?;
            }
            Shape::Result(ok, err) => {
                ok.resolve_imported(root, types, parameters, imported, item, allocation)?;
                err.resolve_imported(root, types, parameters, imported, item, allocation)?;
            }
            Shape::Callable { input, output } => {
                input.resolve_imported(root, types, parameters, imported, item, allocation)?;
                output.resolve_imported(root, types, parameters, imported, item, allocation)?;
            }
            Shape::ImportedFunctionItem { input, output } => {
                input.resolve_imported(root, types, parameters, imported, item, allocation)?;
                output.resolve_imported(root, types, parameters, imported, item, allocation)?;
            }
            Shape::Parameter(name) => {
                if parameters
                    .binary_search_by(|(candidate, _)| candidate.cmp(name))
                    .is_err()
                {
                    return Err(host::Error::UnknownTypeParameter {
                        item: item.to_string().into(),
                        parameter: name.to_string().into(),
                    });
                }
            }
            Shape::Registered { path, arguments } => {
                let relative = path.take();
                let qualified = host::path::Path::qualify_imported(root, relative, allocation)?;
                let Some(definition) = types
                    .binary_search_by(|definition| definition.path_ref().cmp(&qualified))
                    .ok()
                    .map(|index| &types[index])
                else {
                    return Err(host::Error::UnknownFacadeType {
                        item: item.to_string().into(),
                        ty: qualified.to_string().into(),
                    });
                };
                if definition.parameters().len() != arguments.len() {
                    return Err(host::Error::InvalidFacadeTypeArguments {
                        item: item.to_string().into(),
                        ty: qualified.to_string().into(),
                        expected: definition.parameters().len(),
                        actual: arguments.len(),
                    });
                }
                *path = qualified;
                for argument in arguments {
                    argument
                        .resolve_imported(root, types, parameters, imported, item, allocation)?;
                }
            }
            Shape::Imported {
                identity,
                arguments,
            } => {
                for argument in arguments.iter_mut() {
                    argument
                        .resolve_imported(root, types, parameters, imported, item, allocation)?;
                }
                // A type another facade declares is that facade's, as a
                // path into a dependency is that crate's.
                if let Some(export) = imported.foreign(identity) {
                    assert_eq!(
                        export.parameters().len(),
                        arguments.len(),
                        "a crate type's use in `{item}` gives `{}` its declared parameters",
                        export.path()
                    );
                    allocation
                        .retain_checked(Some(1), export.retained_bytes())
                        .map_err(|_| host::Error::Allocation)?;
                    let export = Box::new(export.try_clone().map_err(|_| host::Error::Allocation)?);
                    let arguments = std::mem::take(arguments);
                    self.0 = Shape::External { export, arguments };
                    return Ok(());
                }
                // The importer declares each crate type its bindings' items
                // name, here or in another facade.
                let expected = imported.parameters(identity).unwrap_or_else(|| {
                    panic!("the bindings declare each crate type `{item}`'s signature names")
                });
                // The importer writes each of a crate type's uses with its
                // declared parameters.
                assert_eq!(
                    expected,
                    arguments.len(),
                    "a crate type's use in `{item}` gives `{}` its declared parameters",
                    types
                        .iter()
                        .find(|definition| definition.identity().imported() == Some(identity))
                        .map_or_else(|| "?".to_owned(), host::type_def::Definition::path)
                );
            }
            Shape::External { export, arguments } => {
                if export.parameters().len() != arguments.len() {
                    return Err(host::Error::InvalidFacadeTypeArguments {
                        item: item.to_string().into(),
                        ty: export.path().into(),
                        expected: export.parameters().len(),
                        actual: arguments.len(),
                    });
                }
                for argument in arguments {
                    argument
                        .resolve_imported(root, types, parameters, imported, item, allocation)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) const fn shape(&self) -> &Shape {
        &self.0
    }

    /// Whether this type holds no native value: a number, `bool`, `char`,
    /// `String` or the unit, alone or in an `Option`, a `Result`, a `Vec`,
    /// an array or a tuple. Rawr holds such a value as its own, so a field
    /// of it is read in place, as the generated bridge's `plain_value` reads
    /// it.
    #[cfg(feature = "rust-import")]
    pub(crate) fn plain_value(&self) -> bool {
        match &self.0 {
            Shape::Unit => true,
            Shape::Builtin(builtin) => builtin.name().is_some(),
            Shape::Option(inner) | Shape::Array { element: inner, .. } => inner.plain_value(),
            Shape::Result(ok, err) => ok.plain_value() && err.plain_value(),
            Shape::Tuple(members) => members.iter().all(Self::plain_value),
            // One of std's types Rawr's std holds as its own, as `PathBuf`.
            Shape::External { export, .. } => {
                export.facade_root().to_string() == crate::std_facade::ROOT
                    && export
                        .contract()
                        .capabilities()
                        .contains(crate::capability::Kind::Clone)
            }
            _ => false,
        }
    }

    pub(crate) fn collect_exports<'a>(&'a self, exports: &mut Vec<&'a host::type_def::Export>) {
        match &self.0 {
            Shape::Tuple(members) => {
                for member in members {
                    member.collect_exports(exports);
                }
            }
            Shape::Array { element, .. }
            | Shape::Option(element)
            | Shape::Reference {
                target: element, ..
            }
            | Shape::Iter(element)
            | Shape::Boxed(element)
            | Shape::Range(element)
            | Shape::RangeInclusive(element) => {
                element.collect_exports(exports);
            }
            Shape::Result(ok, err) => {
                ok.collect_exports(exports);
                err.collect_exports(exports);
            }
            Shape::Callable { input, output } => {
                input.collect_exports(exports);
                output.collect_exports(exports);
            }
            #[cfg(feature = "rust-import")]
            Shape::ImportedFunctionItem { input, output } => {
                input.collect_exports(exports);
                output.collect_exports(exports);
            }
            Shape::Registered { arguments, .. } => {
                for argument in arguments {
                    argument.collect_exports(exports);
                }
            }
            #[cfg(feature = "rust-import")]
            Shape::Imported { arguments, .. } => {
                for argument in arguments {
                    argument.collect_exports(exports);
                }
            }
            Shape::External { export, arguments } => {
                exports.push(export);
                for argument in arguments {
                    argument.collect_exports(exports);
                }
            }
            Shape::Unit | Shape::Builtin(_) | Shape::Parameter(_) => {}
        }
    }
}

fn collect_types(types: impl IntoIterator<Item = Type>) -> Result<Box<[Type]>, host::Error> {
    let mut collected = Vec::new();
    for ty in types {
        collected
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        collected.push(ty);
    }
    Ok(collected.into_boxed_slice())
}

fn resolve_types(
    unresolved: &[Type],
    root: &host::path::Path,
    types: &[host::type_def::Definition],
    parameters: &[(host::path::Path, crate::capability::Set)],
    imported: &ImportedTypes,
    item: &host::path::Path,
) -> Result<Box<[Type]>, host::Error> {
    let mut resolved = Vec::new();
    resolved
        .try_reserve_exact(unresolved.len())
        .map_err(|_| host::Error::Allocation)?;
    for ty in unresolved {
        resolved.push(ty.resolve(root, types, parameters, imported, item)?);
    }
    Ok(resolved.into_boxed_slice())
}

impl crate::digest::Write for Type {
    fn write(&self, writer: &mut crate::digest::Writer) -> Result<(), crate::digest::Error> {
        use crate::digest::write_slice;

        match &self.0 {
            Shape::Unit => writer.tag(0x2301),
            Shape::Tuple(members) => {
                writer.tag(0x2302);
                write_slice(writer, members)?;
            }
            Shape::Array { element, sequence } => {
                writer.tag(0x230b);
                element.write(writer)?;
                writer.text(&match sequence {
                    Sequence::Array(length) => length.to_string(),
                    Sequence::Vec => "_".into(),
                    Sequence::Slice => "[]".into(),
                });
            }
            Shape::Option(element) => {
                writer.tag(0x230c);
                element.write(writer)?;
            }
            Shape::Reference { mutable, target } => {
                writer.tag(if *mutable { 0x2315 } else { 0x2314 });
                target.write(writer)?;
            }
            Shape::Iter(element) => {
                writer.tag(0x230e);
                element.write(writer)?;
            }
            Shape::Boxed(element) => {
                writer.tag(0x2317);
                element.write(writer)?;
            }
            Shape::Range(element) => {
                writer.tag(0x2313);
                element.write(writer)?;
            }
            Shape::RangeInclusive(element) => {
                writer.tag(0x2316);
                element.write(writer)?;
            }
            Shape::Result(ok, err) => {
                writer.tag(0x230d);
                ok.write(writer)?;
                err.write(writer)?;
            }
            Shape::Callable { input, output } => {
                writer.tag(0x2303);
                writer.tag(0x2311);
                input.write(writer)?;
                writer.tag(0x2312);
                output.write(writer)?;
            }
            #[cfg(feature = "rust-import")]
            Shape::ImportedFunctionItem { input, output } => {
                writer.tag(0x230a);
                writer.tag(0x2371);
                input.write(writer)?;
                writer.tag(0x2372);
                output.write(writer)?;
            }
            Shape::Builtin(builtin) => writer.tag(match builtin {
                Builtin::Bool => 0x2321,
                Builtin::Char => 0x2322,
                Builtin::F64 => 0x2323,
                Builtin::I64 => 0x2324,
                Builtin::String => 0x2325,
                Builtin::InclusiveRange => 0x2326,
                Builtin::FloatInclusiveRange => 0x2327,
                Builtin::U64 => 0x2328,
                Builtin::U32 => 0x2329,
                Builtin::I8 => 0x232a,
                Builtin::I16 => 0x232b,
                Builtin::I32 => 0x232c,
                Builtin::Isize => 0x232d,
                Builtin::U8 => 0x232e,
                Builtin::U16 => 0x232f,
                Builtin::Usize => 0x2330,
                Builtin::F32 => 0x2331,
                Builtin::Duration => 0x2333,
                Builtin::Instant => 0x2334,
            }),
            Shape::Registered { path, arguments } => {
                writer.tag(0x2305);
                writer.tag(0x2331);
                writer.text(&path.to_string());
                writer.tag(0x2332);
                write_slice(writer, arguments)?;
            }
            #[cfg(feature = "rust-import")]
            Shape::Imported {
                identity,
                arguments,
            } => {
                writer.tag(0x2308);
                writer.tag(0x2351);
                writer.bytes(identity.as_bytes());
                writer.tag(0x2352);
                write_slice(writer, arguments)?;
            }
            Shape::External { export, arguments } => {
                writer.tag(0x2307);
                writer.tag(0x2341);
                export.write(writer)?;
                writer.tag(0x2342);
                write_slice(writer, arguments)?;
            }
            Shape::Parameter(path) => {
                writer.tag(0x2306);
                writer.text(&path.to_string());
            }
        }
        Ok(())
    }
}

impl ImportedTypes {
    #[cfg(feature = "rust-import")]
    pub(crate) fn from_facade(types: &[host::type_def::Definition]) -> Result<Self, host::Error> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(types.len())
            .map_err(|_| host::Error::Allocation)?;
        for definition in types {
            let Some(identity) = definition.identity().imported() else {
                continue;
            };
            entries.push((*identity, definition.parameters().len()));
        }
        entries.sort_unstable();
        entries.dedup();
        Ok(Self {
            declared: entries.into_boxed_slice(),
            foreign: Box::default(),
        })
    }

    /// The types a facade's items name: those of `types`, and those of
    /// `foreign`, which other facades declare.
    #[cfg(feature = "rust-import")]
    pub(crate) fn from_imported_facade<A: host::ImportAllocation + ?Sized>(
        types: &[host::type_def::Definition],
        mut foreign: Box<[(host::type_def::import::Identity, host::type_def::Export)]>,
        allocation: &mut A,
    ) -> Result<Self, host::Error> {
        let count = types
            .iter()
            .filter(|definition| definition.identity().imported().is_some())
            .count();
        allocation
            .retain_checked(
                Some(count),
                count.checked_mul(std::mem::size_of::<(host::type_def::import::Identity, usize)>()),
            )
            .map_err(|_| host::Error::Allocation)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| host::Error::Allocation)?;
        for definition in types {
            let Some(identity) = definition.identity().imported() else {
                continue;
            };
            entries.push((*identity, definition.parameters().len()));
        }
        entries.sort_unstable();
        entries.dedup();
        foreign.sort_unstable_by_key(|(identity, _)| *identity);
        Ok(Self {
            declared: entries.into_boxed_slice(),
            foreign,
        })
    }

    #[cfg(not(feature = "rust-import"))]
    pub(crate) fn from_facade(_types: &[host::type_def::Definition]) -> Result<Self, host::Error> {
        Ok(Self)
    }

    #[cfg(feature = "rust-import")]
    fn parameters(&self, identity: &host::type_def::import::Identity) -> Option<usize> {
        self.declared
            .binary_search_by(|(candidate, _)| candidate.cmp(identity))
            .ok()
            .map(|index| self.declared[index].1)
    }

    /// The export of the facade that declares a type this one names and
    /// doesn't declare.
    #[cfg(feature = "rust-import")]
    fn foreign(
        &self,
        identity: &host::type_def::import::Identity,
    ) -> Option<&host::type_def::Export> {
        self.foreign
            .binary_search_by(|(candidate, _)| candidate.cmp(identity))
            .ok()
            .map(|index| &self.foreign[index].1)
    }
}

#[cfg(all(test, feature = "rust-import"))]
mod tests {
    use super::{Shape, Type};
    use crate::host;

    struct Subscription;

    #[test]
    fn imported_function_qualifies_registered_output_without_a_path_sentinel() {
        let root = host::path::Path::parse("iced").expect("test root is valid");
        let subscription = host::type_def::Definition::declared::<Subscription>(
            host::path::Path::parse("iced::Subscription").expect("test type path is valid"),
            &[],
            &[],
            host::type_def::Storage::Owned,
            host::type_def::Role::Ordinary,
            host::usage::Policy::Ordinary,
            host::type_def::Visibility::Public,
        )
        .expect("test type declaration is valid");
        let types = [subscription];
        let imported = super::ImportedTypes::from_facade(&types)
            .expect("test imported index allocation succeeds");
        let mut allocation = host::TestImportAllocation::default();

        let resolved = subscription_function()
            .resolve_imported(&root, &[], &types, &[], &imported, &mut allocation)
            .expect("registered output resolves inside the imported facade");

        assert_eq!(resolved.path(), "iced::listen");
        let Shape::Registered { path, arguments } = resolved.signature().output().shape() else {
            panic!("resolved output must remain a registered facade type");
        };
        assert_eq!(path.to_string(), "iced::Subscription");
        assert!(arguments.is_empty());
        assert!(path.segments().iter().all(|segment| segment != "_"));
    }

    #[test]
    fn imported_function_reports_the_exact_missing_registered_output() {
        let root = host::path::Path::parse("iced").expect("test root is valid");
        let imported = super::ImportedTypes::from_facade(&[])
            .expect("empty imported index allocation succeeds");
        let mut allocation = host::TestImportAllocation::default();

        let error = subscription_function()
            .resolve_imported(&root, &[], &[], &[], &imported, &mut allocation)
            .expect_err("missing registered type must be rejected");

        assert_eq!(
            error,
            host::Error::UnknownFacadeType {
                item: "iced::listen".into(),
                ty: "iced::Subscription".into(),
            }
        );
        assert!(!error.to_string().contains('_'));
    }

    fn subscription_function() -> host::item::Definition {
        let mut signature = host::signature::Builder::new();
        signature.output(Type::registered("Subscription").expect("test type path is valid"));
        host::item::Definition::function("listen", signature.finish())
            .expect("test function declaration is valid")
    }
}
