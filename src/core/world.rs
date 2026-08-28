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

use crate::core::{
    level::{Level, LevelIndex, LevelIndexOwned},
    util::gen_slot_vec::{RefCellGenSlotVec, SlotIndex},
    vk::VkContext,
    window::{IWindowBoth, RootWindow, WindowId, WindowIdOwned},
};
use crate::core::{util::sentinel::SentinelMaxU32, window::IWindow};

enum WorldAction {
    DeleteLevel(LevelIndexOwned),
    DeleteWindow(WindowIdOwned),
}

enum LevelStorage {
    Occupied(Level),
    Vacant(SentinelMaxU32),
}

/// The entire context of the engine, including Components.
pub struct World {
    // UnsafeCell custom implementation because RefCellGenSlotVec would mean RefCell<RefCell<>>
    levels: Box<boxcar::Vec<UnsafeCell<(u32, LevelStorage)>>>,
    level_free_head: Cell<SentinelMaxU32>,

    action_queue: RefCell<VecDeque<WorldAction>>,

    stable_rate: Cell<Duration>,
    min_idle_delay: Cell<Duration>,

    windows: RefCellGenSlotVec<(LevelIndexOwned, Box<dyn IWindowBoth>)>,

    main_window: Cell<Option<WindowId>>,
    main_level: Cell<Option<LevelIndex>>,

    active_event_loop: Cell<*const ActiveEventLoop>,

    vk_ctx: OnceCell<Arc<VkContext>>,
}

impl World {
    pub(crate) fn new_empty() -> Self {
        Self {
            levels: Box::new(boxcar::Vec::new()),
            level_free_head: Cell::new(SentinelMaxU32::NONE),

            action_queue: RefCell::new(VecDeque::new()),

            stable_rate: Cell::new(Duration::from_millis(15)),
            min_idle_delay: Cell::new(Duration::from_millis(15)),

            windows: RefCellGenSlotVec::new(),

            main_window: Cell::new(None),
            main_level: Cell::new(None),

            active_event_loop: Cell::new(std::ptr::null()),

            vk_ctx: OnceCell::new(),
        }
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
                    self.level_free_head.set(SentinelMaxU32::from_some(level.0));

                    level.leak();
                }
                WorldAction::DeleteWindow(id) => {
                    let (window, level) = {
                        let taken = self.windows.take(id.slot).expect("window index not valid");

                        let Some((level, window)) = taken else {
                            continue;
                        };

                        (window, level)
                    };

                    if self.main_window.get() == Some(id.handle()) {
                        self.main_window.set(None);
                        self.main_level.set(None);
                    }
                    drop(window);
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
    ///
    /// Note, the `Level` will be inactive, so make sure to call
    /// [`Level::set_active`].
    pub fn create_level(&self) -> LevelIndexOwned {
        let i: u32 = self
            .levels
            .count()
            .try_into()
            .expect("too many levels allocated");

        if i == u32::MAX {
            panic!("too many levels allocated");
        }

        let head = self.level_free_head.take();

        if head.is_some() {
            let head = head.into_inner();

            // # Safety
            // We *know* there are no accessors to a dead level
            let s = unsafe {
                (&raw const *self.levels.get(head as usize).expect("level is free"))
                    .cast_mut()
                    .as_mut_unchecked()
            };
            let s = unsafe { s.get().as_mut_unchecked() };

            let g = s.0;
            let LevelStorage::Vacant(new_head) = std::mem::replace(
                &mut s.1,
                LevelStorage::Occupied(Level::new(LevelIndex(head, g))),
            ) else {
                unreachable!()
            };

            self.level_free_head.set(new_head);

            LevelIndexOwned(head, s.0)
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
        let slot = self.windows.reserve();
        let id = WindowId { slot };
        let window = build(id, handle, self);
        self.windows.fill(slot, (level, window));
        WindowIdOwned { slot }
    }

    /// Returns an [`IWindow`] by its ID.
    ///
    /// # Borrows
    ///
    /// Immutably borrows the window's storage slot until the returned
    /// [`Ref`] is dropped.
    pub fn get_window(&self, id: WindowId) -> Option<Ref<'_, dyn IWindow>> {
        let slot_ref = self.windows.acquire(id.slot).ok()?;
        Ref::filter_map(slot_ref, |(_, window)| -> Option<&dyn IWindow> {
            Some(window.as_ref())
        })
        .ok()
    }

    /// Returns an [`IWindow`] by its ID, mutably.
    ///
    /// # Borrows
    ///
    /// Mutably borrows the window's storage slot until the returned
    /// [`RefMut`] is dropped.
    pub fn get_window_mut(&self, id: WindowId) -> Option<RefMut<'_, dyn IWindow>> {
        let slot_ref = self.windows.acquire_mut(id.slot).ok()?;
        RefMut::filter_map(slot_ref, |(_, window)| -> Option<&mut dyn IWindow> {
            Some(window.as_mut())
        })
        .ok()
    }

