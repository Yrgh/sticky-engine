//! Inherit properties of Components, mainly [`IComponent`]

use crate::engine::{world::World, level::LevelIndex};

use super::*;

#[derive(Clone)]
/// The parent of a Component, either another Component or a
/// [`Level`](crate::engine::level::Level).
pub enum ComponentParent {
    /// The Component has another Component as a parent
    Component(DynComponentId),
    /// The Component is top-level within the given
    /// [`Level`](crate::engine::level::Level).
    Level(LevelIndex),
}

impl ComponentParent {
    /// Returns the [`LevelIndex`] of a Component's parent.
    pub fn level_id(&self) -> LevelIndex {
        match self {
            ComponentParent::Component(dci) => dci.level_id(),
            ComponentParent::Level(lidx) => *lidx,
        }
    }
}

impl<T: Into<DynComponentId>> From<T> for ComponentParent {
    fn from(value: T) -> Self {
        ComponentParent::Component(value.into())
    }
}

impl From<LevelIndex> for ComponentParent {
    fn from(value: LevelIndex) -> Self {
        ComponentParent::Level(value)
    }
}

/// Base trait for all Components.
///
/// Avoid implementing this on your own. Use [`comp_def!`](macros::comp_def) to
/// generate the Component for you.
///
/// **Note:** Every Component "owns" its children. Removing a Component that is
/// not your direct child may result in panics. You are free to use
/// `.expect("...")` when accessing your own child.
///
/// # Lifecycle
///
/// Every Component implements [`spawn`](IComponent::spawn). `spawn`
/// should be *the* way all Components are created. `spawn` should get the
/// parent's [`Level`] from the [`World`], create the Component, and return the
/// new ID. During `spawn`, do not attempt to use an ancestor's child
/// Components, as they may not be created yet.
///
/// When a Component is removed from the tree, either by being replaced or
/// explicitly removed, the parent should call
/// [`destroy_hook`](IComponent::destroy_hook), then remove the Component from
/// the `Level`.
///
/// # Other hooks
///
/// The main loop runs physics at a stable interval. On each simulation, every `Level` calls
/// [`pre_phys_hook`](IComponent::pre_phys_hook) on every top-level Component. Each Component is to
/// do its processing, **then** go to each child and call `pre_phys_hook` on them, too. After all
/// `pre_phys_hook` calls have run, each `Level` processes its physics simulation. Finally, step 1
/// is repeated, except with [`post_phys_hook`](IComponent::post_phys_hook). Unlike its `pre` counterpart,
/// Components should run their logic **after** their children.
///
/// [`idle_hook`](IComponent::idle_hook) has a similar processing order to `pre_phys_hook`, except
/// `idle_hook` runs before Components queue to be drawn. If the renderer is disabled, `idle_hook`
/// may never run. `idle_hook` is not suitable for driving game logic.
/// 
/// # Safety
/// 
/// `parent_id` must return the ID passed through `spawn`.
pub unsafe trait IComponent: Any {
    /// Must return the ID of the parent Component or [`Level`](crate::engine::level::Level)
    fn parent_id(&self) -> ComponentParent;

    /// Must return all child Components owned by this Component.
    fn children(&self) -> Box<dyn Iterator<Item = DynComponentId>>;

    /// Create a new Component. Like [`Default`], but in context.
    ///
    /// You may have access to ancestor IDs, but accessing them can result in a
    /// panic or None result.
    fn spawn(world: &World, parent: ComponentParent) -> ComponentId<Self>
    where
        Self: Sized;

    /// Called before a Component is removed from its parent.
    ///
    /// You **must** remove your children when this is called.
    fn destroy_hook(&mut self, world: &World);

    /// Runs before the physics simulation, on a stable interval.
    ///
    /// You **must** run your logic, *then* run your children's logic.
    fn pre_phys_hook(&mut self, world: &World, delta: f32);
    /// Runs after the physics simulation.
    ///
    /// Your **must** run your children's logic, *then* your logic.
    fn post_phys_hook(&mut self, world: &World, delta: f32);

    /// Runs before Components submit to the draw queue.
    ///
    /// You **must** run your logic, *then* run your children's logic.
    fn idle_hook(&mut self, world: &World, delta: f32);
}
