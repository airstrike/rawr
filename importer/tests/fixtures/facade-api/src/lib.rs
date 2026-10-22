#[cfg(doc)]
compile_error!("D146 forbids documentation compilation in import authority");

mod facade {
    pub use support_api::{
        BlanketImpl, BorrowConvert, BUILD_MARKER, DeclaredBounds, ExternalImpl, Node, PrimitiveImpl,
        FieldSurface, ImplSurface, OpaqueFields, PrivateSurface, ReferenceImpl, SURFACE_CONST,
        SURFACE_STATIC, TraitSurface, VariantSurface,
    };
}

pub use facade::{
    BlanketImpl, BorrowConvert, BUILD_MARKER, DeclaredBounds, ExternalImpl, Node as RenamedNode,
    FieldSurface, ImplSurface, OpaqueFields, PrimitiveImpl, PrivateSurface, ReferenceImpl,
    SURFACE_CONST, SURFACE_STATIC, TraitSurface, VariantSurface,
};

pub use std::primitive::u32 as PrimitiveAlias;

pub type ExternalAlias = support_api::AliasTarget;

#[cfg(doc)]
pub fn doc_only() -> u8 {
    1
}

#[cfg(not(doc))]
pub fn normal_only() -> u8 {
    2
}

#[macro_export]
macro_rules! facade_macro {
    () => { 1_u8 };
}

#[derive(Clone)]
struct PrivateReturn(u8);

pub fn private_return() -> PrivateReturn {
    PrivateReturn(1)
}

pub fn opaque_private() -> impl Clone + use<> {
    PrivateReturn(2)
}

pub fn capture<T: Clone>(value: T) -> impl Clone + use<T> {
    value
}

pub fn boxed_iterator() -> Box<dyn Iterator<Item = u8>> {
    Box::new([1_u8].into_iter())
}

pub fn const_bound<const N: usize>(value: [u8; N]) -> [u8; N] {
    value
}

pub fn bound_only<T: support_api::BoundOnly>(value: T) -> T {
    value
}

pub fn primitive_impl_relevance(value: u32) -> u32
where
    u32: support_api::PrimitiveImpl,
{
    value
}

pub fn blanket_impl_relevance(value: u8) -> u8
where
    u8: support_api::BlanketImpl,
{
    value
}

pub fn reference_impl_relevance<'a, T: ?Sized>(value: &'a T) -> &'a T
where
    &'a T: support_api::ReferenceImpl,
{
    value
}

pub fn external_impl_relevance<T>(value: Option<T>) -> Option<T>
where
    Option<T>: support_api::ExternalImpl,
{
    value
}

pub fn repeated<T: Clone>(value: T) -> T
where
    T: Clone,
{
    value
}

pub fn node<'a, T>(value: T) -> support_api::Node<T>
where
    T: Clone + 'a,
{
    support_api::Node { value, next: None }
}

pub fn map_node<T, F>(node: RenamedNode<T>, map: F) -> RenamedNode<T>
where
    T: Clone,
    F: for<'a> Fn(&'a T) -> T,
{
    RenamedNode {
        value: map(&node.value),
        next: None,
    }
}

pub trait Projected<'a, T, const N: usize> {
    type Item;
}

struct ProjectionCapture<'a, T, const N: usize>(&'a [T; N]);

impl<'a, T, const N: usize> Projected<'a, T, N> for ProjectionCapture<'a, T, N> {
    type Item = &'a [T; N];
}

pub fn opaque_projection<'a, T: 'a, const N: usize>(
    value: &'a [T; N],
) -> impl Projected<'a, T, N, Item = &'a [T; N]> + use<'a, T, N> {
    ProjectionCapture(value)
}

pub fn sibling_opaque_projection<'a, T: 'a, const N: usize>(
    value: &'a [T; N],
) -> impl Projected<'a, T, N, Item = &'a [T; N]> + use<'a, T, N> {
    ProjectionCapture(value)
}

