//! The entire state of the engine.
//! 
//! The [`World`] is created once and shared far and wide. Everything in the [`World`] relies on
//! non-blocking interior mutability, meaning the [`World`] cannot be shared across threads. The
//! [`World`] contains all [`Level`]s and Components.
use std::{cell::{Cell, RefCell}, collections::VecDeque, time::Duration};

use crate::engine::level::{Level, LevelIndex, LevelIndexOwned};

enum WorldAction {
    DeleteLevel(LevelIndexOwned),
}

/// The entire context of the engine, including Components.
pub struct World {
    levels: Box<boxcar::Vec<Level>>,
    action_queue: RefCell<VecDeque<WorldAction>>,

    stable_rate: Cell<Duration>,
    
}

impl World {
    pub(crate) fn new() -> Self {
        Self {
            levels: Box::new(boxcar::vec![Level::new(LevelIndex(0))]),
            action_queue: RefCell::new(VecDeque::new()),
            stable_rate: Cell::new(Duration::from_millis(15))
        }
    }

    pub(crate) fn flush_actions(&mut self) {
        while let Some(action) = self.action_queue.borrow_mut().pop_front() {
            match action {
                WorldAction::DeleteLevel(mut level) => {
                    self.levels
                        .get(level.0.try_into().expect("level index not valid"))
                        .expect("level index not valid")
                        .destroy_internal(self);

                    // Important: mark as non-leaking
                    level.0 = u32::MAX;
                }
            }
        }
    }

    /// Returns a reference to a [`Level`].
    pub fn get_level(&self, level: LevelIndex) -> Option<&Level> {
        self.levels.get(level.0 as usize)
    }

    /// Create a new [`Level`], returning the special index to free it.
    pub fn create_level(&self) -> LevelIndexOwned {
        let i = self
            .levels
            .count()
            .try_into()
            .expect("too many levels allocated");
        
        if i == u32::MAX {
            panic!("too many levels allocated");
        }

        self.levels.push(Level::new(LevelIndex(i)));
        LevelIndexOwned(i)
    }

    /// Destroy a [`Level`] using its owning index.
    pub fn destroy_level(&self, level: LevelIndexOwned) {
        self.action_queue
            .borrow_mut()
            .push_back(WorldAction::DeleteLevel(level));
    }

    /// Returns the main level, created when the main loop begins.
    pub fn main_level(&self) -> &Level {
        self.levels
            .get(0)
            .expect("main level added with new, never removed, still gone")
    }

    /// Returns an iterator over every level
    pub fn iter_levels(&self) -> impl Iterator<Item = &Level> {
        self.levels.iter().map(|(_, l)| l)
    }

    /// Returns the rate at which physics are run.
    pub fn get_stable_tick_rate(&self) -> Duration {
        self.stable_rate.get()
    }

    /// Sets the rate at which physics are run.
    pub fn set_stable_tick_rate(&self, rate: Duration) {
        self.stable_rate.set(rate);
    }
}
