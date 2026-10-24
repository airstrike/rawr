//! Stable method signatures and generation-local resolved definitions.

use crate::resolve::{self, type_ref};
use crate::{item, program, receiver, signature, type_def, ImmutableString, Span};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// A generation-local resolved method identifier.
///
/// Stable method identity uses [`item::Key`]; this dense ID never enters a
/// persistent script value.
///
/// ```compile_fail
/// let _ = rawr::method::Id(0);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(usize);

impl Id {
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

/// One completely resolved inherent method in a compilation generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Definition {
    id: Id,
    key: item::Key,
    fingerprint: signature::Fingerprint,
    owner: type_def::Id,
    owner_identity: type_def::Identity,
    owner_schema: program::schema::Fingerprint,
    owner_name: ImmutableString,
    source: program::source::Id,
    public: bool,
    name: crate::item::Name,
    receiver: Option<receiver::Mode>,
    receiver_span: Option<Span>,
    /// `mut self`: the owned receiver binding is mutable.
    receiver_mutable: bool,
    parameters: Box<[resolve::item::Parameter]>,
    output: type_ref::Type,
    body: Span,
    /// `Some(T)` for `from` in `impl From<T> for Owner`.
    conversion: Option<type_ref::Type>,
    /// The trait of the `impl Trait for Owner` this function belongs to.
    trait_: Option<ImmutableString>,
    /// The trait's type arguments: `i64` of `impl Convert<i64> for Owner`.
    trait_arguments: Box<[type_ref::Type]>,
    /// This function is the trait's default body, which the impl receives.
    trait_default: bool,
    /// An associated `const NAME: T = value;`, used as `Type::NAME`.
    constant: bool,
    /// `async fn`: the body runs as an `async move` block.
    asynchronous: bool,
    /// The value of such a constant when it is one integer literal, as an
    /// array length (`[T; Self::N]`) names it.
    integer: Option<crate::INT>,
    /// The impl's generic parameters, then the method's own.
    generics: Box<[resolve::item::Generic]>,
    /// The owner's type arguments in the impl header: `T` of `impl<T> Pair<T>`.
    owner_arguments: Box<[type_ref::Type]>,
    /// The owner type of an impl for a builtin type (`i64`, `Vec<T>`);
    /// `owner` is then `type_def::Id::builtin`.
    builtin_owner: Option<type_ref::Type>,
    /// The text `body` spans when it is not its source's: the declaration
    /// of a facade trait (iced's `canvas::Program`) whose default body this is.
    body_text: Option<ImmutableString>,
}

#[allow(dead_code)]
impl Definition {
    pub(crate) fn new(
        id: Id,
        key: item::Key,
        owner: &type_def::Definition,
        function: &resolve::item::Function,
        source: program::source::Id,
    ) -> Self {
        let receiver = function.receiver.as_ref().map(|receiver| match receiver {
            crate::item::Receiver::Shared(_) => receiver::Mode::Shared,
            crate::item::Receiver::Mutable(_) => receiver::Mode::Mutable,
            crate::item::Receiver::Owned(_) | crate::item::Receiver::OwnedMutable(_) => {
                receiver::Mode::Owned
            }
        });
        let receiver_span = function.receiver.as_ref().map(crate::item::Receiver::span);
        let receiver_mutable = matches!(
            function.receiver,
            Some(crate::item::Receiver::OwnedMutable(_))
        );
        let output = function
            .output
            .clone()
            .unwrap_or(type_ref::Type::Unit(function.body.span));
        let owner_identity = type_def::Identity::Script(owner.key().clone());
        let fingerprint = signature::Fingerprint::function(
            true,
            receiver,
            &function.parameters,
            function.output.as_ref(),
        );

        Self {
            id,
            key,
            fingerprint,
            owner: owner.id(),
            owner_identity,
            owner_schema: owner.schema().clone(),
            owner_name: owner.name().into(),
            source,
            public: function.header.visibility.is_some(),
            name: function.name.clone(),
            receiver,
            receiver_span,
            receiver_mutable,
            parameters: function.parameters.clone(),
            output,
            body: function.body.span,
            conversion: None,
            trait_: None,
            trait_arguments: Box::default(),
            trait_default: false,
            constant: function.body.constant,
            asynchronous: function.body.asynchronous,
            integer: function
                .body
                .syntax
                .integer()
                .filter(|_| function.body.constant),
            generics: function.generics.clone(),
            owner_arguments: Box::default(),
            builtin_owner: None,
            body_text: None,
        }
    }

    /// This default body read from its facade trait's `declaration`.
    pub(crate) fn with_body_text(mut self, declaration: ImmutableString) -> Self {
        self.body_text = Some(declaration);
        self
    }

    /// The text `body` spans when it is not its source's.
    pub(crate) fn body_text(&self) -> Option<&str> {
        self.body_text.as_deref()
    }

