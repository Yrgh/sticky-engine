//! Generational slot vectors

use std::cell::{BorrowMutError, Cell, Ref, RefCell, RefMut, UnsafeCell};

use crate::core::{GetError, GetMutError, util::sentinel::SentinelMaxU32};

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

    /// Returns whether this index is invalid.
    pub fn is_valid(&self) -> bool {
        self.pidx != u32::MAX && self.gidx != u32::MAX
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
    pub fn remove(&self, index: SlotIndex) -> Result<(), GetMutError> {
        let Some(cell) = self.data.get(index.pidx as usize) else {
            return Err(GetMutError::NotFound);
        };

        let mut cell = cell.try_borrow_mut()?;
        let cell_gidx = cell.0;

        match &mut cell.1 {
            Storage::Vacant(_) => Err(GetMutError::NotFound),
            Storage::Occupied(_) | Storage::Reserved if index.gidx != cell_gidx => {
                Err(GetMutError::NotFound)
            }
            Storage::Occupied(_) => {
                cell.0 = cell_gidx.wrapping_add(1);
                cell.1 = Storage::Vacant(self.free_head.replace(self.free_head.replace(SentinelMaxU32::from_some(index.pidx))));
                self.len.set(self.len.get() - 1);

                Ok(())
            }
            Storage::Reserved => {
                cell.0 = cell_gidx.wrapping_add(1);
                cell.1 = Storage::Vacant(self.free_head.replace(self.free_head.replace(SentinelMaxU32::from_some(index.pidx))));

                Ok(())
            }
        }
    }

    /// Takes the value at the given index, leaving a vacant slot.
    ///
    /// Returns `None` if the index is out of bounds, the generation does not
    /// match, or the slot is not occupied.
    /// 
    /// Note: this does not remove reserved slots.
    pub fn take(&self, index: SlotIndex) -> Result<Option<T>, BorrowMutError> {
        let Some(cell) = self.data.get(index.pidx as usize) else {
            return Ok(None);
        };

        let mut cell = cell.try_borrow_mut()?;
        let cell_gidx = cell.0;

        if index.gidx != cell_gidx {
            return Ok(None);
        }

        match &cell.1 {
            Storage::Occupied(_) => {
                let Storage::Occupied(value) = std::mem::replace(&mut cell.1, Storage::Vacant(self.free_head.replace(SentinelMaxU32::from_some(index.pidx)))) else {
                    unreachable!()
                };

                cell.0 = cell_gidx.wrapping_add(1);
                self.len.set(self.len.get() - 1);

                Ok(Some(value))
            }
            Storage::Reserved | Storage::Vacant(_) => {
                Ok(None)
            }
        }
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
    pub fn acquire(&self, index: SlotIndex) -> Result<Ref<'_, T>, GetError> {
        let cell = self
            .data
            .get(index.pidx as usize)
            .ok_or(GetError::NotFound)?
            .try_borrow()?;

        if cell.0 != index.gidx {
            return Err(GetError::NotFound);
        }

        Ref::filter_map(cell, |c| match &c.1 {
            Storage::Occupied(v) => Some(v),
            _ => None,
        })
        .map_err(|_| GetError::NotFound)
    }

    /// Gets and borrows a value mutably given the index.
    ///
    /// # Borrows
    ///
    /// Mutably borrows the value at the given index if it is valid.
    pub fn acquire_mut(&self, index: SlotIndex) -> Result<RefMut<'_, T>, GetMutError> {
        let cell = self
            .data
            .get(index.pidx as usize)
            .ok_or(GetMutError::NotFound)?
            .try_borrow_mut()?;

        if cell.0 != index.gidx {
            return Err(GetMutError::NotFound);
        }

        RefMut::filter_map(cell, |c| match &mut c.1 {
            Storage::Occupied(v) => Some(v),
            _ => None,
        })
        .map_err(|_| GetMutError::NotFound)
    }

    #[expect(dead_code)]
    pub(crate) fn get_mut(&mut self, index: SlotIndex) -> Option<&mut T> {
        let cell = self.data.get_mut(index.pidx as usize)?.get_mut();

        if cell.0 != index.gidx {
            return None;
        }

        match &mut cell.1 {
            Storage::Occupied(v) => Some(v),
            _ => None,
        }
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

/// Like a [`RefCellGenSlotVec`], but only returns shared references and
/// requires mutable access for removal.
pub struct NoMutGenSlotVec<T> {
    data: Box<boxcar::Vec<UnsafeCell<GenSto<T>>>>,
    free_head: Cell<SentinelMaxU32>,
    len: Cell<u32>,
}

impl<T> NoMutGenSlotVec<T> {
    /// Creates a new, empty [`NoMutGenSlotVec`].
    pub fn new() -> Self {
        Self {
            data: Box::new(boxcar::Vec::new()),
            free_head: Cell::new(SentinelMaxU32::NONE),
            len: Cell::new(0),
        }
    }

    /// Returns the number of elements in the [`NoMutGenSlotVec`].
    pub fn len(&self) -> u32 {
        self.len.get()
    }

    /// Returns `true` if the [`NoMutGenSlotVec`] is empty.
    pub fn is_empty(&self) -> bool {
        self.len.get() == 0
    }

    /// Inserts a value into the [`NoMutGenSlotVec`] and returns its index.
    pub fn insert(&self, value: T) -> SlotIndex {
        match self.free_head.take() {
            SentinelMaxU32::NONE => {
                let new_idx = self
                    .data
                    .push(UnsafeCell::new((0, Storage::Occupied(value))))
                    as u32;
                assert!(new_idx < u32::MAX, "too many slots created");

                self.len.set(self.len.get() + 1);

                SlotIndex {
                    pidx: new_idx,
                    gidx: 0,
                }
            }
            some => {
                let slot_ptr = self
                    .data
                    .get(some.into_inner() as usize)
                    .expect("free head should point to an inserted slot")
                    .get();

                // SAFETY: No mutable references + `!Sync` guarantees atomicity.
                let cell = unsafe { slot_ptr.as_ref_unchecked() };

                assert!(
                    matches!(cell.1, Storage::Vacant(_)),
                    "free head shouldn't point at a non-Vacant slot."
                );

                // SAFETY: Same as above + there can be no persistent references
                let cell = unsafe { slot_ptr.as_mut_unchecked() };

                let Storage::Vacant(new_head) =
                    std::mem::replace(&mut cell.1, Storage::Occupied(value))
                else {
                    unreachable!()
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

    /// Reserve a slot to be [`fill`](Self::fill)ed later, returning the index.
    pub fn reserve(&self) -> SlotIndex {
        match self.free_head.take() {
            SentinelMaxU32::NONE => {
                let new_idx = self.data.push(UnsafeCell::new((0, Storage::Reserved))) as u32;
                assert!(new_idx < u32::MAX, "too many slots created");

                SlotIndex {
                    pidx: new_idx,
                    gidx: 0,
                }
            }
            some => {
                let slot_ptr = self
                    .data
                    .get(some.into_inner() as usize)
                    .expect("free head should point to an inserted slot")
                    .get();

                // SAFETY: No mutable references + `!Sync` guarantees atomicity.
                let cell = unsafe { slot_ptr.as_ref_unchecked() };

                assert!(
                    matches!(cell.1, Storage::Vacant(_)),
                    "free head shouldn't point at a non-Vacant slot."
                );

                // SAFETY: Same as above + there can be no persistent references
                let cell = unsafe { slot_ptr.as_mut_unchecked() };

                let Storage::Vacant(new_head) = std::mem::replace(&mut cell.1, Storage::Reserved)
                else {
                    unreachable!()
                };

                self.free_head.set(new_head);

                SlotIndex {
                    pidx: some.into_inner(),
                    gidx: cell.0,
                }
            }
        }
    }

    /// Fill a reserved slot with a value.
    /// 
    /// # Panics
    /// If the the index is invalid, or if the slot is already filled or was removed.
    pub fn fill(&self, index: SlotIndex, value: T) {
        let slot_ptr = self
            .data
            .get(index.pidx as usize)
            .expect("slot index should be valid")
            .get();

        // SAFETY: No mutable references + `!Sync` guarantees atomicity.
        let cell = unsafe { slot_ptr.as_ref_unchecked() };

        assert!(
            matches!(cell.1, Storage::Reserved),
            "slot should be reserved"
        );

        // SAFETY: Same as above + there can be no persistent references
        let cell = unsafe { slot_ptr.as_mut_unchecked() };

        cell.1 = Storage::Occupied(value);
        self.len.set(self.len.get() + 1);
    }

    /// Remove a slot, returning whether it was successful.
    /// 
    /// Note: removing a reserved slot does not count as success, but does
    /// actually do something.
    pub fn remove(&mut self, index: SlotIndex) -> bool {
        let Some(slot) = self.data.get_mut(index.pidx as usize) else {
            return false;
        };

        let cell = slot.get_mut();

        match &cell.1 {
            Storage::Occupied(_) => {
                cell.1 = Storage::Vacant(self.free_head.replace(SentinelMaxU32::from_some(index.pidx)));
                cell.0 = cell.0.wrapping_add(1);
                self.len.set(self.len.get() - 1);
                true
            }
            Storage::Reserved => {
                cell.1 = Storage::Vacant(self.free_head.replace(SentinelMaxU32::from_some(index.pidx)));
                cell.0 = cell.0.wrapping_add(1);
                false
            }
            Storage::Vacant(_) => false,
        }
    }

    /// Get the value at the given index.
    pub fn get(&self, index: SlotIndex) -> Option<&T> {
        let slot = self.data.get(index.pidx as usize)?;

        // SAFETY: We cannot own a mutable reference now, and we won't mutate a
        // borrowed value.
        let cell = unsafe { slot.get().as_ref_unchecked() };

        if cell.0 != index.gidx {
            return None;
        }

        if let Storage::Occupied(value) = &cell.1 {
            Some(value)
        } else {
            None
        }
    }

    /// Mutably get the value at the given index.
    pub fn get_mut(&mut self, index: SlotIndex) -> Option<&mut T> {
        let cell = self.data.get_mut(index.pidx as usize)?.get_mut();

        if cell.0 != index.gidx {
            return None;
        }

        if let Storage::Occupied(value) = &mut cell.1 {
            Some(value)
        } else {
            None
        }
    }

    /// Iterate immutably over all values.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter().filter_map(|(_, slot)| {
            // SAFETY: We cannot own a mutable reference now + all non-transient
            // references cannot not get mutably borrowed.
            let cell = unsafe { slot.get().as_ref_unchecked() };
            match &cell.1 {
                Storage::Occupied(value) => Some(value),
                _ => None
            }
        })
    }
}

impl<T> Default for NoMutGenSlotVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn basic_insert_get() {
        let v = NoMutGenSlotVec::<String>::new();
        let idx = v.insert("hello".into());
        assert_eq!(v.len(), 1);
        assert_eq!(v.get(idx).map(String::as_str), Some("hello"));
    }

    #[test]
    fn insert_remove_retrieve() {
        let mut v = NoMutGenSlotVec::<i32>::new();
        let idx = v.insert(42);
        assert!(v.remove(idx));
        assert_eq!(v.len(), 0);
        assert!(v.get(idx).is_none());
    }

    #[test]
    fn generation_recycling() {
        let mut v = NoMutGenSlotVec::<u64>::new();
        let idx1 = v.insert(100);
        assert!(v.remove(idx1));
        let idx2 = v.insert(200);
        assert_eq!(idx1.pidx(), idx2.pidx());
        assert_ne!(idx1.gidx(), idx2.gidx());
        assert!(v.get(idx1).is_none());
        assert_eq!(v.get(idx2), Some(&200));
    }

    #[test]
    fn reserve_fill_get() {
        let v = NoMutGenSlotVec::<String>::new();
        let idx = v.reserve();
        assert!(v.get(idx).is_none());
        assert_eq!(v.len(), 0);
        v.fill(idx, "reserved".into());
        assert_eq!(v.len(), 1);
        assert_eq!(v.get(idx).map(String::as_str), Some("reserved"));
    }

    #[test]
    fn iter_all_values() {
        let v = NoMutGenSlotVec::<i32>::new();
        let _a = v.insert(10);
        let _b = v.insert(20);
        let _c = v.insert(30);

        let mut vals: Vec<i32> = v.iter().copied().collect();
        vals.sort();
        assert_eq!(vals, vec![10, 20, 30]);
    }

    #[test]
    fn aliased_insert_during_get_reference() {
        let v = NoMutGenSlotVec::<String>::new();
        let idx = v.insert("first".into());
        let reference = v.get(idx).unwrap();

        // insert takes &self, so this is allowed by the borrow checker.
        // The reference must remain valid.
        let _idx2 = v.insert("second".into());
        assert_eq!(reference, "first");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn iter_with_concurrent_insert() {
        let v = NoMutGenSlotVec::<i32>::new();
        let _ = v.insert(1);

        let first: Vec<&i32> = v.iter().collect();
        assert_eq!(first, vec![&1]);

        let _ = v.insert(2);
        let all: Vec<&i32> = v.iter().collect();
        assert_eq!(all, vec![&1, &2]);
    }

    #[test]
    fn stress_recycling() {
        let mut v = NoMutGenSlotVec::<usize>::new();
        let mut indices = Vec::new();

        for i in 0..200 {
            indices.push(v.insert(i));
        }
        assert_eq!(v.len(), 200);

        for &idx in indices.iter().step_by(2) {
            assert!(v.remove(idx));
        }
        assert_eq!(v.len(), 100);

        for i in 0..100 {
            let idx = v.insert(i + 1000);
            indices.push(idx);
        }
        assert_eq!(v.len(), 200);

        for &idx in indices.iter() {
            let _ = v.get(idx);
        }
    }

    #[test]
    #[should_panic(expected = "slot should be reserved")]
    fn fill_occupied_slot_panics() {
        let v = NoMutGenSlotVec::<i32>::new();
        let idx = v.insert(1);
        v.fill(idx, 2);
    }

    #[test]
    #[should_panic(expected = "slot index should be valid")]
    fn fill_invalid_index_panics() {
        let v = NoMutGenSlotVec::<i32>::new();
        v.fill(SlotIndex::invalid(), 1);
    }

    #[test]
    fn remove_reserved_slot() {
        let mut v = NoMutGenSlotVec::<i32>::new();
        let idx = v.reserve();
        assert!(!v.remove(idx));
        assert_eq!(v.len(), 0);
    }

    #[test]
    fn get_stale_index_after_remove_and_reinsert() {
        let mut v = NoMutGenSlotVec::<String>::new();
        let idx = v.insert("a".into());
        assert!(v.remove(idx));
        let _new_idx = v.insert("b".into());
        assert!(v.get(idx).is_none());
    }
}
