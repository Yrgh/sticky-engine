//! The global string interner for asset paths.

use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

use parking_lot::RwLock;
use weak_table::WeakHashSet;

/// A global string interner for asset paths.
///
/// All asset paths are sent into this pool, reducing allocations application-wide.
pub struct Interner {
    inner: RwLock<WeakHashSet<Weak<str>>>,
    pinned: RwLock<PinSet>,
}

/// Maximum number of paths the interner keeps strongly pinned.
const PIN_CAP: usize = 256;

/// A bounded, LRU-ordered set of strongly-held paths.
#[derive(Default)]
struct PinSet {
    lru: VecDeque<Arc<str>>,
}

impl PinSet {
    /// Mark `path` as most-recently-pinned, evicting the least-recently-pinned
    /// entry if the pin set is full.
    fn touch(&mut self, path: Arc<str>) {
        if let Some(pos) = self.lru.iter().position(|p| Arc::ptr_eq(p, &path)) {
            self.lru.remove(pos);
        } else if self.lru.len() == PIN_CAP {
            // Evict the least-recently-pinned entry. If nothing else holds it
            // strongly, its weak entry will be swept on the next `periodic`.
            self.lru.pop_front();
        }
        self.lru.push_back(path);
    }
}

impl Interner {
    /// Intern a string, returning a shared [`Arc<str>`].
    ///
    /// If the string is currently interned, it will return the existing `Arc`.
    pub fn intern(&self, s: &str) -> Arc<str> {
        {
            let guard = self.inner.read();

            if let Some(arc) = guard.get(s) {
                return arc;
            }
        }

        let mut guard = self.inner.write();

        if let Some(arc) = guard.get(s) {
            return arc;
        }

        let arc: Arc<str> = s.into();

        guard.insert(arc.clone());

        arc
    }

    /// Intern a string and keep it strongly pinned.
    ///
    /// This behaves like [`intern`](Self::intern), but additionally ensures the
    /// returned [`Arc<str>`] stays live even if all references to the path are dropped.
    ///
    /// This is intended for paths of assets that were successfully loaded or
    /// saved, which are likely to be accessed again.
    pub fn pin_path(&self, s: &str) -> Arc<str> {
        let arc = self.intern(s);

        let mut guard = self.pinned.write();
        guard.touch(arc.clone());

        arc
    }

    pub(crate) fn periodic(&self) {
        let mut guard = self.inner.write();

        guard.remove_expired();
    }

    /// Remove all entries from the interner, releasing any pins.
    pub fn clear(&self) {
        self.inner.write().clear();
        self.pinned.write().lru.clear();
    }

    /// The number of distinct paths currently interned (including pins).
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Returns `true` if no paths are currently interned.
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    pub(crate) fn new() -> Self {
        Self {
            inner: RwLock::new(WeakHashSet::new()),
            pinned: RwLock::new(PinSet::default()),
        }
    }
}
