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

/// A validated structural type in a static host signature.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Type(Shape);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum Shape {
    Unit,
    Tuple(Box<[Type]>),
    /// `[T; N]`, or a slice `&[T]` without a length: an array at runtime.
    Array {
        element: Box<Type>,
        length: Option<usize>,
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
    Support(SupportTarget),
    Parameter(host::path::Path),
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum SupportTarget {
    Local(host::support::Local),
    External(Box<host::support::SupportExport>),
}

#[cfg(feature = "rust-import")]
pub(crate) struct ImportedTypes(Box<[(host::type_def::import::Identity, usize)]>);

#[cfg(not(feature = "rust-import"))]
pub(crate) struct ImportedTypes;

impl Type {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        fn types(values: &mut [Type], profile: &host::facade::ProfileFinalization<'_>) -> usize {
            values.iter_mut().fold(0_usize, |completed, value| {
                completed.saturating_add(value.normalize_profile(profile))
            })
        }

        match &mut self.0 {
            Shape::Unit | Shape::Builtin(_) | Shape::Parameter(_) => 0,
            Shape::Tuple(values)
            | Shape::Registered {
                arguments: values, ..
            } => types(values, profile),
            #[cfg(feature = "rust-import")]
            Shape::Imported {
                arguments: values, ..
            } => types(values, profile),
            Shape::Array { element, .. }
            | Shape::Option(element)
            | Shape::Reference {
                target: element, ..
            }
            | Shape::Iter(element)
            | Shape::Range(element)
            | Shape::RangeInclusive(element) => element.normalize_profile(profile),
            Shape::Result(ok, err) => ok
                .normalize_profile(profile)
                .saturating_add(err.normalize_profile(profile)),
            Shape::Callable { input, output } => input
                .normalize_profile(profile)
                .saturating_add(output.normalize_profile(profile)),
            #[cfg(feature = "rust-import")]
            Shape::ImportedFunctionItem { input, output } => input
                .normalize_profile(profile)
                .saturating_add(output.normalize_profile(profile)),
            Shape::External { export, arguments } => export
                .normalize_profile(profile)
                .saturating_add(types(arguments, profile)),
            Shape::Support(target) => match target {
                SupportTarget::Local(local) => local.normalize_profile(profile),
                SupportTarget::External(export) => export.normalize_profile(profile),
            },
        }
    }

    pub(crate) const fn signature_support(&self) -> bool {
        matches!(self.0, Shape::Support(_))
    }

    pub(crate) fn nested_signature_support(&self) -> Option<&'static str> {
        fn contains(ty: &Type) -> bool {
            match ty.shape() {
                Shape::Support(_) => true,
                Shape::Tuple(types)
                | Shape::Registered {
                    arguments: types, ..
                }
                | Shape::External {
                    arguments: types, ..
                } => types.iter().any(contains),
                #[cfg(feature = "rust-import")]
                Shape::Imported {
                    arguments: types, ..
                } => types.iter().any(contains),
                Shape::Array { element, .. }
                | Shape::Option(element)
                | Shape::Reference {
                    target: element, ..
                }
                | Shape::Iter(element)
                | Shape::Range(element)
                | Shape::RangeInclusive(element) => contains(element),
                Shape::Result(ok, err) => contains(ok) || contains(err),
                Shape::Callable { input, output } => contains(input) || contains(output),
                #[cfg(feature = "rust-import")]
                Shape::ImportedFunctionItem { input, output } => {
                    contains(input) || contains(output)
                }
                Shape::Unit | Shape::Builtin(_) | Shape::Parameter(_) => false,
            }
        }

        if self.signature_support() {
            return None;
        }
        match self.shape() {
            Shape::Tuple(types) if types.iter().any(contains) => Some("tuple"),
            Shape::Array { element, .. } if contains(element) => Some("array"),
            Shape::Iter(element) if contains(element) => Some("iterator"),
            Shape::Range(element) | Shape::RangeInclusive(element) if contains(element) => {
                Some("range")
            }
            Shape::Option(element) if contains(element) => Some("option"),
            Shape::Reference { target, .. } if contains(target) => Some("reference"),
            Shape::Result(ok, err) if contains(ok) || contains(err) => Some("result"),
            Shape::Callable { input, output } if contains(input) || contains(output) => {
                Some("callable")
            }
            #[cfg(feature = "rust-import")]
            Shape::ImportedFunctionItem { input, output }
                if contains(input) || contains(output) =>
            {
                Some("imported function item")
            }
            Shape::Registered { arguments, .. } if arguments.iter().any(contains) => {
                Some("registered type application")
            }
            #[cfg(feature = "rust-import")]
            Shape::Imported { arguments, .. } if arguments.iter().any(contains) => {
                Some("imported type application")
            }
            Shape::External { arguments, .. } if arguments.iter().any(contains) => {
                Some("external type application")
            }
            Shape::Unit
            | Shape::Tuple(_)
            | Shape::Array { .. }
            | Shape::Reference { .. }
            | Shape::Option(_)
            | Shape::Iter(_)
            | Shape::Range(_)
            | Shape::RangeInclusive(_)
            | Shape::Result(..)
            | Shape::Callable { .. }
            | Shape::Builtin(_)
            | Shape::Registered { .. }
            | Shape::External { .. }
            | Shape::Support(_)
            | Shape::Parameter(_) => None,
            #[cfg(feature = "rust-import")]
            Shape::Imported { .. } | Shape::ImportedFunctionItem { .. } => None,
        }
    }

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
            Shape::Support(target) => match target {
                SupportTarget::Local(local) => local.contract().retained_bytes(),
                SupportTarget::External(export) => export.retained_bytes(),
            },
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
            Shape::Array { element, length } => Shape::Array {
                element: Box::new(element.try_clone()?),
                length: *length,
            },
            Shape::Option(element) => Shape::Option(Box::new(element.try_clone()?)),
            Shape::Reference { mutable, target } => Shape::Reference {
                mutable: *mutable,
                target: Box::new(target.try_clone()?),
            },
            Shape::Iter(element) => Shape::Iter(Box::new(element.try_clone()?)),
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
            Shape::Support(target) => Shape::Support(match target {
                SupportTarget::Local(local) => SupportTarget::Local(local.try_clone()?),
                SupportTarget::External(export) => {
                    SupportTarget::External(Box::new(export.try_clone()?))
                }
            }),
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
            length: Some(length),
        })
    }

    /// Construct a slice `&[element]` of any length: an array at runtime.
    #[must_use]
    pub fn slice(element: Self) -> Self {
        Self(Shape::Array {
            element: Box::new(element),
            length: None,
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
        let arguments = collect_types(arguments)?;
        if export.parameters().len() != arguments.len() {
            return Err(host::Error::InvalidFacadeTypeArguments {
                item: "<external signature>".into(),
                ty: export.path().into(),
                expected: export.parameters().len(),
                actual: arguments.len(),
            });
        }
        Ok(Self(Shape::External {
            export: Box::new(export),
            arguments,
        }))
    }

    /// Construct one same-facade signature-only support reference.
    #[doc(hidden)]
    #[must_use]
    pub fn support(local: host::support::Local) -> Self {
        Self(Shape::Support(SupportTarget::Local(local)))
    }

    /// Construct one cross-facade signature-only support reference.
    #[doc(hidden)]
    #[must_use]
    pub fn support_external(export: host::support::SupportExport) -> Self {
        Self(Shape::Support(SupportTarget::External(Box::new(export))))
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
        supports: &[host::support::SignatureContract],
        parameters: &[(host::path::Path, crate::capability::Set)],
        _imported: &ImportedTypes,
        item: &host::path::Path,
    ) -> Result<Self, host::Error> {
        match &self.0 {
            Shape::Unit => Ok(Self::unit()),
            Shape::Tuple(members) => Ok(Self(Shape::Tuple(resolve_types(
                members, root, types, supports, parameters, _imported, item,
            )?))),
            Shape::Array { element, length } => Ok(Self(Shape::Array {
                element: Box::new(
                    element.resolve(root, types, supports, parameters, _imported, item)?,
                ),
                length: *length,
            })),
            Shape::Reference { mutable, target } => Ok(Self::reference(
                *mutable,
                target.resolve(root, types, supports, parameters, _imported, item)?,
            )),
            Shape::Option(element) => Ok(Self::option(
                element.resolve(root, types, supports, parameters, _imported, item)?,
            )),
            Shape::Iter(element) => Ok(Self::iterator(
                element.resolve(root, types, supports, parameters, _imported, item)?,
            )),
            Shape::Range(element) => Ok(Self::range(
                element.resolve(root, types, supports, parameters, _imported, item)?,
            )),
            Shape::RangeInclusive(element) => Ok(Self::range_inclusive(
                element.resolve(root, types, supports, parameters, _imported, item)?,
            )),
            Shape::Result(ok, err) => Ok(Self::result(
                ok.resolve(root, types, supports, parameters, _imported, item)?,
                err.resolve(root, types, supports, parameters, _imported, item)?,
            )),
            Shape::Callable { input, output } => Ok(Self(Shape::Callable {
                input: Box::new(input.resolve(root, types, supports, parameters, _imported, item)?),
                output: Box::new(
                    output.resolve(root, types, supports, parameters, _imported, item)?,
                ),
            })),
            #[cfg(feature = "rust-import")]
            Shape::ImportedFunctionItem { input, output } => {
                Ok(Self(Shape::ImportedFunctionItem {
                    input: Box::new(
                        input.resolve(root, types, supports, parameters, _imported, item)?,
                    ),
                    output: Box::new(
                        output.resolve(root, types, supports, parameters, _imported, item)?,
                    ),
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
                let arguments = resolve_types(
                    arguments, root, types, supports, parameters, _imported, item,
                )?;
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
                let Some(expected) = _imported.parameters(identity) else {
                    return Err(host::Error::UnknownImportedType {
                        item: item.to_string().into(),
                        identity: *identity.as_bytes(),
                    });
                };
                if expected != arguments.len() {
                    return Err(host::Error::InvalidFacadeTypeArguments {
                        item: item.to_string().into(),
                        ty: "<imported>".into(),
                        expected,
                        actual: arguments.len(),
                    });
                }
                Ok(Self(Shape::Imported {
                    identity: *identity,
                    arguments: resolve_types(
                        arguments, root, types, supports, parameters, _imported, item,
                    )?,
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
                    arguments: resolve_types(
                        arguments, root, types, supports, parameters, _imported, item,
                    )?,
                }))
            }
            Shape::Support(target) => {
                if let SupportTarget::Local(local) = target {
                    let contract = local.contract();
                    let exact = supports
                        .binary_search_by(|candidate| candidate.path_ref().cmp(contract.path_ref()))
                        .ok()
                        .map(|index| &supports[index]);
                    if exact != Some(contract) {
                        return Err(host::Error::UnknownFacadeType {
                            item: item.to_string().into(),
                            ty: contract.path().into(),
                        });
                    }
                }
                Ok(Self(Shape::Support(target.clone())))
            }
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn resolve_imported<A: host::ImportAllocation + ?Sized>(
        &mut self,
        root: &host::path::Path,
        types: &[host::type_def::Definition],
        supports: &[host::support::SignatureContract],
        parameters: &[(host::path::Path, crate::capability::Set)],
        imported: &ImportedTypes,
        item: &host::path::Path,
        allocation: &mut A,
    ) -> Result<(), host::Error> {
        match &mut self.0 {
            Shape::Unit | Shape::Builtin(_) => {}
            Shape::Tuple(members) => {
                for member in members {
                    member.resolve_imported(
                        root, types, supports, parameters, imported, item, allocation,
                    )?;
                }
            }
            Shape::Array { element, .. }
            | Shape::Option(element)
            | Shape::Reference {
                target: element, ..
            }
            | Shape::Iter(element)
            | Shape::Range(element)
            | Shape::RangeInclusive(element) => {
                element.resolve_imported(
                    root, types, supports, parameters, imported, item, allocation,
                )?;
            }
            Shape::Result(ok, err) => {
                ok.resolve_imported(
                    root, types, supports, parameters, imported, item, allocation,
                )?;
                err.resolve_imported(
                    root, types, supports, parameters, imported, item, allocation,
                )?;
            }
            Shape::Callable { input, output } => {
                input.resolve_imported(
                    root, types, supports, parameters, imported, item, allocation,
                )?;
                output.resolve_imported(
                    root, types, supports, parameters, imported, item, allocation,
                )?;
            }
            Shape::ImportedFunctionItem { input, output } => {
                input.resolve_imported(
                    root, types, supports, parameters, imported, item, allocation,
                )?;
                output.resolve_imported(
                    root, types, supports, parameters, imported, item, allocation,
                )?;
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
                    argument.resolve_imported(
                        root, types, supports, parameters, imported, item, allocation,
                    )?;
                }
            }
            Shape::Imported {
                identity,
                arguments,
            } => {
                let Some(expected) = imported.parameters(identity) else {
                    return Err(host::Error::UnknownImportedType {
                        item: item.to_string().into(),
                        identity: *identity.as_bytes(),
                    });
                };
                if expected != arguments.len() {
                    return Err(host::Error::InvalidFacadeTypeArguments {
                        item: item.to_string().into(),
                        ty: "<imported>".into(),
                        expected,
                        actual: arguments.len(),
                    });
                }
                for argument in arguments {
                    argument.resolve_imported(
                        root, types, supports, parameters, imported, item, allocation,
                    )?;
                }
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
                    argument.resolve_imported(
                        root, types, supports, parameters, imported, item, allocation,
                    )?;
                }
            }
            Shape::Support(SupportTarget::Local(local)) => {
                let contract = local.contract();
                let exact = supports
                    .binary_search_by(|candidate| candidate.path_ref().cmp(contract.path_ref()))
                    .ok()
                    .map(|index| &supports[index]);
                if exact != Some(contract) {
                    return Err(host::Error::UnknownFacadeType {
                        item: item.to_string().into(),
                        ty: contract.path().into(),
                    });
                }
            }
            Shape::Support(SupportTarget::External(_)) => {}
        }
        Ok(())
    }

    pub(crate) const fn shape(&self) -> &Shape {
        &self.0
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
            Shape::Support(_) | Shape::Unit | Shape::Builtin(_) | Shape::Parameter(_) => {}
        }
    }

    pub(crate) fn collect_support_exports<'a>(
        &'a self,
        exports: &mut Vec<&'a host::support::SupportExport>,
    ) {
        match &self.0 {
            Shape::Tuple(members) => {
                for member in members {
                    member.collect_support_exports(exports);
                }
            }
            Shape::Array { element, .. }
            | Shape::Option(element)
            | Shape::Reference {
                target: element, ..
            }
            | Shape::Iter(element)
            | Shape::Range(element)
            | Shape::RangeInclusive(element) => {
                element.collect_support_exports(exports);
            }
            Shape::Result(ok, err) => {
                ok.collect_support_exports(exports);
                err.collect_support_exports(exports);
            }
            Shape::Callable { input, output } => {
                input.collect_support_exports(exports);
                output.collect_support_exports(exports);
            }
            #[cfg(feature = "rust-import")]
            Shape::ImportedFunctionItem { input, output } => {
                input.collect_support_exports(exports);
                output.collect_support_exports(exports);
            }
            Shape::Registered { arguments, .. } | Shape::External { arguments, .. } => {
                for argument in arguments {
                    argument.collect_support_exports(exports);
                }
            }
            #[cfg(feature = "rust-import")]
            Shape::Imported { arguments, .. } => {
                for argument in arguments {
                    argument.collect_support_exports(exports);
                }
            }
            Shape::Support(SupportTarget::External(export)) => exports.push(export),
            Shape::Support(SupportTarget::Local(_))
            | Shape::Unit
            | Shape::Builtin(_)
            | Shape::Parameter(_) => {}
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
    supports: &[host::support::SignatureContract],
    parameters: &[(host::path::Path, crate::capability::Set)],
    imported: &ImportedTypes,
    item: &host::path::Path,
) -> Result<Box<[Type]>, host::Error> {
    let mut resolved = Vec::new();
    resolved
        .try_reserve_exact(unresolved.len())
        .map_err(|_| host::Error::Allocation)?;
    for ty in unresolved {
        resolved.push(ty.resolve(root, types, supports, parameters, imported, item)?);
    }
    Ok(resolved.into_boxed_slice())
}

impl crate::preview::digest::Write for Type {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        use crate::preview::digest::write_slice;

        match &self.0 {
            Shape::Unit => writer.tag(0x2301),
            Shape::Tuple(members) => {
                writer.tag(0x2302);
                write_slice(writer, members)?;
            }
            Shape::Array { element, length } => {
                writer.tag(0x230b);
                element.write(writer)?;
                writer.text(&length.map_or_else(|| "_".into(), |length| length.to_string()));
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
            Shape::Support(target) => {
                writer.tag(0x2309);
                match target {
                    SupportTarget::Local(local) => {
                        writer.tag(0x2361);
                        local.contract().write(writer)?;
                    }
                    SupportTarget::External(export) => {
                        writer.tag(0x2362);
                        export.contract().write(writer)?;
                        writer.tag(0x2363);
                        export.dependency().write(writer)?;
                    }
                }
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
        Ok(Self(entries.into_boxed_slice()))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn from_imported_facade<A: host::ImportAllocation + ?Sized>(
        types: &[host::type_def::Definition],
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
        Ok(Self(entries.into_boxed_slice()))
    }

    #[cfg(not(feature = "rust-import"))]
    pub(crate) fn from_facade(_types: &[host::type_def::Definition]) -> Result<Self, host::Error> {
        Ok(Self)
    }

    #[cfg(feature = "rust-import")]
    fn parameters(&self, identity: &host::type_def::import::Identity) -> Option<usize> {
        self.0
            .binary_search_by(|(candidate, _)| candidate.cmp(identity))
            .ok()
            .map(|index| self.0[index].1)
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
            .resolve_imported(&root, &[], &types, &[], &[], &imported, &mut allocation)
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
            .resolve_imported(&root, &[], &[], &[], &[], &imported, &mut allocation)
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
