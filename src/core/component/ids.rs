//! Traits and structs for Component ID types.

use std::{
    any::TypeId,
    cell::{Ref, RefMut},
    marker::PhantomData,
};

use crate::core::{
    ComponentGetError, ComponentGetMutError,
    level::{Level, LevelIndex},
    relations::RELATIONS,
    world::world,
};

use super::*;

/// Base trait for all IDs to Components.
///
/// # Safety
///
/// Implementations of [`Hash`](std::hash::Hash) must match the following:
///
/// ```rust
/// fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
///     state.write_u32(self.pidx);
///     state.write_u32(self.gidx);
///     self.lidx.hash(state);
/// }
/// ```
///
/// [`PartialEq`] must compare `pidx`, `gidx`, and `lidx`. Comparing type IDs is not necessary.
pub unsafe trait ISlotId: Any + std::hash::Hash + PartialEq + Eq + Clone {
    /// The type or trait object this ID resolves to.
    type TraitObject: ISlotTr<Id = Self> + ?Sized;

    /// Construct a new ID from the parts, given the source type.
    ///
    /// # Safety
    ///
    /// The constructed ID must acquire the correct Component,
    /// targeting the same level, generation, and raw index. The type ID must
    /// match, too.
    unsafe fn from_parts(pidx: u32, gidx: u32, lidx: LevelIndex, tyid: TypeId) -> Self
    where
        Self: Sized;

    /// Returns the level ID.
    fn level_id(&self) -> LevelIndex;
    /// Returns the parts required to access a Component from a [`Level`]
    fn acquire_parts(&self) -> (u32, u32, TypeId);

    /// Try to access this ID immutably.
    ///
    /// Note: this can panic if the Component is already accessed mutably
    ///
    /// # Borrows
    ///
    /// Immutably borrows the referenced Component's slot until the returned
    /// [`Ref`] is dropped.
    fn get<'a>(&'a self) -> Result<Ref<'a, Self::TraitObject>, ComponentGetError>;

    /// Try to access this ID mutably.
    ///
    /// Note: this can panic if the Component is already accessed mutably or immutably
    ///
    /// # Borrows
    ///
    /// Mutably borrows the referenced Component's slot until the returned
    /// [`RefMut`] is dropped.
    fn get_mut<'a>(&'a self) -> Result<RefMut<'a, Self::TraitObject>, ComponentGetMutError>;

    /// Returns the [`Level`] of the Component this ID references.
    ///
    /// This is implemented automatically. Don't override it.
    fn get_level(&self) -> Option<&Level> {
        world().get_level(self.level_id())
    }

    /// Attempts to cast this ID to another ID.
    fn cast<D: ISlotId>(self) -> Result<D, Self> {
        let (pidx, gidx, tyid) = self.acquire_parts();
        if RELATIONS.implements(tyid, TypeId::of::<D::TraitObject>()) {
            let lidx = self.level_id();
            Ok(unsafe { D::from_parts(pidx, gidx, lidx, tyid) })
        } else {
            Err(self)
        }
    }
}

/// Base trait for all possible [`TraitObject`](ISlotId::TraitObject)s of a Component ID.
pub trait ISlotTr: Any + IComponent {
    /// The ID type this is the trait object/type of.
    type Id: ISlotId<TraitObject = Self>;
}

impl dyn IComponent {
    /// Try to cast this trait object to a concrete Component
    pub fn downcast_ref<C: IComponent>(&self) -> Option<&C> {
        <dyn Any>::downcast_ref(self)
    }

    /// Try to cast this trait object to a concrete Component
    pub fn downcast_mut<C: IComponent>(&mut self) -> Option<&mut C> {
        <dyn Any>::downcast_mut(self)
    }
}

impl ISlotTr for dyn IComponent {
    type Id = DynComponentId;
}

impl<T: IComponent> ToDyn<dyn IComponent> for T {
    fn to_dyn(&self) -> &dyn IComponent {
        self
    }

    fn to_dyn_mut(&mut self) -> &mut dyn IComponent {
        self
    }
}

/// A basic Component ID, referring to a specific type.
pub struct ComponentId<C: IComponent> {
    pidx: u32,
    gidx: u32,
    lidx: LevelIndex,
    _marker: PhantomData<C>,
}

