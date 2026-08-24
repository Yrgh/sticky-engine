//! Distinct Component containers.
//! 
//! In this engine, Components are divided up into [`Level`]s. Each `Level`
//! contains a list of Components inside and a list of top-level Components.
//! Each `Level` renders and simulates physics independently.

use std::{
    any::{Any, TypeId},
    cell::{Cell, Ref, RefCell, RefMut},
};

use elsa::FrozenMap;

use crate::{
    core::{
        component::{ComponentId, DynComponentId, IComponent, ISlotId}, relations::{BuildTypeIdHasher, RELATIONS}, renderer::{PrimaryRenderingQueue, RenderingQueue}, window::WindowId, world::World,
    }, log,
};

trait IColumn: Any {
    fn len(&self) -> u32;

    fn remove(&self, pidx: u32, gidx: u32);

    fn get_pairs(&self) -> Box<dyn Iterator<Item=(u32, u32)> + '_>;

    fn get_dyn(&self, pidx: u32, gidx: u32) -> Option<Ref<'_, dyn IComponent>>;
    fn get_dyn_mut(&self, pidx: u32, gidx: u32) -> Option<RefMut<'_, dyn IComponent>>;
}

struct Column<C: IComponent> {
    slots: boxcar::Vec<RefCell<(u32, SlotValue<C>)>>,
    free_head: Cell<Option<u32>>,
    len: Cell<u32>,
}

impl<C: IComponent> Column<C> {
    fn get_mut(&self, pidx: u32) -> Option<RefMut<'_, (u32, SlotValue<C>)>> {
        Some(self.slots.get(pidx as usize)?.borrow_mut())
    }
}

impl<C: IComponent> IColumn for Column<C> {
    fn len(&self) -> u32 {
        self.len.get()
    }

    fn remove(&self, pidx: u32, gidx: u32) {
        let Some(mut storage) = self.get_mut(pidx) else {
            return;
        };

        if storage.0 == gidx {
            storage.0 = gidx.wrapping_add(1);
            storage.1 = SlotValue::Vacant(self.free_head.get());
            self.free_head.set(Some(gidx));
            self.len.set(self.len.get() - 1);
        }
    }

    fn get_pairs(&self) -> Box<dyn Iterator<Item=(u32, u32)> + '_> {
        Box::new(
            self.slots
                .iter()
                .filter_map(|(pidx, storage)| {
                    let b = storage.borrow();
                    if let SlotValue::Occupied(_) = &b.1 {
                        Some((pidx as u32, b.0))
                    } else {
                        None
                    }
                })
        )
    }

    fn get_dyn(&self, pidx: u32, gidx: u32) -> Option<Ref<'_, dyn IComponent>> {
        Ref::filter_map(
            self.slots.get(pidx as usize)?.borrow(),
            |s| -> Option<&dyn IComponent> {
                if s.0 == gidx
                    && let SlotValue::Occupied(c) = &s.1
                {
                    Some(c)
                } else {
                    None
                }
            },
        )
        .ok()
    }

    fn get_dyn_mut(&self, pidx: u32, gidx: u32) -> Option<RefMut<'_, dyn IComponent>> {
        RefMut::filter_map(
            self.slots.get(pidx as usize)?.borrow_mut(),
            |s| -> Option<&mut dyn IComponent> {
                if s.0 == gidx
                    && let SlotValue::Occupied(c) = &mut s.1
                {
                    Some(c)
                } else {
                    None
                }
            },
        )
        .ok()
    }
}

enum SlotValue<C: IComponent> {
    Occupied(C),
    Reserved,
    /// The next free item, if there are any
    Vacant(Option<u32>),
}

/// Container for an isolated set of Components, complete with its own physics simulation and
/// render targets.
///
/// Each `Level` is separate from every other. Components should only have children in the same
/// `Level` as themselves. Each `Level` has a list of root Components
pub struct Level {
    self_idx: Cell<Option<LevelIndex>>,
    component_columns: FrozenMap<TypeId, Box<dyn IColumn>, BuildTypeIdHasher>,
    top_level: RefCell<Vec<DynComponentId>>,

    rendering_queue: RefCell<RenderingQueue>,

    window: Option<WindowId>,
}

impl Level {
    pub(crate) fn new(self_idx: LevelIndex) -> Self {
        Self {
            self_idx: Cell::new(Some(self_idx)),
            component_columns: FrozenMap::default(),
            top_level: RefCell::new(Vec::new()),
            rendering_queue: RefCell::new(RenderingQueue::new()),
            window: None,
        }
    }