    /// Returns an [`IWindowBoth`] by its ID, mutably.
    ///
    /// # Borrows
    ///
    /// Mutably borrows the window's storage slot until the returned
    /// [`RefMut`] is dropped.
    pub(crate) fn get_window_mut_int(&self, id: WindowId) -> Option<RefMut<'_, dyn IWindowBoth>> {
        let slot_ref = self.windows.acquire_mut(id.slot).ok()?;
        RefMut::filter_map(slot_ref, |(_, window)| -> Option<&mut dyn IWindowBoth> {
            Some(window.as_mut())
        })
        .ok()
    }

    /// Returns the [`WindowId`] of the window backed by the given OS window.
    ///
    /// # Borrows
    ///
    /// Transiently immutably borrows every window's storage slot.
    pub fn window_by_os_id(&self, os_id: winit::window::WindowId) -> Option<WindowId> {
        self.windows.ids().find_map(|slot| {
            let s = self.windows.acquire(slot).ok()?;
            if s.1.as_os().map(|w| w.id()) == Some(os_id) {
                Some(WindowId { slot })
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

    /// Returns an immutable iterator over every [`IWindow`], paired with its
    /// [`WindowId`].
    ///
    /// # Borrows
    ///
    /// Each yielded pair immutably borrows its window's storage slot
    /// until the pair is dropped.
    pub fn iter_windows(&self) -> impl Iterator<Item = (WindowId, Ref<'_, dyn IWindow>)> {
        self.windows.ids().filter_map(move |slot| {
            let s = self.windows.acquire(slot).ok()?;
            let id = WindowId { slot };
            let window_ref = Ref::map(s, |(_, w)| -> &dyn IWindow { w.as_ref() });
            Some((id, window_ref))
        })
    }

    /// Returns a mutable iterator over every [`IWindow`], paired with its
    /// [`WindowId`].
    ///
    /// # Borrows
    ///
    /// Each yielded pair mutably borrows its window's storage slot
    /// until the pair is dropped.
    pub fn iter_windows_mut(&self) -> impl Iterator<Item = (WindowId, RefMut<'_, dyn IWindow>)> {
        let ids: Vec<SlotIndex> = self.windows.ids().collect();
        ids.into_iter().filter_map(move |slot| {
            let s = self.windows.acquire_mut(slot).ok()?;
            let id = WindowId { slot };
            let window_ref = RefMut::map(s, |(_, w)| -> &mut dyn IWindow { w.as_mut() });
            Some((id, window_ref))
        })
    }

    /// Returns a mutable iterator over every window, as [`IWindowBoth`], paired
    /// with its [`WindowId`].
    ///
    /// # Borrows
    ///
    /// Each yielded pair mutably borrows its window's storage slot
    /// until the pair is dropped.
    pub(crate) fn iter_windows_mut_int(
        &self,
    ) -> impl Iterator<Item = (WindowId, RefMut<'_, dyn IWindowBoth>)> {
        let ids: Vec<SlotIndex> = self.windows.ids().collect();
        ids.into_iter().filter_map(move |slot| {
            let s = self.windows.acquire_mut(slot).ok()?;
            let id = WindowId { slot };
            let window_ref = RefMut::map(s, |(_, w)| -> &mut dyn IWindowBoth { w.as_mut() });
            Some((id, window_ref))
        })
    }

    /// Attempts a non-blocking swapchain acquisition on every window.
    ///
    /// Returns `true` if there are no windows, or if at least one window staged a swapchain
    /// image. This is used to decide whether idle/rendering work may proceed: if windows exist
    /// but none has an image available, running idle would only build frames that cannot be
    /// presented yet.
    ///
    /// # Borrows
    ///
    /// Mutably borrows each window's storage slot transiently.
    pub(crate) fn try_acquire_any_swapchain(&self) -> bool {
        if self.windows.is_empty() {
            return true;
        }

        // Short circuit is ok because windows trying twice should be a no-op,
        // and they should try during draw.
        self.iter_windows_mut_int()
            .any(|(_id, mut w)| w.try_acquire_swapchain())
    }

    /// Returns an iterator over every [`WindowId`].
    ///
    /// # Borrows
    ///
    /// Transiently immutably borrows each window's storage slot.
    pub fn iter_window_ids(&self) -> impl Iterator<Item = WindowId> {
        self.windows.ids().map(|slot| WindowId { slot })
    }

    /// Destroy an [`IWindow`], along with its [`Level`].
    pub fn destroy_window(&self, id: WindowIdOwned) {
        self.action_queue
            .borrow_mut()
            .push_back(WorldAction::DeleteWindow(id));
    }

    /// Switches the [`Level`] bound to a given [`IWindow`].
    ///
    /// If the window is not found, returns `Err` with the given [`LevelIndexOwned`]. If the window
    /// is found, returns the `LevelIndexOwned` of the replaced `Level`.
    ///
    /// # Borrows
    ///
    /// Mutably borrows the given window, and immutably borrows the given
    /// `Level` and the `Level` previously owned by the window.
    pub fn switch_window_level(
        &self,
        window: WindowId,
        level: LevelIndexOwned,
    ) -> Result<LevelIndexOwned, LevelIndexOwned> {
        let new_handle = level.handle();

        let old_level = {
            let mut slot = match self.windows.acquire_mut(window.slot) {
                Ok(slot) => slot,
                Err(_) => return Err(level),
            };

            slot.1.switch_level(new_handle);

            self.get_level(new_handle)
                .expect("we have the owning handle")
                .set_window(window);

            std::mem::replace(&mut slot.0, level)
        };

        self.get_level(old_level.handle())
            .expect("we have the owning handle")
            .unset_window();

        Ok(old_level)
    }
}