impl<C: IComponent> Clone for ComponentId<C> {
    fn clone(&self) -> Self {
        Self {
            pidx: self.pidx,
            gidx: self.gidx,
            lidx: self.lidx,
            _marker: PhantomData,
        }
    }
}

impl<C: IComponent> std::hash::Hash for ComponentId<C> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.pidx);
        state.write_u32(self.gidx);
        self.lidx.hash(state);
    }
}

impl<C: IComponent> PartialEq for ComponentId<C> {
    fn eq(&self, other: &Self) -> bool {
        self.pidx == other.pidx && self.gidx == other.gidx && self.lidx == other.lidx
    }
}

impl<C: IComponent> Eq for ComponentId<C> {}

unsafe impl<C: IComponent> ISlotId for ComponentId<C> {
    type TraitObject = C;

    unsafe fn from_parts(pidx: u32, gidx: u32, lidx: LevelIndex, _: TypeId) -> Self
    where
        Self: Sized,
    {
        Self {
            pidx,
            gidx,
            lidx,
            _marker: PhantomData,
        }
    }

    fn level_id(&self) -> LevelIndex {
        self.lidx
    }

    fn acquire_parts(&self) -> (u32, u32, TypeId) {
        (self.pidx, self.gidx, TypeId::of::<C>())
    }

    fn get<'a>(&'a self) -> Result<Ref<'a, Self::TraitObject>, ComponentGetError> {
        world()
            .get_level(self.lidx)
            .ok_or(ComponentGetError::NotFound)?
            .acquire_component_internal(self.pidx, self.gidx)
    }

    fn get_mut<'a>(&'a self) -> Result<RefMut<'a, Self::TraitObject>, ComponentGetMutError> {
        world()
            .get_level(self.lidx)
            .ok_or(ComponentGetMutError::NotFound)?
            .acquire_component_internal_mut(self.pidx, self.gidx)
    }
}

impl<C: IComponent> ISlotTr for C {
    type Id = ComponentId<C>;
}

/// A Component ID with no knowledge about the Component.
pub struct DynComponentId {
    pidx: u32,
    gidx: u32,
    lidx: LevelIndex,
    tyid: TypeId,
}

impl<C: IComponent> From<ComponentId<C>> for DynComponentId {
    fn from(value: ComponentId<C>) -> Self {
        Self {
            pidx: value.pidx,
            gidx: value.gidx,
            lidx: value.lidx,
            tyid: TypeId::of::<C>(),
        }
    }
}

impl Clone for DynComponentId {
    fn clone(&self) -> Self {
        Self {
            pidx: self.pidx,
            gidx: self.gidx,
            lidx: self.lidx,
            tyid: self.tyid,
        }
    }
}

unsafe impl ISlotId for DynComponentId {
    type TraitObject = dyn IComponent;

    unsafe fn from_parts(pidx: u32, gidx: u32, lidx: LevelIndex, tyid: TypeId) -> Self
    where
        Self: Sized,
    {
        Self {
            pidx,
            gidx,
            lidx,
            tyid,
        }
    }

    fn level_id(&self) -> LevelIndex {
        self.lidx
    }

    fn acquire_parts(&self) -> (u32, u32, TypeId) {
        (self.pidx, self.gidx, self.tyid)
    }

    fn get<'a>(&'a self) -> Result<Ref<'a, dyn IComponent>, ComponentGetError> {
        world()
            .get_level(self.lidx)
            .ok_or(ComponentGetError::NotFound)?
            .acquire_component_internal_dyn(self.tyid, self.pidx, self.gidx)
    }

    fn get_mut<'a>(&'a self) -> Result<RefMut<'a, dyn IComponent>, ComponentGetMutError> {
        world()
            .get_level(self.lidx)
            .ok_or(ComponentGetMutError::NotFound)?
            .acquire_component_internal_dyn_mut(self.tyid, self.pidx, self.gidx)
    }
}

impl std::hash::Hash for DynComponentId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.pidx);
        state.write_u32(self.gidx);
        self.lidx.hash(state);
    }
}

impl<D: ISlotId> PartialEq<D> for DynComponentId {
    fn eq(&self, other: &D) -> bool {
        let (pidx, gidx, _) = other.acquire_parts();
        let lidx = other.level_id();
        self.pidx == pidx && self.gidx == gidx && self.lidx == lidx
    }
}

impl Eq for DynComponentId {}
