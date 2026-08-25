//! The entire state of the engine.
//!
//! The [`World`] is created once and shared far and wide. Everything in the [`World`] relies on
//! non-blocking interior mutability, meaning the [`World`] cannot be shared across threads. The
//! [`World`] contains all [`Level`]s, [`IWindow`]s, and Components.
use std::{
    cell::{Cell, OnceCell, Ref, RefCell, RefMut, UnsafeCell},
    collections::VecDeque,
    sync::Arc,
    time::Duration,
};

use anyhow::Result as AResult;

use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::WindowAttributes};

use crate::core::window::IWindow;
use crate::core::{
    level::{Level, LevelIndex, LevelIndexOwned},
    vk::VkContext,
    window::{IWindowBoth, RootWindow, WindowId, WindowIdOwned},
};

enum WorldAction {
    DeleteLevel(LevelIndexOwned),
    DeleteWindow(WindowIdOwned),
}

enum LevelStorage {
    Occupied(Level),
    Vacant(Option<usize>),
}

enum WindowStorage {
    Occupied(LevelIndexOwned, Box<dyn IWindowBoth>),
    Vacant(Option<usize>),
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum WorldStorage {
    Active(&'static World),
    Inactive(&'static World),
    NotCreated,
}

#[cfg(test)]
thread_local! {
    pub(crate) static WORLD_PTR: Cell<WorldStorage> = const { Cell::new(WorldStorage::NotCreated) };
}

pub(crate) fn new_world(headless: bool) -> World {
    let world = World::new_empty();
    if headless {
        world.make_headless();
    }
    world
}

#[cfg(test)]
pub(crate) fn new_world_test(headless: bool) -> &'static World {
    match WORLD_PTR.get() {
        WorldStorage::Active(_) => {
            panic!("creating new world while one is already active on this thread")
        }
        WorldStorage::Inactive(w) => {
            if headless {
                w.make_headless();
            }

            WORLD_PTR.set(WorldStorage::Active(w));
            w
        }
        WorldStorage::NotCreated => {
            let world = World::new_empty();
            if headless {
                world.make_headless();
            }
            let world: &'static World = Box::leak(Box::new(world));

            WORLD_PTR.set(WorldStorage::Active(world));
            world
        }
    }
}

#[cfg(test)]
pub(crate) struct DropWorldPtr {}

#[cfg(test)]
impl Drop for DropWorldPtr {
    fn drop(&mut self) {
        if let WorldStorage::Active(w) = WORLD_PTR.get() {
            WORLD_PTR.set(WorldStorage::Inactive(w));
            w.clear();
        }
    }
}

#[cfg(test)]
/// Runs a
pub fn run_test<F: FnOnce() -> R, R>(f: F) -> R {
    let _ = new_world_test(true);
    let _world_guard = DropWorldPtr {};
    f()
}

/// The entire context of the engine, including Components.
pub struct World {
    levels: Box<boxcar::Vec<UnsafeCell<(u32, LevelStorage)>>>,
    level_free_head: Cell<Option<usize>>,

    action_queue: RefCell<VecDeque<WorldAction>>,

    stable_rate: Cell<Duration>,
    min_idle_delay: Cell<Duration>,

    windows: Box<boxcar::Vec<RefCell<(u32, WindowStorage)>>>,
    window_free_head: Cell<Option<usize>>,

    main_window: Cell<Option<WindowId>>,
    main_level: Cell<Option<LevelIndex>>,

    active_event_loop: Cell<*const ActiveEventLoop>,

    vk_ctx: OnceCell<Arc<VkContext>>,
}

impl World {
    pub(crate) fn new_empty() -> Self {
        Self {
            levels: Box::new(boxcar::Vec::new()),
            level_free_head: Cell::new(None),

            action_queue: RefCell::new(VecDeque::new()),

            stable_rate: Cell::new(Duration::from_millis(15)),
            min_idle_delay: Cell::new(Duration::from_millis(15)),

            windows: Box::new(boxcar::Vec::new()),
            window_free_head: Cell::new(None),
            main_window: Cell::new(None),
            main_level: Cell::new(None),

            active_event_loop: Cell::new(std::ptr::null()),

            vk_ctx: OnceCell::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn clear(&self) {
        // Reset levels
        for (i, level) in self.levels.iter() {
            let s = unsafe { level.get().as_mut_unchecked() };
            s.0 = 0;
            s.1 = LevelStorage::Vacant((i != self.levels.count() - 1).then_some(i + 1));
        }
        self.level_free_head
            .set((!self.levels.is_empty()).then_some(0));

        self.action_queue.borrow_mut().clear();

        self.stable_rate.set(Duration::ZERO);
        self.min_idle_delay.set(Duration::ZERO);

        // Reset windows
        for (i, w) in self.windows.iter() {
            let mut s = w.borrow_mut();
            s.0 = 0;
            s.1 = WindowStorage::Vacant((i != self.windows.count() - 1).then_some(i + 1));
        }
        self.window_free_head
            .set((!self.windows.is_empty()).then_some(0));

        self.main_window.take();
        self.main_level.take();

        self.active_event_loop.take();
    }

    pub(crate) fn make_headless(&self) {
        let lidxo = self.create_level();
        let lidx = lidxo.handle();
        lidxo.leak();

        self.main_level.set(Some(lidx));

        self.stable_rate.set(Duration::from_millis(15));
        self.min_idle_delay.set(Duration::from_millis(15));
    }
}

impl World {
    /// If the Vulkan context is uninitialized, tries to initialize it.
    ///
    /// This is done automatically if the main loop is run with `headless`d set
    /// to `false`.
    pub fn init_vk(&self, init_opts: super::vk::InitializationOptions) -> AResult<()> {
        if self.vk_ctx.get().is_none() {
            let vk_ctx = Arc::new(VkContext::new(init_opts)?);

            // No meaningful error
            let _ = self.vk_ctx.set(vk_ctx);
        }

        Ok(())
    }

    /// Returns the Vulkan context if it has been initialized.
    ///
    /// See [`init_vk`](Self::init_vk).
    pub fn get_vk(&self) -> Option<Arc<VkContext>> {
        self.vk_ctx.get().cloned()
    }
}

impl World {
    /// Returns the rate at which physics are run.
    pub fn get_stable_tick_rate(&self) -> Duration {
        self.stable_rate.get()
    }

    /// Sets the rate at which physics are run.
    pub fn set_stable_tick_rate(&self, rate: Duration) {
        self.stable_rate.set(rate);
    }

    /// Returns the minimum delay between idle hooks.
    pub fn get_idle_min_delay(&self) -> Duration {
        self.min_idle_delay.get()
    }

    /// Sets the minimum delay between idle hooks.
    pub fn set_idle_min_delay(&self, rate: Duration) {
        self.min_idle_delay.set(rate);
    }

    /// Processes all queued level/window deletions.
    ///
    /// # Safety
    ///
    /// Must be called when nothing else is using the [`World`].
    pub(crate) unsafe fn flush_actions(&self) {
        while let Some(action) = self.action_queue.borrow_mut().pop_front() {
            match action {
                WorldAction::DeleteLevel(level) => {
                    // # Safety
                    // We *know* there are no accessors to a dead level
                    let s = unsafe {
                        (&raw const *self
                            .levels
                            .get(level.0 as usize)
                            .expect("level index was given out"))
                            .cast_mut()
                            .as_mut_unchecked()
                    };

                    let s = unsafe { s.get().as_mut_unchecked() };

                    if let LevelStorage::Occupied(level) = &s.1 {
                        level.destroy_internal(self)
                    }

                    s.0 = s.0.wrapping_add(1);
                    s.1 = LevelStorage::Vacant(self.level_free_head.take());
                    self.level_free_head.set(Some(level.0 as usize));

                    level.leak();
                }
                WorldAction::DeleteWindow(id) => {
                    let (_window, level) = {
                        let mut storage = self
                            .windows
                            .get(id.0 as usize)
                            .expect("window index not valid")
                            .borrow_mut();

                        if id.1 != storage.0 {
                            continue;
                        }

                        let WindowStorage::Occupied(level, window) = std::mem::replace(
                            &mut storage.1,
                            WindowStorage::Vacant(self.window_free_head.get()),
                        ) else {
                            unreachable!()
                        };

                        storage.0 = storage.0.wrapping_add(1);
                        self.window_free_head.set(Some(id.0 as usize));

                        (window, level)
                    };

                    if self.main_window.get() == Some(id.handle()) {
                        self.main_window.set(None);
                        self.main_level.set(None);
                    }
                    id.leak();

                    self.action_queue
                        .borrow_mut()
                        .push_back(WorldAction::DeleteLevel(level));
                }
            }
        }
    }

    pub(crate) unsafe fn set_active_event_loop(&self, ael: &ActiveEventLoop) {
        self.active_event_loop.set(ael);
    }

    pub(crate) fn unset_active_event_loop(&self) {
        self.active_event_loop.take();
    }

    fn active_event_loop(&self) -> Option<&ActiveEventLoop> {
        if self.active_event_loop.get().is_null() {
            None
        } else {
            Some(unsafe { self.active_event_loop.get().as_ref_unchecked() })
        }
    }
}

impl World {
    /// Returns the ID of the main window, if one exists.
    pub fn main_window(&self) -> Option<WindowId> {
        self.main_window.get()
    }

    /// Returns whether the given window is the main window.
    pub fn is_main_window(&self, id: WindowId) -> bool {
        self.main_window.get() == Some(id)
    }

    /// Returns the [`Level`] owned by the main window.
    ///
    /// In headless mode, returns the main [`Level`] created by the engine.
    pub fn main_level(&self) -> Option<&Level> {
        self.get_level(self.main_level.get()?)
    }

    pub(crate) fn set_main_window(&self, id: WindowId, level: LevelIndex) {
        self.main_window.set(Some(id));
        self.main_level.set(Some(level));
    }
}

impl World {
    /// Create a new [`Level`], returning the special index to free it.
    pub fn create_level(&self) -> LevelIndexOwned {
        let i: u32 = self
            .levels
            .count()
            .try_into()
            .expect("too many levels allocated");

        if i == u32::MAX {
            panic!("too many levels allocated");
        }

        if let Some(head) = self.level_free_head.take() {
            // # Safety
            // We *know* there are no accessors to a dead level
            let s = unsafe {
                (&raw const *self.levels.get(head).expect("level is free"))
                    .cast_mut()
                    .as_mut_unchecked()
            };
            let s = unsafe { s.get().as_mut_unchecked() };

            let g = s.0;
            let LevelStorage::Vacant(new_head) = std::mem::replace(
                &mut s.1,
                LevelStorage::Occupied(Level::new(LevelIndex(head as u32, g))),
            ) else {
                unreachable!()
            };

            self.level_free_head.set(new_head);

            LevelIndexOwned(head as u32, s.0)
        } else {
            self.levels.push(UnsafeCell::new((
                0,
                LevelStorage::Occupied(Level::new(LevelIndex(i, 0))),
            )));
            LevelIndexOwned(i, 0)
        }
    }

    /// Returns a reference to a [`Level`].
    pub fn get_level(&self, level: LevelIndex) -> Option<&Level> {
        let (g, s) = unsafe { self.levels.get(level.0 as usize)?.get().as_ref_unchecked() };
        if *g == level.1
            && let LevelStorage::Occupied(l) = s
        {
            Some(l)
        } else {
            None
        }
    }

    /// Destroy a [`Level`] using its owning index.
    pub fn destroy_level(&self, level: LevelIndexOwned) {
        self.action_queue
            .borrow_mut()
            .push_back(WorldAction::DeleteLevel(level));
    }

    /// Returns an iterator over every level
    pub fn iter_levels(&self) -> impl Iterator<Item = &Level> {
        self.levels.iter().filter_map(|(_, s)| {
            let (_, l) = unsafe { s.get().as_ref_unchecked() };
            if let LevelStorage::Occupied(l) = &l {
                Some(l)
            } else {
                None
            }
        })
    }
}

impl World {
    /// Create a new [`RootWindow`] from the given attributes.
    ///
    /// This must be called from within the main loop, where an event loop is
    /// active.
    ///
    /// # Borrows
    ///
    /// Indirectly mutably borrows the destination window's storage slot.
    pub fn create_root_window(&self, attrs: WindowAttributes) -> AResult<WindowIdOwned> {
        let ael = self.active_event_loop().ok_or_else(|| {
            anyhow::anyhow!("no active event loop; create windows from within the main loop")
        })?;
        let window = ael
            .create_window(attrs)
            .map_err(|e| anyhow::anyhow!("failed to create window: {e}"))?;

        Ok(
            self.insert_window(self.create_level(), move |id, level, self_| {
                self_
                    .get_level(level)
                    .expect("level just created")
                    .set_window(id);
                Box::new(RootWindow::new(
                    id,
                    level,
                    window,
                    self.get_vk().expect("need Vulkan to create a window"),
                ))
            }),
        )
    }

    /// Inserts a built window into the [`World`].
    ///
    /// # Borrows
    ///
    /// Mutably borrows the destination window's storage slot.
    fn insert_window(
        &self,
        level: LevelIndexOwned,
        build: impl FnOnce(WindowId, LevelIndex, &Self) -> Box<dyn IWindowBoth>,
    ) -> WindowIdOwned {
        let handle = level.handle();

        if let Some(head) = self.window_free_head.take() {
            let mut storage = self.windows.get(head).expect("window is free").borrow_mut();
            let g = storage.0;
            let WindowStorage::Vacant(next) = std::mem::replace(
                &mut storage.1,
                WindowStorage::Occupied(level, build(WindowId(head as u32, g), handle, self)),
            ) else {
                unreachable!("free head should point to a vacant slot")
            };

            self.window_free_head.set(next);

            WindowIdOwned(head as u32, g)
        } else {
            let i: u32 = self
                .windows
                .count()
                .try_into()
                .expect("too many windows allocated");

            if i == u32::MAX {
                panic!("too many windows allocated");
            }

            let window = build(WindowId(i, 0), handle, self);
            self.windows
                .push(RefCell::new((0, WindowStorage::Occupied(level, window))));
            WindowIdOwned(i, 0)
        }
    }

    /// Returns an [`IWindow`] by its ID.
    ///
    /// # Borrows
    ///
    /// Immutably borrows the window's storage slot until the returned
    /// [`Ref`] is dropped.
    pub fn get_window(&self, id: WindowId) -> Option<Ref<'_, dyn IWindow>> {
        Ref::filter_map(
            self.windows.get(id.0 as usize)?.borrow(),
            |s| -> Option<&dyn IWindow> {
                if let WindowStorage::Occupied(_, window) = &s.1
                    && id.1 == s.0
                {
                    Some(window.as_ref())
                } else {
                    None
                }
            },
        )
        .ok()
    }

    /// Returns an [`IWindow`] by its ID, mutably.
    ///
    /// # Borrows
    ///
    /// Mutably borrows the window's storage slot until the returned
    /// [`RefMut`] is dropped.
    pub fn get_window_mut(&self, id: WindowId) -> Option<RefMut<'_, dyn IWindow>> {
        RefMut::filter_map(
            self.windows.get(id.0 as usize)?.borrow_mut(),
            |s| -> Option<&mut dyn IWindow> {
                if let WindowStorage::Occupied(_, window) = &mut s.1
                    && id.1 == s.0
                {
                    Some(window.as_mut())
                } else {
                    None
                }
            },
        )
        .ok()
    }

    /// Returns an [`IWindowBoth`] by its ID, mutably.
    ///
    /// # Borrows
    ///
    /// Mutably borrows the window's storage slot until the returned
    /// [`RefMut`] is dropped.
    pub(crate) fn get_window_mut_int(&self, id: WindowId) -> Option<RefMut<'_, dyn IWindowBoth>> {
        RefMut::filter_map(self.windows.get(id.0 as usize)?.borrow_mut(), |s| {
            if let WindowStorage::Occupied(_, window) = &mut s.1
                && id.1 == s.0
            {
                Some(window.as_mut())
            } else {
                None
            }
        })
        .ok()
    }

    /// Returns the [`WindowId`] of the window backed by the given OS window.
    ///
    /// # Borrows
    ///
    /// Transiently immutably borrows every window's storage slot.
    pub fn window_by_os_id(&self, os_id: winit::window::WindowId) -> Option<WindowId> {
        self.windows.iter().find_map(|(pidx, storage)| {
            let s = storage.borrow();
            if let WindowStorage::Occupied(_, window) = &s.1
                && window.as_os().map(|w| w.id()) == Some(os_id)
            {
                Some(WindowId(pidx as u32, s.0))
            } else {
                None
            }
        })
    }

    /// Forwards a window event to the window it belongs to.
    ///
    /// # Borrows
    ///
    /// Mutably borrows the target window's storage slot while handling the
    /// event.
    pub(crate) fn handle_window_event(&self, id: WindowId, event: &WindowEvent) {
        let Some(mut window) = self.get_window_mut_int(id) else {
            return;
        };
        window.on_input_event(self, event);
    }

    /// Returns an immutable iterator over every [`IWindow`].
    ///
    /// # Borrows
    ///
    /// Each yielded [`Ref`] immutably borrows its window's
    /// storage slot until dropped.
    pub fn iter_windows(&self) -> impl Iterator<Item = Ref<'_, dyn IWindow>> {
        self.windows.iter().filter_map(|(_, storage)| {
            Ref::filter_map(storage.borrow(), |s| -> Option<&dyn IWindow> {
                if let WindowStorage::Occupied(_, window) = &s.1 {
                    Some(window.as_ref())
                } else {
                    None
                }
            })
            .ok()
        })
    }

