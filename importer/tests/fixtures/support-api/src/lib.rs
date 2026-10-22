use compiler_helper::identity;

#[identity]
pub struct Node<T: Clone> {
    pub value: T,
    pub next: Option<Box<Node<T>>>,
}

pub trait BorrowConvert<'a, T>
where
    T: Clone + 'a,
{
    type Output: Clone;
    const DEFAULT: usize;

    fn convert(&'a self, input: &'a T) -> Self::Output;
}

pub struct DeclaredBounds<'a: 'b, 'b, T: Clone + ?Sized, U>
where
    U: Copy,
{
    pub value: &'a T,
    pub marker: std::marker::PhantomData<&'b U>,
}

pub trait PrimitiveImpl {
    type Associated;
    const MARKER: usize;

    fn increment(self) -> Self::Associated;
}

impl PrimitiveImpl for u32 {
    type Associated = u64;
    const MARKER: usize = 1;

    fn increment(self) -> Self::Associated {
        u64::from(self) + 1
    }
}

pub trait BlanketImpl {
    type Associated;
    const MARKER: usize;

    fn blanket(&self) -> Self::Associated;
}

impl<T: Clone> BlanketImpl for T {
    type Associated = T;
    const MARKER: usize = 2;

    fn blanket(&self) -> Self::Associated {
        self.clone()
    }
}

pub trait ReferenceImpl {
    type Associated;
    const MARKER: usize;

    fn width(self) -> Self::Associated;
}

impl<T: ?Sized> ReferenceImpl for &T {
    type Associated = usize;
    const MARKER: usize = 3;

    fn width(self) -> Self::Associated {
        std::mem::size_of_val(self)
    }
}

pub trait ExternalImpl {
    type Associated;
    const MARKER: usize;

    fn present(self) -> Self::Associated;
}

pub trait BoundOnly {
    fn marker(&self) -> u8;
}

pub struct AliasTarget(pub u8);

impl AliasTarget {
    pub fn inherent_value(&self) -> u8 {
        self.0
    }
}

pub trait AliasTrait {
    fn trait_value(&self) -> u8;
}

impl AliasTrait for AliasTarget {
    fn trait_value(&self) -> u8 {
        self.0
    }
}

pub struct FieldLeaf;

impl FieldLeaf {
    pub fn field_leaf(&self) -> u8 {
        1
    }
}

pub struct FieldSurface {
    pub leaf: FieldLeaf,
}

pub struct VariantLeaf;

impl VariantLeaf {
    pub fn variant_leaf(&self) -> u8 {
        2
    }
}

pub enum VariantSurface {
    Leaf(VariantLeaf),
}

pub struct ConstLeaf;

impl ConstLeaf {
    pub fn const_leaf(&self) -> u8 {
        3
    }
}

pub const SURFACE_CONST: Option<ConstLeaf> = None;

pub struct StaticLeaf;

impl StaticLeaf {
    pub fn static_leaf(&self) -> u8 {
        4
    }
}

pub static SURFACE_STATIC: Option<StaticLeaf> = None;

pub struct TraitLeaf;

impl TraitLeaf {
    pub fn trait_leaf(&self) -> u8 {
        5
    }
}

pub trait TraitSurface {
    const LEAF: Option<TraitLeaf> = None;

    fn leaf(&self) -> TraitLeaf;
}

pub struct ImplLeaf;

impl ImplLeaf {
    pub fn impl_leaf(&self) -> u8 {
        6
    }
}

pub struct ImplConstLeaf;

impl ImplConstLeaf {
    pub fn impl_const_leaf(&self) -> u8 {
        7
    }
}

pub struct ImplSurface;

impl ImplSurface {
    pub const LEAF: Option<ImplConstLeaf> = None;

    pub fn leaf(&self) -> ImplLeaf {
        ImplLeaf
    }

    fn hidden_impl_detail(&self) -> u8 {
        8
    }
}

struct PrivateSurfaceLeaf;

impl PrivateSurfaceLeaf {
    pub fn private_leaf(&self) -> u8 {
        8
    }
}

pub struct PrivateSurface {
    hidden: PrivateSurfaceLeaf,
}

impl BoundOnly for u8 {
    fn marker(&self) -> u8 {
        *self
    }
}

#[derive(Clone)]
struct PrivateField(u8);

pub struct OpaqueFields {
    pub visible: u8,
    hidden: PrivateField,
}

impl OpaqueFields {
    pub fn new(visible: u8) -> Self {
        Self {
            visible,
            hidden: PrivateField(visible),
        }
    }
}

impl<T> ExternalImpl for Option<T> {
    type Associated = bool;
    const MARKER: usize = 4;

    fn present(self) -> Self::Associated {
        self.is_some()
    }
}

impl external_api::ForeignTrait for Box<Node<u8>> {
    type Associated = u8;
    const MARKER: usize = 5;

    fn foreign(&self) -> Self::Associated {
        self.value
    }
}

pub static BUILD_MARKER: &str = env!("SUPPORT_BUILD_MARKER");

mod private {
    pub trait HiddenTrait {
        type Associated;
        const MARKER: usize;

        fn hidden(self) -> Self::Associated;
    }

    impl HiddenTrait for u16 {
        type Associated = u16;
        const MARKER: usize = 6;

        fn hidden(self) -> Self::Associated {
            self
        }
    }

    pub struct Unrelated;

    pub fn unrelated() -> Unrelated {
        Unrelated
    }
}
