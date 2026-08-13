//! Component definitions and IDs

use std::any::Any;

pub use ids::*;
pub use props::*;

pub mod ids;
pub mod props;

/// Conversions between references. In practice, for converting types to trait objects
pub trait ToDyn<D: Any + ?Sized>: Any {
    /// Convert `Self` to `D` via shared reference.
    fn to_dyn(&self) -> &D;
    /// Convert `Self` to `D` via mutable reference.
    fn to_dyn_mut(&mut self) -> &mut D;
}

/// Hacky but necessary for ComponentId<T> to work with ISlotId
impl<T: Any + ?Sized> ToDyn<T> for T {
    fn to_dyn(&self) -> &T {
        self
    }

    fn to_dyn_mut(&mut self) -> &mut T {
        self
    }
}

mod private {
    pub trait Sealed {}
}

#[diagnostic::on_unimplemented(
    message = "static child Components must have a single type (use ComponentId<...>)",
    label = "This field has an invalid type",
    note = "Use dyn if you wish to have a dynamic type"
)]
/// A Component that can be marked as `static`, e.g. [`ComponentId`].
pub trait StaticValid: private::Sealed {
    /// The `C` of a [`ComponentId<C>`].
    type Component: IComponent;
}

impl<C: IComponent> private::Sealed for ComponentId<C> {}
impl<C: IComponent> StaticValid for ComponentId<C> {
    type Component = C;
}
