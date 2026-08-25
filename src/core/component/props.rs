//! Inherit properties of Components, mainly [`IComponent`]

use crate::core::{ComponentGetError, ComponentGetMutError, level::LevelIndex, world::world};

use super::*;

#[derive(Clone)]
/// The parent of a Component, either another Component or a
/// [`Level`](crate::core::level::Level).
pub enum ComponentParent {
    /// The Component has another Component as a parent
    Component(DynComponentId),
    /// The Component is top-level within the given
    /// [`Level`](crate::core::level::Level).
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
/// Avoid implementing this on your own. Use [`comp_def!`](crate::comp_def) to
/// generate the Component for you.
///
/// **Note:** Every Component "owns" its children. Removing a Component that is
/// not your direct child may result in panics. You are free to use
/// `.expect("...")` when accessing your own child.
///
/// # Lifecycle
///
/// Every Component implements [`spawn`](IComponent::spawn). `spawn` should be
/// *the* way all Components are created. `spawn` should get the parent's
/// [`Level`](crate::core::level::Level) from the
/// [`World`](crate::core::world::World), create the Component, and return the
/// new ID. During `spawn`, do not attempt to use an ancestor's child
/// Components, as they may not be created yet. After `spawn`, the root caller
/// should call [`post_init`](IComponent::post_init).
///
/// When a Component is removed from the tree, either by being replaced or
/// explicitly removed, the parent should call [`destroy`](IComponent::destroy),
/// then remove the Component from the `Level`.
///
/// # Other hooks
///
/// The main loop runs physics at a stable interval. On each simulation, every
/// `Level` calls [`pre_phys`](IComponent::pre_phys) on every top-level
/// Component. Each Component is to do its processing, **then** go to each child
/// and call `pre_phys` on them, too. After all `pre_phys` calls have run, each
/// `Level` processes its physics simulation. Finally, step 1 is repeated,
/// except with [`post_phys`](IComponent::post_phys). Unlike its `pre`
/// counterpart, Components should run their logic **after** their children.
///
/// [`idle`](IComponent::idle) has a similar processing order to `pre_phys`,
/// except `idle` runs before Components queue to be drawn. If the renderer is
/// disabled, `idle` may never run. `idle` is not suitable for driving game
/// logic.
///
/// # Safety
///
/// `parent_id` must return the ID passed through `spawn`.
pub unsafe trait IComponent: Any {
    /// Must return the ID of the parent Component or [`Level`](crate::core::level::Level)
    fn parent_id(&self) -> ComponentParent;

    /// Must return all child Components owned by this Component.
    fn children(&self) -> Box<dyn Iterator<Item = DynComponentId>>;

    /// Extra parameter for [`spawn`](IComponent::spawn).
    type SpawnInfo
    where
        Self: Sized;

    /// Create a new Component. Like [`Default`], but in context.
    ///
    /// You should not try to access any ancestors or children during this
    /// function. Save it for [`post_init`](IComponent::post_init).
    fn spawn(parent: ComponentParent, info: Self::SpawnInfo) -> ComponentId<Self>
    where
        Self: Sized;

    /// Called after a Component, all its children, and all its ancestors, have initialized.
    ///
    /// Self logic runs before child logic.
    fn post_init_hook(&mut self) {}

    /// Calls [`post_init_hook`](IComponent::post_init_hook) on all children in
    /// depth-first, parent-first order.
    ///
    /// # Borrows
    /// Mutably borrows all descendants of self, but only one at a time.
    fn post_init(&mut self) {
        self.post_init_hook();

        let mut stack: Vec<_> = self.children().collect();
        stack.reverse();

        while let Some(comp) = stack.pop() {
            let mut comp = match comp.get_mut() {
                Ok(comp) => comp,
                Err(ComponentGetMutError::NotFound) => continue,
                Err(ComponentGetMutError::BorrowMutError(e)) => {
                    panic!("post_init borrow error: {e}")
                }
            };

            comp.post_init_hook();

            let mut children: Vec<_> = comp.children().collect();
            children.reverse();
            stack.extend(children);
        }
    }

    /// Called before a Component is removed from its parent.
    ///
    /// Self logic runs before child logic.
    fn destroy_hook(&mut self) {}

