//! Traits and structs for Component ID types.

use std::{
    any::TypeId,
    cell::{Ref, RefMut},
    marker::PhantomData,
};

use crate::core::{
    ComponentGetError, ComponentGetMutError,
    level::{Level, LevelId},
    relations::RELATIONS,
    util::gen_slot_vec::SlotIndex,
    world::World,
};

use super::*;

/// Base trait for all IDs to Components.
///
/// # Safety
///
/// Implementations of [`Hash`](std::hash::Hash) must match the following:
///
/// ```rust,ignore
/// fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
///     self.slot.hash(state);
///     self.lidx.hash(state);
/// }
/// ```
///
/// [`PartialEq`] must compare `slot` and `lidx`. Comparing type IDs is not necessary.
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
    unsafe fn from_parts(slot: SlotIndex, lidx: LevelId, tyid: TypeId) -> Self
    where
        Self: Sized;

    /// Returns the level ID.
    fn level_id(&self) -> LevelId;
    /// Returns the parts required to access a Component from a [`Level`]
    fn acquire_parts(&self) -> (SlotIndex, TypeId);

    /// Try to access this ID immutably.
    ///
    /// Note: this can panic if the Component is already accessed mutably
    ///
    /// # Borrows
    ///
    /// Immutably borrows the referenced Component's slot until the returned
    /// [`Ref`] is dropped.
    fn get<'w>(&self, world: &'w World) -> Result<Ref<'w, Self::TraitObject>, ComponentGetError>;

    /// Try to access this ID mutably.
    ///
    /// Note: this can panic if the Component is already accessed mutably or immutably
    ///
    /// # Borrows
    ///
    /// Mutably borrows the referenced Component's slot until the returned
    /// [`RefMut`] is dropped.
    fn get_mut<'w>(
        &self,
        world: &'w World,
    ) -> Result<RefMut<'w, Self::TraitObject>, ComponentGetMutError>;

    /// Returns the [`Level`] of the Component this ID references.
    ///
    /// This is implemented automatically. Don't override it.
    fn get_level<'w>(&self, world: &'w World) -> Option<&'w Level> {
        world.get_level(self.level_id())
    }

    /// Attempts to cast this ID to another ID.
    fn cast<D: ISlotId>(self) -> Result<D, Self> {
        let (slot, tyid) = self.acquire_parts();
        if RELATIONS.implements(tyid, TypeId::of::<D::TraitObject>()) {
            let lidx = self.level_id();
            Ok(unsafe { D::from_parts(slot, lidx, tyid) })
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
    slot: SlotIndex,
    lidx: LevelId,
    _marker: PhantomData<C>,
}

impl<C: IComponent> Clone for ComponentId<C> {
    fn clone(&self) -> Self {
        Self {
            slot: self.slot,
            lidx: self.lidx,
            _marker: PhantomData,
        }
    }
}

impl<C: IComponent> std::fmt::Debug for ComponentId<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ComponentId(l/slot = {:?}/{:?}, type: {:?})",
            self.lidx,
            self.slot,
            TypeId::of::<C>()
        )
    }
}

impl<C: IComponent> std::hash::Hash for ComponentId<C> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.slot.hash(state);
        self.lidx.hash(state);
    }
}

impl<C: IComponent> PartialEq for ComponentId<C> {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot && self.lidx == other.lidx
    }
}

impl<C: IComponent> Eq for ComponentId<C> {}

unsafe impl<C: IComponent> ISlotId for ComponentId<C> {
    type TraitObject = C;

    unsafe fn from_parts(slot: SlotIndex, lidx: LevelId, _: TypeId) -> Self
    where
        Self: Sized,
    {
        Self {
            slot,
            lidx,
            _marker: PhantomData,
        }
    }

    fn level_id(&self) -> LevelId {
        self.lidx
    }

    fn acquire_parts(&self) -> (SlotIndex, TypeId) {
        (self.slot, TypeId::of::<C>())
    }

    fn get<'w>(&self, world: &'w World) -> Result<Ref<'w, Self::TraitObject>, ComponentGetError> {
        world
            .get_level(self.lidx)
            .ok_or(ComponentGetError::NotFound)?
            .acquire_component_internal(self.slot)
    }

    fn get_mut<'w>(
        &self,
        world: &'w World,
    ) -> Result<RefMut<'w, Self::TraitObject>, ComponentGetMutError> {
        world
            .get_level(self.lidx)
            .ok_or(ComponentGetMutError::NotFound)?
            .acquire_component_internal_mut(self.slot)
    }
}

impl<C: IComponent> ISlotTr for C {
    type Id = ComponentId<C>;
}

/// A Component ID with no knowledge about the Component.
pub struct DynComponentId {
    slot: SlotIndex,
    lidx: LevelId,
    tyid: TypeId,
}

impl<C: IComponent> From<ComponentId<C>> for DynComponentId {
    fn from(value: ComponentId<C>) -> Self {
        Self {
            slot: value.slot,
            lidx: value.lidx,
            tyid: TypeId::of::<C>(),
        }
    }
}

impl Clone for DynComponentId {
    fn clone(&self) -> Self {
        Self {
            slot: self.slot,
            lidx: self.lidx,
            tyid: self.tyid,
        }
    }
}

impl std::fmt::Debug for DynComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DynComponentId(l/slot = {:?}/{:?}, type: {:?})",
            self.lidx, self.slot, self.tyid
        )
    }
}

unsafe impl ISlotId for DynComponentId {
    type TraitObject = dyn IComponent;

    unsafe fn from_parts(slot: SlotIndex, lidx: LevelId, tyid: TypeId) -> Self
    where
        Self: Sized,
    {
        Self {
            slot,
            lidx,
            tyid,
        }
    }

    fn level_id(&self) -> LevelId {
        self.lidx
    }

    fn acquire_parts(&self) -> (SlotIndex, TypeId) {
        (self.slot, self.tyid)
    }

    fn get<'w>(&self, world: &'w World) -> Result<Ref<'w, dyn IComponent>, ComponentGetError> {
        world
            .get_level(self.lidx)
            .ok_or(ComponentGetError::NotFound)?
            .acquire_component_internal_dyn(self.tyid, self.slot)
    }

    fn get_mut<'w>(
        &self,
        world: &'w World,
    ) -> Result<RefMut<'w, dyn IComponent>, ComponentGetMutError> {
        world
            .get_level(self.lidx)
            .ok_or(ComponentGetMutError::NotFound)?
            .acquire_component_internal_dyn_mut(self.tyid, self.slot)
    }
}

impl std::hash::Hash for DynComponentId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.slot.hash(state);
        self.lidx.hash(state);
    }
}

impl<D: ISlotId> PartialEq<D> for DynComponentId {
    fn eq(&self, other: &D) -> bool {
        let (slot, _) = other.acquire_parts();
        let lidx = other.level_id();
        self.slot == slot && self.lidx == lidx
    }
}

impl Eq for DynComponentId {}
