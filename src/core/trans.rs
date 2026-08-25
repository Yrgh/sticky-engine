//! Transform utilities

use std::{cell::Cell, collections::VecDeque};

use crate::{
    core::{
        component::{ComponentParent, ISlotId},
        math,
        world::World,
    },
    slot_def,
};

/// Transform of a 3D object
pub type Trans3 = math::Pose3;

#[slot_def]
/// Core Slot for Components with a 3D transform
pub trait STrans3 {
    // Required

    /// Returns an immutable reference to the [**provider**](ITrans3Provider)
    /// for this Component.
    fn get_provider(&self) -> &dyn ITrans3Provider;
    /// Returns a mutable reference to the [**provider**](ITrans3Provider) for
    /// this Component.
    fn get_provider_mut(&mut self) -> &mut dyn ITrans3Provider;

    // Provided

    /// Returns the transform relative to the owning
    /// [`Level`](crate::core::level::Level).
    ///
    /// # Borrows
    ///
    /// Can recursively borrow ancestors implementing [`STrans3`], depending on
    /// the [`provider`](ITrans3Provider).
    fn get_global_trans(&self, world: &World) -> Trans3 {
        self.get_provider().get_global_trans(world)
    }
    /// Returns the transform relative to the parent.
    ///
    /// # Borrows
    ///
    /// Can recursively borrow ancestors implementing [`STrans3`], depending on
    /// the [`provider`](ITrans3Provider).
    fn get_local_trans(&self, world: &World) -> Trans3 {
        self.get_provider().get_local_trans(world)
    }
    /// Sets the transform relative to the owning
    /// [`Level`](crate::core::level::Level).
    ///
    /// # Borrows
    ///
    /// Can recursively borrow ancestors implementing [`STrans3`], depending on
    /// the [`provider`](ITrans3Provider), and will mutably borrow every
    /// descendant implementing [`STrans3`], recursively.
    fn set_global_trans(&mut self, world: &World, trans: Trans3) {
        self.get_provider_mut().set_global_trans(world, trans);

        let mut queue =
            VecDeque::from_iter(self.children().filter_map(|c| c.cast::<STrans3Id>().ok()));
        while let Some(c) = queue.pop_front() {
            let mut c = c.get_mut(world).expect("child removed");
            c.get_provider_mut().mark_dirty_single();
            queue.extend(c.children().filter_map(|c| c.cast().ok()));
        }
    }
    /// Sets the transform relative to the parent.
    ///
    /// # Borrows
    ///
    /// Can recursively borrow ancestors implementing [`STrans3`], depending on
    /// the [`provider`](ITrans3Provider), and will mutably borrow every
    /// descendant implementing [`STrans3`], recursively.
    fn set_local_trans(&mut self, world: &World, trans: Trans3) {
        self.get_provider_mut().set_local_trans(world, trans);

        let mut queue =
            VecDeque::from_iter(self.children().filter_map(|c| c.cast::<STrans3Id>().ok()));
        while let Some(c) = queue.pop_front() {
            let mut c = c.get_mut(world).expect("child removed");
            c.get_provider_mut().mark_dirty_single();
            queue.extend(c.children().filter_map(|c| c.cast().ok()));
        }
    }

    // TODO: More setters?
}

/// A 3D transform **provider**.
///
/// [`STrans3`] describes a Component *containing* a provider. This is the
/// actual implementation, for example [`Trans3ProviderRelative`].
pub trait ITrans3Provider {
    /// Returns the transform relative to the owning
    /// [`Level`](crate::core::level::Level).
    ///
    /// # Borrows
    ///
    /// Implementations may recursively borrow ancestors.
    fn get_global_trans(&self, world: &World) -> Trans3;
    /// Returns the transform relative to the parent.
    ///
    /// # Borrows
    ///
    /// Implementations may recursively borrow ancestors.
    fn get_local_trans(&self, world: &World) -> Trans3;
    /// Sets the transform relative to the owning
    /// [`Level`](crate::core::level::Level).
    ///
    /// # Borrows
    ///
    /// Implementations may recursively borrow ancestors.
    fn set_global_trans(&mut self, world: &World, trans: Trans3);
    /// Sets the transform relative to the parent.
    ///
    /// # Borrows
    ///
    /// Implementations may recursively borrow ancestors.
    fn set_local_trans(&mut self, world: &World, trans: Trans3);
    /// Marks that some cached transforms might be invalidated.
    ///
    /// Use interior mutability for cached values, since you will need to change
    /// them in a `get_x_trans` function.
    fn mark_dirty_single(&self);
}

