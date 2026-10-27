//! Typed callable signatures in static host facades.

use crate::{capability, host, ImmutableString};

/// A validated callable signature before facade path resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Definition {
    type_parameters: Box<[(host::path::Path, capability::Set)]>,
    /// Each type parameter's index in `type_parameters`, in the order the
    /// item declares them (`fn f<T, R>` is `T`, then `R`).
    declared: Box<[u32]>,
    parameters: Box<[host::type_ref::Type]>,
    output: host::type_ref::Type,
    /// What the item's bounds tie between its type parameters.
    relations: Box<[Relation]>,
    /// Its callable parameters take a script function as a slot (D594).
    crossing: bool,
}

/// A bound `R: Trait<A..>` on an imported generic item's type parameter
/// `R`, with the trait's implementations the importer could name. rand's
/// `random_range` has `R: SampleRange<T>`, and rand implements
/// `SampleRange<T>` for `Range<T>`. When an argument fills `R` with one
/// case's self type, the case's trait arguments are `A..`, as rustc's
/// trait selection infers them: `random_range(0.5..1.0)` gives the `T` of
/// the range's floats.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Relation {
    parameter: ImmutableString,
    arguments: Box<[Pattern]>,
    cases: Box<[Case]>,
}

/// One implementation of a [`Relation`]'s trait.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Case {
    this: Pattern,
    arguments: Box<[Pattern]>,
}

/// A type in a [`Relation`]: the item's type parameter, an
/// implementation's own type parameter, or a type made of them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Pattern {
    /// The item's type parameter by name.
    Parameter(ImmutableString),
    /// An implementation's type parameter, by its index.
    Variable(u32),
    /// `()`.
    Unit,
    /// A builtin.
    Builtin(host::type_ref::Builtin),
    /// `Range<T>` of a float.
    Range(Box<Pattern>),
    /// `Option<T>`.
    Option(Box<Pattern>),
    /// `(A, B, ..)`.
    Tuple(Box<[Pattern]>),
    /// A type without variables, as Freya's `Fill` or `Color` is.
    Type(host::type_ref::Type),
}

impl Relation {
    /// A bound on `parameter` whose trait arguments are `arguments`.
    #[must_use]
    pub fn new(
        parameter: impl Into<ImmutableString>,
        arguments: Box<[Pattern]>,
        cases: Box<[Case]>,
    ) -> Self {
        Self {
            parameter: parameter.into(),
            arguments,
            cases,
        }
    }

    pub(crate) fn parameter(&self) -> &str {
        &self.parameter
    }

    pub(crate) fn arguments(&self) -> &[Pattern] {
        &self.arguments
    }

    pub(crate) fn cases(&self) -> &[Case] {
        &self.cases
    }
}

impl Case {
    /// An implementation for `this` with the trait arguments `arguments`.
    #[must_use]
    pub const fn new(this: Pattern, arguments: Box<[Pattern]>) -> Self {
        Self { this, arguments }
    }

    pub(crate) const fn this(&self) -> &Pattern {
        &self.this
    }

    pub(crate) fn arguments(&self) -> &[Pattern] {
        &self.arguments
    }
}

/// A deterministic exact structural signature proof.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint {
    type_parameters: Box<[(ImmutableString, capability::Set)]>,
    parameters: Box<[host::type_ref::Type]>,
    output: host::type_ref::Type,
}

/// Builds one structural host callable signature.
#[derive(Clone, Debug, Default)]
pub struct Builder {
    type_parameters: Vec<(host::path::Path, capability::Set)>,
    declared: Vec<u32>,
    parameters: Vec<host::type_ref::Type>,
    output: Option<host::type_ref::Type>,
    relations: Vec<Relation>,
    crossing: bool,
}

