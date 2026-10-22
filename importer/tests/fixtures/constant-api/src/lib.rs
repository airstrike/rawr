#![feature(
    associated_type_defaults,
    inherent_associated_types,
    min_generic_const_args
)]
#![allow(incomplete_features)]

pub trait ProjectedLength {
    type const LENGTH: usize;
}

pub type const FREE_LENGTH: usize = 6;

pub trait DefaultedLength {
    type const REQUIRED: usize;
    type const DEFAULTED: usize = 5;
    type const FORWARDED: usize = Self::REQUIRED;
}

pub struct ConcreteLength;

impl ProjectedLength for ConcreteLength {
    type const LENGTH: usize = 3;
}

pub struct UnrelatedLength;

impl ProjectedLength for UnrelatedLength {
    type const LENGTH: usize = 9;
}

pub fn projected_array<T: ProjectedLength>(value: [u8; T::LENGTH]) -> [u8; T::LENGTH] {
    value
}

pub fn second_projected_array<X, Y: ProjectedLength>(
    value: [u8; Y::LENGTH],
) -> [u8; Y::LENGTH] {
    value
}

pub fn free_array(value: [u8; FREE_LENGTH]) -> [u8; FREE_LENGTH] {
    value
}

pub fn concrete_array(
    value: [u8; <ConcreteLength as ProjectedLength>::LENGTH],
) -> [u8; <ConcreteLength as ProjectedLength>::LENGTH] {
    value
}

pub struct InherentLength<T>(pub T);

impl<T> InherentLength<T> {
    pub type const LENGTH: usize = 4;
}

pub fn inherent_array<T>(
    value: [u8; InherentLength::<T>::LENGTH],
) -> [u8; InherentLength::<T>::LENGTH] {
    value
}