    pub(crate) fn for_window(self_idx: LevelIndex, window: WindowId) -> Self {
        Self {
            self_idx: Cell::new(Some(self_idx)),
            component_columns: FrozenMap::default(),
            top_level: RefCell::new(Vec::new()),
            rendering_queue: RefCell::new(RenderingQueue::new()),
            window: Some(window),
        }
    }

    /// Returns the index of the bound window, if there is any.
    pub fn get_window(&self) -> Option<WindowId> {
        self.window
    }

    /// Returns the [`LevelIndex`] that accesses this `Level`.
    pub fn id(&self) -> LevelIndex {
        self.self_idx.get().expect("level not active")
    }

    #[doc(hidden)]
    /// (**INTERNAL**) Removes a component.
    ///
    /// WARNING: This method is safe to use, but is intended to be used
    /// internally. It is only made public so it can be generated by a macro.
    /// Only use this if you know what you are doing.
    pub fn remove_component_internal(&self, tyid: TypeId, pidx: u32, gidx: u32) {
        let Some(column) = self.component_columns.get(&tyid) else {
            return;
        };

        column.remove(pidx, gidx);
    }

    #[doc(hidden)]
    /// (**INTERNAL**) Inserts a component.
    ///
    /// WARNING: This method is safe to use, but is intended to be used
    /// internally. It is only made public so it can be generated by a macro.
    /// Only use this if you know what you are doing.
    pub fn insert_component_internal<C: IComponent>(&self, value: C) -> ComponentId<C> {
        let column = match self.component_columns.get(&TypeId::of::<C>()) {
            Some(o) => o,
            None => self.component_columns.insert(
                TypeId::of::<C>(),
                Box::new(Column::<C> {
                    slots: boxcar::Vec::new(),
                    free_head: Cell::new(None),
                    len: Cell::new(0),
                }),
            ),
        };

        if column.len() == u32::MAX {
            panic!("value inserted to full column");
        }

        let Some(column) = <dyn Any>::downcast_ref::<Column<C>>(column) else {
            unreachable!()
        };

        match column.free_head.get() {
            Some(head) => {
                let Some(mut storage) = column.get_mut(head) else {
                    unreachable!("free head should always point to an allocated slot")
                };

                let SlotValue::Vacant(next_head) = storage.1 else {
                    unreachable!("free head should always point to a vacant slot")
                };

                column.free_head.set(next_head);
                column.len.set(column.len.get() - 1);

                storage.1 = SlotValue::Occupied(value);

                unsafe { ComponentId::from_parts(head, storage.0, self.id(), TypeId::of::<C>()) }
            }
            None => {
                column
                    .slots
                    .push(RefCell::new((0, SlotValue::Occupied(value))));
                let len_sub1 = column.len.get();
                column.len.set(len_sub1 + 1);

                unsafe { ComponentId::from_parts(len_sub1, 0, self.id(), TypeId::of::<C>()) }
            }
        }
    }

    #[doc(hidden)]
    /// (**INTERNAL**) Reserves a component.
    ///
    /// WARNING: This method is safe to use, but is intended to be used
    /// internally. It is only made public so it can be generated by a macro.
    /// Only use this if you know what you are doing.
    pub fn reserve_slot_internal<C: IComponent>(&self) -> ComponentId<C> {
        let column = match self.component_columns.get(&TypeId::of::<C>()) {
            Some(o) => o,
            None => self.component_columns.insert(
                TypeId::of::<C>(),
                Box::new(Column::<C> {
                    slots: boxcar::Vec::new(),
                    free_head: Cell::new(None),
                    len: Cell::new(0),
                }),
            ),
        };

        if column.len() == u32::MAX {
            panic!("value inserted to full column");
        }

        let Some(column) = <dyn Any>::downcast_ref::<Column<C>>(column) else {
            unreachable!()
        };

        match column.free_head.get() {
            Some(head) => {
                let Some(mut storage) = column.get_mut(head) else {
                    unreachable!("free head should always point to an allocated slot")
                };

                let SlotValue::Vacant(next_head) = storage.1 else {
                    unreachable!("free head should always point to a vacant slot")
                };

                column.free_head.set(next_head);
                column.len.set(column.len.get() + 1);

                storage.1 = SlotValue::Reserved;

                unsafe { ComponentId::from_parts(head, storage.0, self.id(), TypeId::of::<C>()) }
            }
            None => {
                column.slots.push(RefCell::new((0, SlotValue::Reserved)));
                let len_sub1 = column.len.get();
                column.len.set(len_sub1 + 1);

                unsafe { ComponentId::from_parts(len_sub1, 0, self.id(), TypeId::of::<C>()) }
            }
        }
    }