    /// This function in an impl for the builtin type `owner`.
    pub(crate) fn for_builtin(mut self, owner: type_ref::Type) -> Self {
        self.builtin_owner = Some(owner);
        self
    }

    /// The owner type of an impl for a builtin type.
    pub(crate) const fn builtin_owner(&self) -> Option<&type_ref::Type> {
        self.builtin_owner.as_ref()
    }

    /// This method in `impl<generics> Owner<owner_arguments>`.
    pub(crate) fn in_generic_impl(
        mut self,
        impl_generics: &[resolve::item::Generic],
        owner_arguments: Box<[type_ref::Type]>,
    ) -> Self {
        let mut generics = impl_generics.to_vec();
        generics.extend(self.generics.iter().cloned());
        self.generics = generics.into_boxed_slice();
        self.owner_arguments = owner_arguments;
        self
    }

    /// The impl's generic parameters, then the method's own.
    pub(crate) fn generics(&self) -> &[resolve::item::Generic] {
        &self.generics
    }

    /// The owner's type arguments in the impl header.
    pub(crate) fn owner_arguments(&self) -> &[type_ref::Type] {
        &self.owner_arguments
    }

    /// This method as `from` of `impl From<source> for Owner`.
    pub(crate) fn with_conversion(mut self, source: type_ref::Type) -> Self {
        self.conversion = Some(source);
        self
    }

    /// This function as a member of `impl trait_ for Owner`.
    pub(crate) fn with_trait(mut self, trait_: ImmutableString) -> Self {
        self.trait_ = Some(trait_);
        self
    }

    /// The trait's type arguments in `impl Trait<arguments> for Owner`.
    pub(crate) fn with_trait_arguments(mut self, arguments: Box<[type_ref::Type]>) -> Self {
        self.trait_arguments = arguments;
        self
    }

    /// The type arguments of the trait this function's impl implements.
    pub(crate) fn trait_arguments(&self) -> &[type_ref::Type] {
        &self.trait_arguments
    }

    /// This function as the trait's default body for the impl's owner.
    pub(crate) fn as_trait_default(mut self) -> Self {
        self.trait_default = true;
        self
    }

    /// Whether this is an associated const, used without call parentheses.
    pub(crate) const fn constant(&self) -> bool {
        self.constant
    }

    pub(crate) const fn asynchronous(&self) -> bool {
        self.asynchronous
    }

    /// An integer-literal associated constant's value.
    pub(crate) const fn integer(&self) -> Option<crate::INT> {
        self.integer
    }

    /// The trait whose default body this function is, when it is one.
    pub(crate) fn trait_default(&self) -> Option<&ImmutableString> {
        self.trait_.as_ref().filter(|_| self.trait_default)
    }

    /// The trait whose impl this function belongs to.
    pub(crate) fn trait_name(&self) -> Option<&ImmutableString> {
        self.trait_.as_ref()
    }

    /// `T` of the `impl From<T>` this method is `from` of.
    pub(crate) const fn conversion(&self) -> Option<&type_ref::Type> {
        self.conversion.as_ref()
    }

    pub(crate) const fn id(&self) -> Id {
        self.id
    }

    pub(crate) fn key(&self) -> &item::Key {
        &self.key
    }

    pub(crate) fn fingerprint(&self) -> &signature::Fingerprint {
        &self.fingerprint
    }

    pub(crate) const fn owner(&self) -> type_def::Id {
        self.owner
    }

    pub(crate) fn owner_identity(&self) -> &type_def::Identity {
        &self.owner_identity
    }

    pub(crate) fn owner_schema(&self) -> &program::schema::Fingerprint {
        &self.owner_schema
    }

    pub(crate) fn owner_name(&self) -> &str {
        &self.owner_name
    }

    pub(crate) const fn source(&self) -> program::source::Id {
        self.source
    }

    pub(crate) const fn is_public(&self) -> bool {
        self.public
    }

    pub(crate) fn name(&self) -> &str {
        &self.name.text
    }

    pub(crate) const fn name_span(&self) -> Span {
        self.name.span
    }

    pub(crate) const fn receiver(&self) -> Option<receiver::Mode> {
        self.receiver
    }

    pub(crate) const fn receiver_span(&self) -> Option<Span> {
        self.receiver_span
    }

    pub(crate) const fn receiver_mutable(&self) -> bool {
        self.receiver_mutable
    }

    pub(crate) fn parameters(&self) -> &[resolve::item::Parameter] {
        &self.parameters
    }

    pub(crate) const fn output(&self) -> &type_ref::Type {
        &self.output
    }

    pub(crate) const fn body(&self) -> Span {
        self.body
    }

    pub(crate) fn symbol(&self) -> ImmutableString {
        format!("\0rawr.method.{}", self.key).into()
    }
}