/// 3D transform provider that inherits the parent transform.
///
/// This is useful for things that are attached to others, such as a physics
/// shape.
pub struct Trans3ProviderRelative {
    parent: Option<STrans3Id>,
    global_cached_trans: Cell<Option<Trans3>>,
    local_trans: Trans3,
}

impl Trans3ProviderRelative {
    /// Creates a new provider.
    ///
    /// `parent` must be the **parent** of the Component this provider is being
    /// used for, **not** the ID of the owning Component.
    pub fn new(parent: &ComponentParent) -> Self {
        Self {
            parent: match parent {
                ComponentParent::Component(id) => id.clone().cast().ok(),
                ComponentParent::Level(_) => None,
            },
            global_cached_trans: Cell::new(None),
            local_trans: Trans3::default(),
        }
    }
}

impl ITrans3Provider for Trans3ProviderRelative {
    /// # Borrows
    ///
    /// Recursively borrows ancestors implementing [`STrans3`] when the cached
    /// transform is not valid.
    fn get_global_trans(&self, world: &World) -> Trans3 {
        if let Some(global_trans) = self.global_cached_trans.get() {
            global_trans
        } else if let Some(parent_id) = &self.parent {
            let parent_trans = parent_id.get(world).expect("parent removed").get_global_trans(world);
            let global = parent_trans * self.local_trans;
            self.global_cached_trans.set(Some(global));
            global
        } else {
            // local = global because no parent
            self.global_cached_trans.set(Some(self.local_trans));
            self.local_trans
        }
    }

    fn get_local_trans(&self, _world: &World) -> Trans3 {
        self.local_trans
    }

    /// # Borrows
    ///
    /// Recursively borrows ancestors implementing [`STrans3`].
    fn set_global_trans(&mut self, world: &World, trans: Trans3) {
        if let Some(parent_id) = &self.parent {
            let parent_trans = parent_id.get(world).expect("parent removed").get_global_trans(world);
            self.local_trans = parent_trans.inv_mul(&trans);
        } else {
            // local = global because no parent
            self.local_trans = trans;
        }
        self.global_cached_trans.set(Some(trans));
    }

    fn set_local_trans(&mut self, _world: &World, trans: Trans3) {
        self.local_trans = trans;
    }

    fn mark_dirty_single(&self) {
        self.global_cached_trans.set(None);
    }
}

/// 3D transform provider that is authoritative over its global transform
///
/// This is useful for things like physics bodies, as they move independently
/// from their parents.
pub struct Trans3ProviderTop {
    parent: Option<STrans3Id>,
    local_cached_trans: Cell<Option<Trans3>>,
    global_trans: Trans3,
}

impl Trans3ProviderTop {
    /// Creates a new provider.
    ///
    /// `parent` must be the **parent** of the Component this provider is being
    /// used for, **not** the ID of the owning Component.
    pub fn new(parent: &ComponentParent) -> Self {
        Self {
            parent: match parent {
                ComponentParent::Component(id) => id.clone().cast().ok(),
                ComponentParent::Level(_) => None,
            },
            local_cached_trans: Cell::new(None),
            global_trans: Trans3::default(),
        }
    }
}

impl ITrans3Provider for Trans3ProviderTop {
    fn get_global_trans(&self, _world: &World) -> Trans3 {
        self.global_trans
    }

    /// # Borrows
    ///
    /// Recursively borrows ancestors implementing [`STrans3`] when the cached
    /// transform is not valid.
    fn get_local_trans(&self, world: &World) -> Trans3 {
        if let Some(local_trans) = self.local_cached_trans.get() {
            local_trans
        } else if let Some(parent_id) = &self.parent {
            let parent_trans = parent_id.get(world).expect("parent removed").get_global_trans(world);
            let local = parent_trans.inv_mul(&self.global_trans);
            self.local_cached_trans.set(Some(local));
            local
        } else {
            // local = global because no parent
            self.local_cached_trans.set(Some(self.global_trans));
            self.global_trans
        }
    }

    fn set_global_trans(&mut self, _world: &World, trans: Trans3) {
        self.global_trans = trans;
        self.local_cached_trans.set(None);
    }

    /// # Borrows
    ///
    /// Recursively borrows ancestors implementing [`STrans3`].
    fn set_local_trans(&mut self, world: &World, trans: Trans3) {
        if let Some(parent_id) = &self.parent {
            let parent_trans = parent_id.get(world).expect("parent removed").get_global_trans(world);
            self.global_trans = parent_trans * trans;
        } else {
            // local = global because no parent
            self.global_trans = trans;
        }
        self.local_cached_trans.set(Some(trans));
    }

    fn mark_dirty_single(&self) {
        self.local_cached_trans.set(None);
    }
}
