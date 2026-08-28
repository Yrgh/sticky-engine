//! Scripts that can be attached to a [`Level`].
//!
//! A [`Level`] can host any number of Scripts. Scripts intercept events like
//! `idle` before they go to the Components. Additionally, each Script can
//! provide any number of additional top-level Components.

use std::{any::Any, fmt::Debug, hash::Hash};

use crate::core::{
    component::{ComponentId, DynComponentId, IComponent, ISlotId},
    level::{Level, LevelIndex},
    util::gen_slot_vec::SlotIndex,
    world::World,
};

/// A Script that can be attached to a [`Level`].
///
/// See the [module-level docs](self) for more info.
pub trait IScript: Any {
    /// Called after the Script is added.
    fn post_init(&mut self, world: &World, level: &Level);

    /// Called before the Script is removed.
    ///
    /// You **must** [`destroy`](crate::core::component::IComponent::destroy)
    /// and remove all Components you have previously created.
    fn destroy(&mut self, world: &World, level: &Level);

    /// Should return the top-level Components this Script owns.
    ///
    /// Note: You can return an empty slice if you want to "pause" processing
    /// for your Components. However, the physics engine does not do a tree
    /// walk, and those children will still process.
    ///
    /// See [`ToggleLevel`].
    fn top_level(&self) -> &[DynComponentId];

    /// Called before `idle` runs on Components.
    ///
    /// The event will be called automatically on Components. Do not send this
    /// event downward *unless* [`top_level`](IScript::top_level) returns `[]`.
    fn idle(&mut self, world: &World, level: &Level, delta: f32);

    /// Called before `pre_phys` runs on Components.
    ///
    /// The event will be called automatically on Components. Do not send this
    /// event downward *unless* [`top_level`](IScript::top_level) returns `[]`.
    fn pre_phys(&mut self, world: &World, level: &Level, delta: f32);

    /// Called before `post_phys` runs on Components.
    ///
    /// The event will be called automatically on Components. Do not send this
    /// event downward *unless* [`top_level`](IScript::top_level) returns `[]`.
    fn post_phys(&mut self, world: &World, level: &Level, delta: f32);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// An index to a Script of any type.
///
/// Unlike [`Level`]s and windows, there is no owning variant. You can remove a
/// Script with any ID.
pub struct DynScriptId {
    pub(super) slot: SlotIndex,
}

/// An index to a Script of a specific type.
///
/// See [`DynScriptId`].
pub struct ScriptId<T: IScript> {
    pub(super) slot: SlotIndex,
    pub(super) _marker: std::marker::PhantomData<T>,
}

impl<T: IScript> Clone for ScriptId<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: IScript> Copy for ScriptId<T> {}

impl<T: IScript> Debug for ScriptId<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptId")
            .field("slot", &self.slot)
            .finish()
    }
}

impl<T: IScript> PartialEq for ScriptId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot
    }
}

impl<T: IScript> PartialEq<DynScriptId> for ScriptId<T> {
    fn eq(&self, other: &DynScriptId) -> bool {
        self.slot == other.slot
    }
}

impl<T: IScript> PartialEq<ScriptId<T>> for DynScriptId {
    fn eq(&self, other: &ScriptId<T>) -> bool {
        self.slot == other.slot
    }
}

impl<T: IScript> Eq for ScriptId<T> {}

impl<T: IScript> Hash for ScriptId<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.slot.hash(state)
    }
}

impl Hash for DynScriptId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.slot.hash(state)
    }
}

impl<T: IScript> From<ScriptId<T>> for DynScriptId {
    fn from(value: ScriptId<T>) -> Self {
        Self { slot: value.slot }
    }
}

/// A basic Script that allows its top level to be turned on and off, including
/// physics.
pub struct ToggleLevel {
    level: LevelIndex,
    top_level: Vec<DynComponentId>,
    is_enabled: bool,
}

impl ToggleLevel {
    /// Creates an empty `ToggleLevel`, already enabled.
    pub fn new() -> Self {
        Self {
            level: LevelIndex(u32::MAX, u32::MAX),
            top_level: Vec::new(),
            is_enabled: true,
        }
    }

    /// Sets whether the top-level Components are enabled.
    ///
    /// See [`Self::is_enabled`].
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.is_enabled != enabled {
            self.is_enabled = enabled;

            // TODO: enable/disable physics by doing a tree walk.
        }
    }

    /// Returns whether the top-level Components are enabled.
    ///
    /// If this returns `false`, no Components owned by this `ToggleLevel` will
    /// receive events like `idle` and `post_phys`. Additionally, no Components
    /// will be rendered or participate in physics.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    /// Spawn a new Component at the end of the top level list.
    pub fn spawn_top_level<C: IComponent>(
        &mut self,
        world: &World,
        info: C::SpawnInfo,
    ) -> ComponentId<C> {
        let id = C::spawn(world, self.level.into(), info);
        self.top_level.push(id.clone().into());
        id.get_mut(world)
            .expect("component was just added")
            .post_init(world);
        id
    }

    /// Removes a Component from the top level list *by ID*.
    ///
    /// # Borrows
    ///
    /// Mutably borrows the removed Component's slot, and the removed Component
    /// plus all of its descendants (via queue-based traversal) through
    /// [`destroy`](IComponent::destroy).
    pub fn remove_top_level(&mut self, world: &World, id: &DynComponentId) -> bool {
        let Some(position) = self.top_level.iter().position(|id2| id2 == id) else {
            return false;
        };

        self.top_level.remove(position);

        id.get_mut(world).expect("id is in world").destroy(world);
        let (slot, tyid) = id.acquire_parts();
        world
            .get_level(self.level)
            .expect("")
            .remove_component_internal(tyid, slot);

        true
    }
}

impl Default for ToggleLevel {
    fn default() -> Self {
        Self::new()
    }
}