impl Builder {
    /// Create an empty signature whose output defaults to `()`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            type_parameters: Vec::new(),
            declared: Vec::new(),
            parameters: Vec::new(),
            output: None,
            relations: Vec::new(),
            crossing: false,
        }
    }

    /// Mark the item's callable parameters as taking a script function as a
    /// slot (D594).
    #[cfg(feature = "rust-import")]
    pub(crate) fn crossing(&mut self) -> &mut Self {
        self.crossing = true;
        self
    }

    /// Declare a type parameter and its exact capability requirements.
    pub fn type_parameter(
        &mut self,
        name: impl Into<ImmutableString>,
        requirements: &[capability::Kind],
    ) -> Result<&mut Self, host::Error> {
        let name = name.into();
        let path = host::path::Path::parse(name.clone())?;
        if path.segments().len() != 1 {
            return Err(host::Error::InvalidTypeParameter(name));
        }
        let requirements = capability::Set::from_explicit(requirements.iter().copied());
        host::type_def::validate_capabilities(&name, requirements)?;
        let index = match self
            .type_parameters
            .binary_search_by(|(candidate, _)| candidate.cmp(&path))
        {
            Ok(_) => {
                return Err(host::Error::DuplicateTypeParameter {
                    item: "<signature>".into(),
                    parameter: name,
                });
            }
            Err(index) => index,
        };
        self.type_parameters
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.declared
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.type_parameters.insert(index, (path, requirements));
        let index = u32::try_from(index).map_err(|_| host::Error::Allocation)?;
        for position in &mut self.declared {
            if *position >= index {
                *position += 1;
            }
        }
        self.declared.push(index);
        Ok(self)
    }

    /// Append one positional parameter type.
    pub fn parameter(&mut self, ty: host::type_ref::Type) -> &mut Self {
        self.parameters.push(ty);
        self
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_parameter(
        &mut self,
        ty: host::type_ref::Type,
    ) -> Result<&mut Self, host::Error> {
        self.parameters
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.parameters.push(ty);
        Ok(self)
    }

    /// Set the callable output type.
    pub fn output(&mut self, ty: host::type_ref::Type) -> &mut Self {
        self.output = Some(ty);
        self
    }

    /// Add what a bound ties between the type parameters.
    pub fn relation(&mut self, relation: Relation) -> Result<&mut Self, host::Error> {
        self.relations
            .try_reserve(1)
            .map_err(|_| host::Error::Allocation)?;
        self.relations.push(relation);
        Ok(self)
    }

    /// Finish the local signature proof.
    #[must_use]
    pub fn finish(self) -> Definition {
        Definition {
            type_parameters: self.type_parameters.into_boxed_slice(),
            declared: self.declared.into_boxed_slice(),
            parameters: self.parameters.into_boxed_slice(),
            output: self.output.unwrap_or_else(host::type_ref::Type::unit),
            relations: self.relations.into_boxed_slice(),
            crossing: self.crossing,
        }
    }
}