    /// Calls [`destroy_hook`](IComponent::destroy_hook) on self and all
    /// descendants in depth-first, parent-first order, removing each
    /// descendant's slot from its [`Level`](crate::core::level::Level) after
    /// its `destroy_hook` runs.
    ///
    /// Does *not* remove self; the caller removes this Component from its
    /// parent or top-level list after calling this.
    ///
    /// # Borrows
    /// Mutably borrows all descendants of self, but only one at a time.
    fn destroy(&mut self) {
        self.destroy_hook();

        let mut stack: Vec<_> = self.children().collect();
        stack.reverse();

        while let Some(id) = stack.pop() {
            let mut children = {
                let mut comp = match id.get_mut() {
                    Ok(comp) => comp,
                    Err(ComponentGetMutError::NotFound) => continue,
                    Err(ComponentGetMutError::BorrowMutError(e)) => {
                        panic!("destroy borrow error: {e}")
                    }
                };

                comp.destroy_hook();

                let mut children: Vec<_> = comp.children().collect();
                children.reverse();
                children
            };

            let (pidx, gidx, tyid) = id.acquire_parts();

            if let Some(level) = world().get_level(id.level_id()) {
                level.remove_component_internal(tyid, pidx, gidx);
            }

            stack.append(&mut children);
        }
    }

    /// Runs before the physics simulation, on a stable interval.
    ///
    /// Self logic runs before child logic.
    fn pre_phys_hook(&mut self, _delta: f32) {}

    /// Calls [`pre_phys_hook`](IComponent::pre_phys_hook) on self and all
    /// descendants in depth-first, parent-first order.
    ///
    /// # Borrows
    /// Mutably borrows all descendants of self, but only one at a time.
    fn pre_phys(&mut self, delta: f32) {
        self.pre_phys_hook(delta);
        let mut stack: Vec<_> = self.children().collect();
        stack.reverse();

        while let Some(comp) = stack.pop() {
            let mut comp = match comp.get_mut() {
                Ok(comp) => comp,
                Err(ComponentGetMutError::NotFound) => continue,
                Err(ComponentGetMutError::BorrowMutError(e)) => {
                    panic!("pre_phys borrow error: {e}")
                }
            };

            comp.pre_phys_hook(delta);

            let mut children: Vec<_> = comp.children().collect();
            children.reverse();
            stack.extend(children);
        }
    }

    /// Runs after the physics simulation.
    ///
    /// Child logic runs before self logic.
    fn post_phys_hook(&mut self, _delta: f32) {}

    /// Calls [`post_phys_hook`](IComponent::post_phys_hook) on all children in
    /// depth-first, parent-*last* order.
    ///
    /// # Borrows
    /// Mutably borrows all descendants of self, but only one at a time.
    fn post_phys(&mut self, delta: f32) {
        self.post_phys_hook(delta);
        let mut stack: Vec<_> = self.children().map(|c| (c, false)).collect();
        stack.reverse();

        while let Some((comp, visited)) = stack.pop() {
            if visited {
                let mut comp = match comp.get_mut() {
                    Ok(comp) => comp,
                    Err(ComponentGetMutError::NotFound) => continue,
                    Err(ComponentGetMutError::BorrowMutError(e)) => {
                        panic!("post_phys borrow error: {e}")
                    }
                };

                comp.post_phys_hook(delta);
            } else {
                stack.push((comp.clone(), true));

                let comp = match comp.get() {
                    Ok(comp) => comp,
                    Err(ComponentGetError::NotFound) => continue,
                    Err(ComponentGetError::BorrowError(e)) => panic!("post_phys borrow error: {e}"),
                };

                let mut children: Vec<_> = comp.children().map(|c| (c, false)).collect();
                children.reverse();
                stack.extend(children);
            }
        }
    }

    /// Runs before Components submit to the draw queue.
    ///
    /// Self logic runs before child logic
    fn idle_hook(&mut self, _delta: f32) {}

    /// Calls [`idle_hook`](IComponent::idle_hook) on all children in
    /// depth-first, parent-first order.
    ///
    /// # Borrows
    /// Mutably borrows all descendants of self, but only one at a time.
    fn idle(&mut self, delta: f32) {
        self.idle_hook(delta);
        let mut stack: Vec<_> = self.children().collect();
        stack.reverse();

        while let Some(comp) = stack.pop() {
            let mut comp = match comp.get_mut() {
                Ok(comp) => comp,
                Err(ComponentGetMutError::NotFound) => continue,
                Err(ComponentGetMutError::BorrowMutError(e)) => {
                    panic!("idle borrow error: {e}")
                }
            };

            comp.idle_hook(delta);

            let mut children: Vec<_> = comp.children().collect();
            children.reverse();
            stack.extend(children);
        }
    }
}