pub fn opaque_higher_ranked<F>(value: F) -> impl for<'a> Fn(&'a u8) -> &'a u8 + Clone + use<F>
where
    F: for<'a> Fn(&'a u8) -> &'a u8 + Clone,
{
    value
}

pub trait RpititProjection {
    fn rpitit_items<'a>(&'a self) -> impl Iterator<Item = &'a u8> + Clone;

    fn rpitit_pair(&self) -> (impl Clone, impl Clone);
}

pub struct RpititSurface(pub [u8; 2]);

impl RpititProjection for RpititSurface {
    fn rpitit_items<'a>(&'a self) -> impl Iterator<Item = &'a u8> + Clone {
        self.0.iter()
    }

    fn rpitit_pair(&self) -> (impl Clone, impl Clone) {
        (self.0[0], self.0[1])
    }
}

pub fn opaque_outlives<'a, T: 'a>(value: &'a T) -> impl Clone + 'a + use<'a, T> {
    value
}

pub fn sibling_opaque_outlives<'a, T: 'a>(value: &'a T) -> impl Clone + 'a + use<'a, T> {
    value
}

pub fn opaque_static_outlives() -> impl Clone + 'static {
    ()
}

pub fn opaque_without_outlives<T: Clone>(value: T) -> impl Clone + use<T> {
    value
}

pub fn opaque_hrtb_outlives<'a, F>(
    value: F,
) -> impl for<'b> Fn(&'b u8) -> &'b u8 + Clone + 'a + use<'a, F>
where
    F: for<'b> Fn(&'b u8) -> &'b u8 + Clone + 'a,
{
    value
}

pub trait DemandRelevant {
    type Output;

    fn output(self) -> Self::Output;
}

pub struct RelevantWrapper<T>(pub T);

struct UnrelatedWrapper<T>(T);

impl<T> DemandRelevant for RelevantWrapper<T> {
    type Output = T;

    fn output(self) -> Self::Output {
        self.0
    }
}

impl<T> DemandRelevant for UnrelatedWrapper<T> {
    type Output = T;

    fn output(self) -> Self::Output {
        self.0
    }
}

pub fn relevant_projection<T>(value: RelevantWrapper<T>) -> T
where
    RelevantWrapper<T>: DemandRelevant<Output = T>,
{
    value.output()
}

pub trait ImplOwnerBound {
    type Evidence;
}

pub struct ImplOwnerSurface;

struct PrivateImplOwner;

impl ImplOwnerBound for ImplOwnerSurface {
    type Evidence = u8;
}

impl ImplOwnerBound for PrivateImplOwner {
    type Evidence = u16;
}

pub trait ImplOwnerReachability {
    fn evidence(self) -> u8;
}

impl ImplOwnerReachability for ImplOwnerSurface
where
    ImplOwnerSurface: ImplOwnerBound<Evidence = u8>,
{
    fn evidence(self) -> u8 {
        1
    }
}

pub fn impl_owner_relevance(value: ImplOwnerSurface) -> u8
where
    ImplOwnerSurface: ImplOwnerReachability,
{
    value.evidence()
}

pub trait VtableBase {
    fn base(&self) -> u8;
}

pub trait VtableContract: VtableBase {
    fn required(&self) -> u8;

    fn defaulted(&self) -> u8 {
        0
    }
}

pub fn vtable_contract(value: &dyn VtableContract) -> &dyn VtableContract {
    value
}

pub const NAMED_ARRAY_LENGTH: usize = 7;

pub struct ConstArraySurface;

impl ConstArraySurface {
    pub const fn four(self) -> [u8; 32 / 8] {
        [0; 32 / 8]
    }

    pub const fn eight(self) -> [u8; 64 / 8] {
        [0; 64 / 8]
    }

    pub fn body_only(self) -> u8 {
        let _body_only = [0_u8; 96 / 8];
        0
    }
}

pub fn named_array() -> [u8; NAMED_ARRAY_LENGTH] {
    [0; NAMED_ARRAY_LENGTH]
}