    #[doc(hidden)]
    /// (**INTERNAL**) Fills a slot with a component.
    ///
    /// WARNING: This method is safe to use, but is intended to be used
    /// internally. It is only made public so it can be generated by a macro.
    /// Only use this if you know what you are doing.
    pub fn fill_slot_internal<C: IComponent>(&self, pidx: u32, gidx: u32, value: C) {
        let Some(column) = self.component_columns.get(&TypeId::of::<C>()) else {
            return;
        };

        if pidx >= column.len() {
            return;
        }

        let Some(column) = <dyn Any>::downcast_ref::<Column<C>>(column) else {
            return;
        };

        let Some(mut storage) = column.get_mut(pidx) else {
            return;
        };

        if gidx != storage.0 {
            return;
        }

        match storage.1 {
            SlotValue::Reserved => storage.1 = SlotValue::Occupied(value),
            _ => panic!("filling slot that isn't reserved"),
        }
    }

    #[doc(hidden)]
    /// (**INTERNAL**) Acquires a component *immutably*.
    ///
    /// WARNING: This method is safe to use, but is intended to be used
    /// internally. It is only made public so it can be generated by a macro.
    /// Only use this if you know what you are doing.
    pub fn acquire_component_internal<C: IComponent>(
        &self,
        pidx: u32,
        gidx: u32,
    ) -> Option<Ref<'_, C>> {
        let column = self.component_columns.get(&TypeId::of::<C>())?;

        if pidx >= column.len() {
            return None;
        }

        let column = <dyn Any>::downcast_ref::<Column<C>>(column)?;

        let storage = column.slots.get(pidx as usize)?.borrow();

