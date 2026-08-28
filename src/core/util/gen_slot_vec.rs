//! Generational slot vectors

use std::cell::{BorrowMutError, Cell, Ref, RefCell, RefMut};

use crate::core::{ComponentGetError, ComponentGetMutError, util::sentinel::SentinelMaxU32};

enum Storage<T> {
    Occupied(T),
    Reserved,
    Vacant(SentinelMaxU32),
}

type GenSto<T> = (u32, Storage<T>);

/// An index into a [`RefCellGenSlotVec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotIndex {
    pidx: u32,
    gidx: u32,
}

impl SlotIndex {
    /// Creates a new `SlotIndex` from a physical index and generation.
    pub const fn new(pidx: u32, gidx: u32) -> Self {
        Self { pidx, gidx }
    }

    /// Returns a new `SlotIndex` that doesn't correspond to a valid slot.
    pub const fn invalid() -> Self {
        Self::new(u32::MAX, u32::MAX)
    }

    /// Returns the physical index.
    pub fn pidx(&self) -> u32 {
        self.pidx
    }

    /// Returns the generation index.
    pub fn gidx(&self) -> u32 {
        self.gidx
    }
}

/// A generational slot vector using [`RefCell`] for interior mutability.
pub struct RefCellGenSlotVec<T> {
    data: Box<boxcar::Vec<RefCell<GenSto<T>>>>,
    free_head: Cell<SentinelMaxU32>,
    len: Cell<u32>,
}

impl<T> RefCellGenSlotVec<T> {
    /// Returns an empty `RefCellGenSlotVec`
    pub fn new() -> Self {
        Self {
            data: Box::new(boxcar::Vec::new()),
            free_head: Cell::new(SentinelMaxU32::NONE),
            len: Cell::new(0),
        }
    }

    /// Tries to remove the value at the given g
    pub fn remove(&self, index: SlotIndex) -> Result<bool, BorrowMutError> {
        let Some(cell) = self.data.get(index.pidx as usize) else {
            return Ok(false);
        };

        let mut cell = cell.try_borrow_mut()?;
        let cell_gidx = cell.0;

        match &mut cell.1 {
            Storage::Vacant(_) => Ok(false),
            Storage::Occupied(_) | Storage::Reserved if index.gidx != cell_gidx => Ok(false),
            Storage::Occupied(_) | Storage::Reserved => {
                cell.0 = cell_gidx.wrapping_add(1);
                cell.1 = Storage::Vacant(self.free_head.take());
                self.free_head.set(SentinelMaxU32::from_some(index.pidx));
                self.len.set(self.len.get() - 1);

                Ok(true)
            }
        }
    }

    /// Takes the value at the given index, leaving a vacant slot.
    ///
    /// Returns `None` if the index is out of bounds, the generation does not
    /// match, or the slot is not occupied.
    pub fn take(&self, index: SlotIndex) -> Result<Option<T>, BorrowMutError> {
        let Some(cell) = self.data.get(index.pidx as usize) else {
            return Ok(None);
        };

        let mut cell = cell.try_borrow_mut()?;
        let cell_gidx = cell.0;

        if index.gidx != cell_gidx {
            return Ok(None);
        }

        let Storage::Occupied(value) = std::mem::replace(
            &mut cell.1,
            Storage::Vacant(self.free_head.take()),
        ) else {
            return Ok(None);
        };

        cell.0 = cell_gidx.wrapping_add(1);
        self.free_head.set(SentinelMaxU32::from_some(index.pidx));
        self.len.set(self.len.get() - 1);

        Ok(Some(value))
    }

    /// Reserves a slot for deferred occupation.
    pub fn reserve(&self) -> SlotIndex {
        match self.free_head.take() {
            SentinelMaxU32::NONE => {
                let new_idx = self.data.push(RefCell::new((0, Storage::Reserved))) as u32;
                assert!(new_idx < u32::MAX, "too many slots created");
                SlotIndex {
                    pidx: new_idx,
                    gidx: 0,
                }
            }
            some => {
                let mut cell = self
                    .data
                    .get(some.into_inner() as usize)
                    .expect("free head should point to an inserted slot")
                    .borrow_mut();

                let Storage::Vacant(new_head) = std::mem::replace(&mut cell.1, Storage::Reserved)
                else {
                    panic!("free head shouldn't point at a non-Vacant slot.")
                };

                self.free_head.set(new_head);

                SlotIndex {
                    pidx: some.into_inner(),
                    gidx: cell.0,
                }
            }
        }
    }

    /// Fills a [`reserved`](Self::reserve) value, panicking if it wasn't `reserve`d.\
    pub fn fill(&self, index: SlotIndex, value: T) {
        let mut cell = self
            .data
            .get(index.pidx as usize)
            .expect("fill index point to an inserted slot")
            .borrow_mut();

        self.len.set(self.len.get() + 1);
        
        let Storage::Reserved = std::mem::replace(&mut cell.1, Storage::Occupied(value)) else {
            panic!("unreserved slot filled");
        };
    }