    /// Returns a mutable iterator over every [`IWindow`].
    ///
    /// # Borrows
    ///
    /// Each yielded [`RefMut`] mutably borrows its window's
    /// storage slot until dropped.
    pub fn iter_windows_mut(&self) -> impl Iterator<Item = RefMut<'_, dyn IWindow>> {
        self.windows.iter().filter_map(|(_, storage)| {
            RefMut::filter_map(storage.borrow_mut(), |s| -> Option<&mut dyn IWindow> {
                if let WindowStorage::Occupied(_, window) = &mut s.1 {
                    Some(window.as_mut())
                } else {
                    None
                }
            })
            .ok()
        })
    }

    /// Returns a mutable iterator over every window, as [`IWindowBoth`].
    ///
    /// # Borrows
    ///
    /// Each yielded [`RefMut`] mutably borrows its window's
    /// storage slot until dropped.
    pub(crate) fn iter_windows_mut_int(&self) -> impl Iterator<Item = RefMut<'_, dyn IWindowBoth>> {
        self.windows.iter().filter_map(|(_, storage)| {
            RefMut::filter_map(storage.borrow_mut(), |s| {
                if let WindowStorage::Occupied(_, window) = &mut s.1 {
                    Some(window.as_mut())
                } else {
                    None
                }
            })
            .ok()
        })
    }

    /// Returns an iterator over every [`WindowId`].
    ///
    /// # Borrows
    ///
    /// Transiently immutably borrows each window's storage slot.
    pub fn iter_window_ids(&self) -> impl Iterator<Item = WindowId> {
        self.windows.iter().filter_map(|(pidx, storage)| {
            let s = storage.borrow();
            if let WindowStorage::Occupied(_, _) = &s.1 {
                Some(WindowId(pidx as u32, s.0))
            } else {
                None
            }
        })
    }

    /// Destroy an [`IWindow`], along with its [`Level`].
    pub fn destroy_window(&self, id: WindowIdOwned) {
        self.action_queue
            .borrow_mut()
            .push_back(WorldAction::DeleteWindow(id));
    }
}