        Ref::filter_map(storage, |storage| {
            if gidx != storage.0 {
                return None;
            }

            match &storage.1 {
                SlotValue::Occupied(slot) => Some(slot),
                SlotValue::Vacant(_) | SlotValue::Reserved => None,
            }
        })
        .ok()
    }

    #[doc(hidden)]
    /// (**INTERNAL**) Acquires a component *mutably*.
    ///
    /// WARNING: This method is safe to use, but is intended to be used
    /// internally. It is only made public so it can be generated by a macro.
    /// Only use this if you know what you are doing.
    pub fn acquire_component_internal_mut<C: IComponent>(
        &self,
        pidx: u32,
        gidx: u32,
    ) -> Option<RefMut<'_, C>> {
        let column = self.component_columns.get(&TypeId::of::<C>())?;

        if pidx >= column.len() {
            return None;
        }

        let column = <dyn Any>::downcast_ref::<Column<C>>(column)?;

        let storage = column.get_mut(pidx)?;

        RefMut::filter_map(storage, |storage| {
            if gidx != storage.0 {
                return None;
            }

            match &mut storage.1 {
                SlotValue::Occupied(slot) => Some(slot),
                SlotValue::Vacant(_) | SlotValue::Reserved => None,
            }
        })
        .ok()
    }

    #[doc(hidden)]
    /// (**INTERNAL**) Acquires a component *dynamically*.
    ///
    /// WARNING: This method is safe to use, but is intended to be used
    /// internally. It is only made public so it can be generated by a macro.
    /// Only use this if you know what you are doing.
    pub fn acquire_component_internal_dyn(
        &self,
        tyid: TypeId,
        pidx: u32,
        gidx: u32,
    ) -> Option<Ref<'_, dyn IComponent>> {
        let column = self.component_columns.get(&tyid)?;

        column.get_dyn(pidx, gidx)
    }

    #[doc(hidden)]
    /// (**INTERNAL**) Acquires a component *dynamically **and** mutably*.
    ///
    /// WARNING: This method is safe to use, but is intended to be used
    /// internally. It is only made public so it can be generated by a macro.
    /// Only use this if you know what you are doing.
    pub fn acquire_component_internal_dyn_mut(
        &self,
        tyid: TypeId,
        pidx: u32,
        gidx: u32,
    ) -> Option<RefMut<'_, dyn IComponent>> {
        let column = self.component_columns.get(&tyid)?;

        column.get_dyn_mut(pidx, gidx)
    }

    #[doc(hidden)]
    /// (**INTERNAL**) Removes all resources and Components, and marks the level as inactive.
    ///
    /// WARNING: This method is safe to use, but is intended to be used
    /// internally. It is only made public so it can be generated by a macro.
    /// Only use this if you know what you are doing.
    pub fn destroy_internal(&self, world: &World) {
        for top_id in self.top_level.borrow_mut().drain(..) {
            let (top_pidx, top_gidx, top_tyid) = top_id.acquire_parts();
            let mut top = top_id.get_mut(world).expect("just acquired from top");
            top.destroy_hook(world);
            drop(top);

            self.remove_component_internal(top_tyid, top_pidx, top_gidx);
        }
    }

    /// Spawn a new Component at the end of the top level list.
    pub fn spawn_top_level<C: IComponent>(&self, world: &World) -> ComponentId<C> {
        let id = C::spawn(world, self.id().into());
        self.top_level.borrow_mut().push(id.clone().into());
        id
    }

    /// Returns the index of an ID in the top level list, if it exists.
    pub fn find_top_level<D: ISlotId>(&self, id: &D) -> Option<usize> {
        self.top_level.borrow().iter().position(|id2| id2 == id)
    }

    /// Returns an iterator over the IDs of all Components in the top level list.
    pub fn iter_top_level(&self) -> impl Iterator<Item = DynComponentId> {
        self.top_level.borrow().clone().into_iter()
    }

    /// Removes a Component from the top level list *by position*.
    pub fn remove_top_level(&self, position: usize, world: &World) {
        let id = self.top_level.borrow_mut().remove(position);
        id.get_mut(world)
            .expect("id is in world")
            .destroy_hook(world);
        let (pidx, gidx, tyid) = id.acquire_parts();
        self.remove_component_internal(tyid, pidx, gidx);
    }

    /// Returns an iterator over all Components of a given type, as *immutable* borrows.
    pub fn iter_type<'a, C: IComponent>(&'a self) -> Box<dyn Iterator<Item = Ref<'a, C>> + 'a> {
        let Some(column) = self.component_columns.get(&TypeId::of::<C>()) else {
            return Box::new(std::iter::empty());
        };
        let column = <dyn Any>::downcast_ref::<Column<C>>(column).expect("column is for type C");

        Box::new(column.slots.iter().flat_map(|(_, slot)| {
            Ref::filter_map(slot.borrow(), |s| match &s.1 {
                SlotValue::Occupied(c) => Some(c),
                _ => None,
            })
            .into_iter()
        }))
    }

    /// Returns an iterator over all Components of a given type, as *mutable* borrows.
    pub fn iter_type_mut<'a, C: IComponent>(
        &'a self,
    ) -> Box<dyn Iterator<Item = RefMut<'a, C>> + 'a> {
        let Some(column) = self.component_columns.get(&TypeId::of::<C>()) else {
            return Box::new(std::iter::empty());
        };
        let column = <dyn Any>::downcast_ref::<Column<C>>(column).expect("column is for type C");

        Box::new(column.slots.iter().flat_map(|(_, slot)| {
            RefMut::filter_map(slot.borrow_mut(), |s| match &mut s.1 {
                SlotValue::Occupied(c) => Some(c),
                _ => None,
            })
            .into_iter()
        }))
    }

    /// Returns a list of all Components in the `Level` that can be identified by `D`.
    pub fn all_matching_ids<D: ISlotId>(&self) -> impl Iterator<Item = D> {
        let self_id = self.id();
        RELATIONS
            .iter_slot_tys::<D>()
            .flat_map(move |tyid| {
                // If the column exists, get all components in it.
                // If it doesn't, we need to match the type, so create a new Box
                match self.component_columns.get(&tyid) {
                    Some(column) => column.get_pairs(),
                    None => Box::new(std::iter::empty()),
                }
                .map(move |(pidx, gidx)| unsafe {
                    D::from_parts(pidx, gidx, self_id, tyid)
                })
            })
    }

    pub(crate) fn update_rendering_queue(&self) -> Ref<'_, RenderingQueue> {
        // TODO: Actually update the rendering queue

        self.rendering_queue.borrow()
    }
}

/// Non-owning index within the [`World`] of a [`Level`].
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct LevelIndex(pub(crate) u32, pub(crate) u32);

/// Singly-owning index within the [`World`] of a [`Level`].
#[derive(Hash)]
pub struct LevelIndexOwned(pub(crate) u32, pub(crate) u32);

impl LevelIndexOwned {
    /// Returns a non-owning copy of this index.
    pub fn handle(&self) -> LevelIndex {
        LevelIndex(self.0, self.1)
    }

    /// Leaks the index. The [`Level`] will live until the [`World`] is dropped.
    /// 
    /// Avoid silent-dropping a `LevelIndexOwned`, as it logs an error unless
    /// you call this manually.
    pub fn leak(mut self) {
        self.0 = u32::MAX;
        self.1 = u32::MAX
    }
}

impl Drop for LevelIndexOwned {
    fn drop(&mut self) {
        if self.0 != u32::MAX && self.1 != u32::MAX {
            log!(err: "Leaked level {:?}", self.handle())
        }
    }
}

impl PartialEq<LevelIndex> for LevelIndexOwned {
    fn eq(&self, other: &LevelIndex) -> bool {
        &self.handle() == other
    }
}

impl PartialEq<LevelIndexOwned> for LevelIndex {
    fn eq(&self, other: &LevelIndexOwned) -> bool {
        self == &other.handle()
    }
}