impl Definition {
    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let type_parameters = self
            .type_parameters
            .iter()
            .try_fold(0_usize, |bytes, (path, _)| {
                bytes.checked_add(path.retained_bytes()?)
            })?;
        let parameters = self
            .parameters
            .iter()
            .try_fold(0_usize, |bytes, ty| bytes.checked_add(ty.retained_bytes()?))?;
        let output = self
            .output
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::type_ref::Type>())?;
        std::mem::size_of::<Self>()
            .checked_add(type_parameters)?
            .checked_add(parameters)?
            .checked_add(output)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn fingerprint_retained_bytes(&self) -> Option<usize> {
        let type_parameters = self.type_parameters.iter().try_fold(
            self.type_parameters
                .len()
                .checked_mul(std::mem::size_of::<(ImmutableString, capability::Set)>())?,
            |bytes, (name, _)| bytes.checked_add(name.name().len()),
        )?;
        let parameters = self
            .parameters
            .iter()
            .try_fold(0_usize, |bytes, ty| bytes.checked_add(ty.retained_bytes()?))?;
        let output = self
            .output
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::type_ref::Type>())?;
        std::mem::size_of::<Fingerprint>()
            .checked_add(type_parameters)?
            .checked_add(parameters)?
            .checked_add(output)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        let mut type_parameters = Vec::new();
        type_parameters
            .try_reserve_exact(self.type_parameters.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for (path, requirements) in &self.type_parameters {
            type_parameters.push((path.try_clone()?, *requirements));
        }
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(self.parameters.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for parameter in &self.parameters {
            parameters.push(parameter.try_clone()?);
        }
        let mut declared = Vec::new();
        declared
            .try_reserve_exact(self.declared.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        declared.extend_from_slice(&self.declared);
        Ok(Self {
            type_parameters: type_parameters.into_boxed_slice(),
            declared: declared.into_boxed_slice(),
            parameters: parameters.into_boxed_slice(),
            output: self.output.try_clone()?,
            relations: self.relations.clone(),
            crossing: self.crossing,
        })
    }

    /// Whether a script function fills a callable parameter as a slot
    /// (D594).
    #[cfg(feature = "rust-import")]
    pub(crate) const fn crossing(&self) -> bool {
        self.crossing
    }

    /// What the item's bounds tie between its type parameters.
    pub(crate) fn relations(&self) -> &[Relation] {
        &self.relations
    }

    /// Return the positional parameter types.
    #[must_use]
    pub const fn parameters(&self) -> &[host::type_ref::Type] {
        &self.parameters
    }

    /// Return the output type.
    #[must_use]
    pub const fn output(&self) -> &host::type_ref::Type {
        &self.output
    }

    /// Whether a parameter is a callable, as a closure argument is.
    #[cfg(feature = "rust-import")]
    pub(crate) fn takes_callable(&self) -> bool {
        self.parameters
            .iter()
            .any(|parameter| matches!(parameter.shape(), host::type_ref::Shape::Callable { .. }))
    }

    /// Return the requirements attached to a declared type parameter.
    #[must_use]
    pub fn requirements(&self, parameter: &str) -> Option<capability::Set> {
        let parameter = host::path::Path::parse(parameter).ok()?;
        self.type_parameters
            .binary_search_by(|(candidate, _)| candidate.cmp(&parameter))
            .ok()
            .map(|index| self.type_parameters[index].1)
    }

    /// The type parameters' names in the order the item declares them,
    /// the order of a turbofish and of an imported instance's types.
    pub(crate) fn declared_type_parameters(&self) -> impl Iterator<Item = ImmutableString> + '_ {
        self.declared.iter().filter_map(|index| {
            self.type_parameters
                .get(*index as usize)
                .map(|(name, _)| name.to_string().into())
        })
    }

    pub(crate) fn type_parameters(
        &self,
    ) -> impl ExactSizeIterator<Item = (ImmutableString, capability::Set)> + '_ {
        self.type_parameters
            .iter()
            .map(|(name, requirements)| (name.to_string().into(), *requirements))
    }

    pub(crate) fn same_inputs(&self, other: &Self) -> Result<bool, host::Error> {
        if self.parameters.len() != other.parameters.len() {
            return Ok(false);
        }
        let mut mappings = Vec::new();
        for (left, right) in self.parameters.iter().zip(&other.parameters) {
            if !same_input_type(
                left,
                right,
                &self.type_parameters,
                &other.type_parameters,
                &mut mappings,
            )? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Produce this signature's deterministic exact structural proof.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            type_parameters: self
                .type_parameters
                .iter()
                .map(|(name, capabilities)| (name.to_string().into(), *capabilities))
                .collect(),
            parameters: self.parameters.clone(),
            output: self.output.clone(),
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_fingerprint(&self) -> Result<Fingerprint, rawr_import::surface::Error> {
        let mut type_parameters = Vec::new();
        type_parameters
            .try_reserve_exact(self.type_parameters.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for (name, capabilities) in &self.type_parameters {
            type_parameters.push((name.name_owned(), *capabilities));
        }
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(self.parameters.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        for parameter in &self.parameters {
            parameters.push(parameter.try_clone()?);
        }
        Ok(Fingerprint {
            type_parameters: type_parameters.into_boxed_slice(),
            parameters: parameters.into_boxed_slice(),
            output: self.output.try_clone()?,
        })
    }

    pub(crate) fn resolve(
        &self,
        root: &host::path::Path,
        types: &[host::type_def::Definition],
        supports: &[host::support::SignatureContract],
        imported: &host::type_ref::ImportedTypes,
        item: &host::path::Path,
    ) -> Result<Self, host::Error> {
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(self.parameters.len())
            .map_err(|_| host::Error::Allocation)?;
        for ty in &self.parameters {
            parameters.push(ty.resolve(
                root,
                types,
                supports,
                &self.type_parameters,
                imported,
                item,
            )?);
        }
        let output =
            self.output
                .resolve(root, types, supports, &self.type_parameters, imported, item)?;
        validate_signature_support(item, &parameters, &output)?;
        #[cfg(feature = "rust-import")]
        let type_parameters = {
            let mut parameters = Vec::new();
            parameters
                .try_reserve_exact(self.type_parameters.len())
                .map_err(|_| host::Error::Allocation)?;
            for (name, requirements) in &self.type_parameters {
                parameters.push((
                    name.try_clone().map_err(|_| host::Error::Allocation)?,
                    *requirements,
                ));
            }
            parameters.into_boxed_slice()
        };
        #[cfg(not(feature = "rust-import"))]
        let type_parameters = self.type_parameters.clone();
        Ok(Self {
            type_parameters,
            declared: self.declared.clone(),
            parameters: parameters.into_boxed_slice(),
            output,
            relations: self.relations.clone(),
            crossing: self.crossing,
        })
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn resolve_imported<A: host::ImportAllocation + ?Sized>(
        mut self,
        root: &host::path::Path,
        types: &[host::type_def::Definition],
        supports: &[host::support::SignatureContract],
        imported: &host::type_ref::ImportedTypes,
        item: &host::path::Path,
        allocation: &mut A,
    ) -> Result<Self, host::Error> {
        for ty in &mut self.parameters {
            ty.resolve_imported(
                root,
                types,
                supports,
                &self.type_parameters,
                imported,
                item,
                allocation,
            )?;
        }
        self.output.resolve_imported(
            root,
            types,
            supports,
            &self.type_parameters,
            imported,
            item,
            allocation,
        )?;
        validate_signature_support(item, &self.parameters, &self.output)?;
        Ok(self)
    }

    pub(crate) fn external_types(&self) -> Vec<&host::type_def::Export> {
        let mut exports = Vec::new();
        for parameter in &self.parameters {
            parameter.collect_exports(&mut exports);
        }
        self.output.collect_exports(&mut exports);
        exports
    }

    pub(crate) fn support_exports(&self) -> Vec<&host::support::SupportExport> {
        let mut exports = Vec::new();
        for parameter in &self.parameters {
            parameter.collect_support_exports(&mut exports);
        }
        self.output.collect_support_exports(&mut exports);
        exports
    }
}

fn validate_signature_support(
    item: &host::path::Path,
    parameters: &[host::type_ref::Type],
    output: &host::type_ref::Type,
) -> Result<(), host::Error> {
    for ty in parameters.iter().chain([output]) {
        if let Some(ty) = ty.nested_signature_support() {
            return Err(host::Error::InvalidSignatureSupport {
                item: item.to_string().into(),
                ty,
            });
        }
    }
    Ok(())
}

fn same_input_type(
    left: &host::type_ref::Type,
    right: &host::type_ref::Type,
    left_parameters: &[(host::path::Path, capability::Set)],
    right_parameters: &[(host::path::Path, capability::Set)],
    mappings: &mut Vec<(usize, usize)>,
) -> Result<bool, host::Error> {
    use host::type_ref::Shape;

    fn same_types(
        left: &[host::type_ref::Type],
        right: &[host::type_ref::Type],
        left_parameters: &[(host::path::Path, capability::Set)],
        right_parameters: &[(host::path::Path, capability::Set)],
        mappings: &mut Vec<(usize, usize)>,
    ) -> Result<bool, host::Error> {
        if left.len() != right.len() {
            return Ok(false);
        }
        for (left, right) in left.iter().zip(right) {
            if !same_input_type(left, right, left_parameters, right_parameters, mappings)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    Ok(match (left.shape(), right.shape()) {
        (Shape::Unit, Shape::Unit) => true,
        (Shape::Builtin(left), Shape::Builtin(right)) => left == right,
        (Shape::Tuple(left), Shape::Tuple(right)) => {
            same_types(left, right, left_parameters, right_parameters, mappings)?
        }
        (Shape::Option(left), Shape::Option(right))
        | (Shape::Iter(left), Shape::Iter(right))
        | (Shape::Range(left), Shape::Range(right)) => {
            same_input_type(left, right, left_parameters, right_parameters, mappings)?
        }
        (
            Shape::Reference {
                mutable: left_mutable,
                target: left,
            },
            Shape::Reference {
                mutable: right_mutable,
                target: right,
            },
        ) => {
            left_mutable == right_mutable
                && same_input_type(left, right, left_parameters, right_parameters, mappings)?
        }
        (Shape::Result(left_ok, left_err), Shape::Result(right_ok, right_err)) => {
            same_input_type(
                left_ok,
                right_ok,
                left_parameters,
                right_parameters,
                mappings,
            )? && same_input_type(
                left_err,
                right_err,
                left_parameters,
                right_parameters,
                mappings,
            )?
        }
        (
            Shape::Array {
                element: left,
                length: left_length,
            },
            Shape::Array {
                element: right,
                length: right_length,
            },
        ) => {
            left_length == right_length
                && same_input_type(left, right, left_parameters, right_parameters, mappings)?
        }
        (
            Shape::Callable {
                input: left_input,
                output: left_output,
            },
            Shape::Callable {
                input: right_input,
                output: right_output,
            },
        ) => {
            if !same_input_type(
                left_input,
                right_input,
                left_parameters,
                right_parameters,
                mappings,
            )? {
                false
            } else {
                same_input_type(
                    left_output,
                    right_output,
                    left_parameters,
                    right_parameters,
                    mappings,
                )?
            }
        }
        #[cfg(feature = "rust-import")]
        (
            Shape::ImportedFunctionItem {
                input: left_input,
                output: left_output,
            },
            Shape::ImportedFunctionItem {
                input: right_input,
                output: right_output,
            },
        ) => {
            if !same_input_type(
                left_input,
                right_input,
                left_parameters,
                right_parameters,
                mappings,
            )? {
                false
            } else {
                same_input_type(
                    left_output,
                    right_output,
                    left_parameters,
                    right_parameters,
                    mappings,
                )?
            }
        }
        (
            Shape::Registered {
                path: left_path,
                arguments: left_arguments,
            },
            Shape::Registered {
                path: right_path,
                arguments: right_arguments,
            },
        ) => {
            left_path == right_path
                && same_types(
                    left_arguments,
                    right_arguments,
                    left_parameters,
                    right_parameters,
                    mappings,
                )?
        }
        #[cfg(feature = "rust-import")]
        (
            Shape::Imported {
                identity: left_identity,
                arguments: left_arguments,
            },
            Shape::Imported {
                identity: right_identity,
                arguments: right_arguments,
            },
        ) => {
            left_identity == right_identity
                && same_types(
                    left_arguments,
                    right_arguments,
                    left_parameters,
                    right_parameters,
                    mappings,
                )?
        }
        (
            Shape::External {
                export: left_export,
                arguments: left_arguments,
            },
            Shape::External {
                export: right_export,
                arguments: right_arguments,
            },
        ) => {
            left_export == right_export
                && same_types(
                    left_arguments,
                    right_arguments,
                    left_parameters,
                    right_parameters,
                    mappings,
                )?
        }
        (Shape::Support(left), Shape::Support(right)) => left == right,
        (Shape::Parameter(left), Shape::Parameter(right)) => {
            let left_index = left_parameters
                .binary_search_by(|(candidate, _)| candidate.cmp(left))
                .ok();
            let right_index = right_parameters
                .binary_search_by(|(candidate, _)| candidate.cmp(right))
                .ok();
            let (Some(left_index), Some(right_index)) = (left_index, right_index) else {
                return Ok(false);
            };
            if left_parameters[left_index].1 != right_parameters[right_index].1 {
                return Ok(false);
            }
            if let Some((_, mapped)) = mappings.iter().find(|(left, _)| *left == left_index) {
                return Ok(*mapped == right_index);
            }
            if mappings.iter().any(|(_, right)| *right == right_index) {
                return Ok(false);
            }
            mappings
                .try_reserve(1)
                .map_err(|_| host::Error::Allocation)?;
            mappings.push((left_index, right_index));
            true
        }
        _ => false,
    })
}

impl Fingerprint {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        let mut completed = 0_usize;
        for parameter in &mut self.parameters {
            completed = completed.saturating_add(parameter.normalize_profile(profile));
        }
        completed.saturating_add(self.output.normalize_profile(profile))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let type_parameters = self.type_parameters.iter().try_fold(
            self.type_parameters
                .len()
                .checked_mul(std::mem::size_of::<(ImmutableString, capability::Set)>())?,
            |bytes, (name, _)| bytes.checked_add(name.len()),
        )?;
        let parameters = self
            .parameters
            .iter()
            .try_fold(0_usize, |bytes, ty| bytes.checked_add(ty.retained_bytes()?))?;
        let output = self
            .output
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::type_ref::Type>())?;
        std::mem::size_of::<Self>()
            .checked_add(type_parameters)?
            .checked_add(parameters)?
            .checked_add(output)
    }
}

impl crate::preview::digest::Write for Fingerprint {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        use crate::preview::digest::write_slice;

        writer.tag(0x2400);
        writer.tag(0x2401);
        writer.count(self.type_parameters.len());
        for (name, capabilities) in &self.type_parameters {
            writer.tag(0x2410);
            writer.tag(0x2411);
            writer.text(name);
            writer.tag(0x2412);
            capabilities.write(writer)?;
        }
        writer.tag(0x2402);
        write_slice(writer, &self.parameters)?;
        writer.tag(0x2403);
        self.output.write(writer)?;
        Ok(())
    }
}
