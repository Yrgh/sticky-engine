//! Scripts that can be attached to a [`Level`].
//!
//! A [`Level`] can host any number of [`IScript`]s of different types. You
//! cannot have multiple Scripts of the same type on the same `Level`.
//!
//! You can use Scripts for things like complex network logic, where the Script
//! does the processing and forwards the results to the appropriate Components.
//!
//! Scripts always receive `raw_input` hooks, in addition to the `IComponent`
//! hooks.

use std::any::Any;

use crate::core::{input::InputEvent, level::Level, world::World};

/// A Script that can be attached to a [`Level`].
///
/// See the [module-level docs](self) for more info.
pub trait IScript: Any {
    /// Called after the Script is added.
    fn post_init(&mut self, world: &World, level: &Level);

    /// Called before the Script is removed.
    fn destroy(&mut self, world: &World, level: &Level);

    /// Called before `idle` runs on Components.
    fn idle(&mut self, world: &World, level: &Level, delta: f32);

    /// Called before `pre_phys` runs on Components.
    fn pre_phys(&mut self, world: &World, level: &Level, delta: f32);

    /// Called before `post_phys` runs on Components.
    fn post_phys(&mut self, world: &World, level: &Level, delta: f32);

    /// Called before `raw_input` runs on
    /// [`SInputReceiver`](crate::core::input::SInputReceiver)s.
    fn raw_input(&mut self, world: &World, level: &Level, event: &InputEvent);
}

impl dyn IScript {
    /// Try to downcast `self` as `S`
    pub fn downcast_ref<S: IScript>(&self) -> Option<&S> {
        <dyn Any>::downcast_ref(self)
    }

    /// Try to downcast `self` as `S` mutably
    pub fn downcast_mut<S: IScript>(&mut self) -> Option<&mut S> {
        <dyn Any>::downcast_mut(self)
    }
}