    /// Immediately inserts a value, rather than [`reserving`](Self::reserve)
    /// and [`filling`](Self::fill) it.
    pub fn insert(&self, value: T) -> SlotIndex {
        match self.free_head.take() {
            SentinelMaxU32::NONE => {
                let new_idx = self.data.push(RefCell::new((0, Storage::Occupied(value)))) as u32;
                assert!(new_idx < u32::MAX, "too many slots created");

                self.len.set(self.len.get() + 1);
                
                SlotIndex {
                    pidx: new_idx,
                    gidx: 0,
                }
            }
            some => {
                let mut cell = self
                    .data
                    .get(some.into_inner() as usize)
                    .expect("free head should point to an inserted slot")
                    .borrow_mut();

                let Storage::Vacant(new_head) =
                    std::mem::replace(&mut cell.1, Storage::Occupied(value))
                else {
                    panic!("free head shouldn't point at a non-Vacant slot.")
                };

                self.free_head.set(new_head);

                self.len.set(self.len.get() + 1);
                
                SlotIndex {
                    pidx: some.into_inner(),
                    gidx: cell.0,
                }
            }
        }
    }

    /// Gets and borrows a value immutably given the index.
    ///
    /// # Borrows
    ///
    /// Immutably borrows the value at the given index if it is valid.
    pub fn acquire(&self, index: SlotIndex) -> Result<Ref<'_, T>, ComponentGetError> {
        let cell = self
            .data
            .get(index.pidx as usize)
            .ok_or(ComponentGetError::NotFound)?
            .try_borrow()?;

        if cell.0 != index.gidx {
            return Err(ComponentGetError::NotFound);
        }

        Ref::filter_map(cell, |c| match &c.1 {
            Storage::Occupied(v) => Some(v),
            _ => None,
        })
        .map_err(|_| ComponentGetError::NotFound)
    }

    /// Gets and borrows a value mutably given the index.
    ///
    /// # Borrows
    ///
    /// Mutably borrows the value at the given index if it is valid.
    pub fn acquire_mut(&self, index: SlotIndex) -> Result<RefMut<'_, T>, ComponentGetMutError> {
        let cell = self
            .data
            .get(index.pidx as usize)
            .ok_or(ComponentGetMutError::NotFound)?
            .try_borrow_mut()?;

        if cell.0 != index.gidx {
            return Err(ComponentGetMutError::NotFound);
        }

        RefMut::filter_map(cell, |c| match &mut c.1 {
            Storage::Occupied(v) => Some(v),
            _ => None,
        })
        .map_err(|_| ComponentGetMutError::NotFound)
    }

    /// Returns an iterator over all values, borrowed immutably.
    ///
    /// # Borrows
    ///
    /// Immutably borrows all values *as the iterator is consumed*.
    pub fn iter(&self) -> impl Iterator<Item = Ref<'_, T>> {
        self.data.iter().filter_map(|(_, slot)| {
            Ref::filter_map(slot.borrow(), |s| match &s.1 {
                Storage::Occupied(v) => Some(v),
                _ => None,
            })
            .ok()
        })
    }

    /// Returns an iterator over all values, borrowed immutably.
    ///
    /// # Borrows
    ///
    /// Mutably borrows all values *as the iterator is consumed*.
    pub fn iter_mut(&self) -> impl Iterator<Item = RefMut<'_, T>> {
        self.data.iter().filter_map(|(_, slot)| {
            RefMut::filter_map(slot.borrow_mut(), |s| match &mut s.1 {
                Storage::Occupied(v) => Some(v),
                _ => None,
            })
            .ok()
        })
    }

    /// Returns an iterator over all IDs.
    ///
    /// # Borrows
    ///
    /// Immutably borrows all values *as the iterator is consumed*.
    pub fn ids(&self) -> impl Iterator<Item = SlotIndex> {
        self.data.iter().filter_map(|(idx, slot)| {
            let s = slot.borrow();
            match &s.1 {
                Storage::Occupied(_) => Some(SlotIndex {
                    pidx: idx as u32,
                    gidx: s.0,
                }),
                _ => None,
            }
        })
    }

    /// Returns the number of values currently added, including reserved ones.
    pub fn len(&self) -> u32 {
        self.len.get()
    }

    /// Returns `true` if there are no inserted or reserved slots.
    pub fn is_empty(&self) -> bool {
        self.len.get() == 0
    }

    /// Clears the slot vector.
    /// 
    /// # Borrows
    /// 
    /// Mutably borrows all values.
    pub fn clear(&self) {
        if self.is_empty() {
            return;
        }

        self.len.set(0);

        self.free_head.set(SentinelMaxU32::from_inner(0));

        let len = self.data.count();

        for (i, cell) in self.data.iter() {
            let i_p1 = i + 1;
            let next = if i_p1 == len {
                SentinelMaxU32::NONE
            } else {
                SentinelMaxU32::from_some(i_p1 as u32)
            };

            *cell.borrow_mut() = (0, Storage::Vacant(next));
        }
    }
    
}

impl<T> Default for RefCellGenSlotVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

