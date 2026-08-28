//! Window management.
//!
//! A window is any surface the engine renders to. Each window owns exactly one
//! [`Level`](crate::core::level::Level), and each window is one of a few kinds:
//!
//! - [`RootWindow`] - a real, on-screen window created by the OS. Receives
//!   input.
//!
//! Windows are owned by the [`World`]. Create them with
//! [`World::create_root_window`], and destroy them with
//! [`World::destroy_window`].

use std::{any::Any, rc::Rc, sync::Arc};

use anyhow::Result as AResult;

use winit::{dpi::PhysicalSize, event::WindowEvent, window::Window as OsWindow};

use crate::core::{
    gpu_api::{IRenderer, ISurface, WindowInstructions},
    level::LevelIndex,
    util::gen_slot_vec::SlotIndex,
    world::World,
};

mod private {
    #[doc(hidden)]
    pub trait Sealed {}
}

pub use private::Sealed;

/// Non-owning handle to an [`IWindow`] within the [`World`].
///
/// This handle is lightweight and cheap to copy. It does **not** keep the
/// window alive; use [`WindowIdOwned`] for that.
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct WindowId {
    pub(crate) slot: SlotIndex,
}

/// Singly-owning handle to an [`IWindow`].
///
/// The window lives until it is explicitly destroyed with
/// [`World::destroy_window`]. Dropping this handle without destroying or
/// leaking the window logs an error. Call [`leak`](Self::leak) to keep the
/// window alive until the [`World`] is dropped.
pub struct WindowIdOwned {
    pub(crate) slot: SlotIndex,
}

impl WindowIdOwned {
    /// Returns a non-owning copy of this handle.
    pub fn handle(&self) -> WindowId {
        WindowId { slot: self.slot }
    }

    /// Prevents this handle from destroying the window when dropped.
    ///
    /// The window will live until the [`World`] is dropped.
    pub fn leak(mut self) {
        self.slot = SlotIndex::invalid();
    }
}

impl Drop for WindowIdOwned {
    fn drop(&mut self) {
        if self.slot != SlotIndex::invalid() {
            tracing::warn!(
                handle.slot = ?self.slot,
                "a WindowIdOwned was dropped without manually being leaked, preventing the Window \
                from being removed until the World is destroyed"
            );
        }
    }
}

pub(crate) trait IWindowInt: Any {
    fn id_i(&self) -> WindowId;

    fn level_i(&self) -> LevelIndex;

    fn as_os_i(&self) -> Option<&OsWindow> {
        None
    }

    fn on_input_event(&mut self, _world: &World, _event: &WindowEvent);

    fn on_resize(&mut self, _world: &World, _size: PhysicalSize<u32>);

    fn as_any_i(&self) -> &dyn Any;

    fn as_any_mut_i(&mut self) -> &mut dyn Any;

    fn suspend(&mut self);

    fn resume(&mut self);

    #[expect(unused)]
    fn set_instructions(&mut self, instructions: WindowInstructions);

    /// Attempts a non-blocking acquisition of the next swapchain image.
    ///
    /// Returns `true` if an image is available (or was already acquired, in
    /// which case this is a no-op) and is now staged for the next
    /// [`draw`](Self::draw). Returns `false` if no image is currently available
    /// and none was staged; in that case no primary rendering should happen.
    fn try_acquire_swapchain(&mut self) -> bool;

    fn draw(&mut self) -> AResult<()>;

    fn switch_level(&mut self, level: LevelIndex);
}

/// Base trait for all windows.
///
/// Windows come in several flavors, each owning exactly one
/// [`Level`](crate::core::level::Level).
pub trait IWindow: Sealed {
    /// Returns the ID of this window.
    fn id(&self) -> WindowId;

    /// Returns the [`LevelIndex`] of the [`Level`](crate::core::level::Level)
    /// this window owns.
    fn level(&self) -> LevelIndex;

    /// Returns the OS window backing this window, if any.
    fn as_os(&self) -> Option<&OsWindow>;

    /// Returns `self` as `&dyn Any`, for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns `self` as `&mut dyn Any`, for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub(crate) trait IWindowBoth: IWindow + IWindowInt {}

impl<T: IWindowInt> Sealed for T {}

impl<T: IWindowInt> IWindowBoth for T {}

impl<T: IWindowInt> IWindow for T {
    /// Returns the ID of this window.
    fn id(&self) -> WindowId {
        <Self as IWindowInt>::id_i(self)
    }

    /// Returns the [`LevelIndex`] of the [`Level`](crate::core::level::Level) this window owns.
    fn level(&self) -> LevelIndex {
        <Self as IWindowInt>::level_i(self)
    }

    /// Returns the OS window backing this window, if any.
    fn as_os(&self) -> Option<&OsWindow> {
        <Self as IWindowInt>::as_os_i(self)
    }

    /// Returns `self` as `&dyn Any`, for downcasting.
    fn as_any(&self) -> &dyn Any {
        <Self as IWindowInt>::as_any_i(self)
    }

    /// Returns `self` as `&mut dyn Any`, for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any {
        <Self as IWindowInt>::as_any_mut_i(self)
    }
}

/// A real, on-screen window backed by the OS.
///
/// This is the standard window type. It receives input events from the OS.
pub struct RootWindow {
    id: WindowId,
    level: LevelIndex,
    window: Arc<OsWindow>,
    size: PhysicalSize<u32>,
    renderer: Rc<dyn IRenderer>,
    surface: Option<Box<dyn ISurface>>,
    instructions: Option<WindowInstructions>,
}

impl RootWindow {
    /// Creates a new root window.
    ///
    /// The `id` is assigned by the [`World`], and `level` is the
    /// [`Level`](crate::core::level::Level) this window owns.
    pub fn new(
        id: WindowId,
        level: LevelIndex,
        window: OsWindow,
        renderer: Rc<dyn IRenderer>,
    ) -> Self {
        let size = window.inner_size();
        Self {
            id,
            level,
            window: Arc::new(window),
            size,
            renderer,
            surface: None,
            instructions: None,
        }
    }

    /// Returns the OS window backing this root window.
    pub fn window(&self) -> &OsWindow {
        &self.window
    }

    /// Returns the current size of this window.
    pub fn size(&self) -> PhysicalSize<u32> {
        self.size
    }
}

impl IWindowInt for RootWindow {
    fn id_i(&self) -> WindowId {
        self.id
    }

    fn level_i(&self) -> LevelIndex {
        self.level
    }

    fn as_os_i(&self) -> Option<&OsWindow> {
        Some(&self.window)
    }

    fn on_input_event(&mut self, _world: &World, _event: &WindowEvent) {}

    fn on_resize(&mut self, _world: &World, size: PhysicalSize<u32>) {
        self.size = size;
        if let Some(surface) = &mut self.surface {
            surface.on_resize(size);
        }
    }

    fn as_any_i(&self) -> &dyn Any {
        self
    }

    fn as_any_mut_i(&mut self) -> &mut dyn Any {
        self
    }

    fn suspend(&mut self) {
        self.surface = None;
    }

    fn resume(&mut self) {
        self.surface = match self.renderer.create_surface(&self.window, self.size) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::error!(window_id = ?self.id, "failed to create surface: {e}");
                return;
            }
        };
    }

    fn try_acquire_swapchain(&mut self) -> bool {
        self.surface.as_mut().is_none_or(|s| s.try_acquire())
    }

    fn draw(&mut self) -> AResult<()> {
        // Try to acquire again, since this is no-op if we already have acquired a valid swapchain.
        if !self.try_acquire_swapchain() {
            return Ok(());
        }

        let Some(surface) = &self.surface else {
            return Ok(());
        };

        if let Some(instructions) = &self.instructions {
            instructions.render(surface.as_ref())?;
        }

        Ok(())
    }

    fn set_instructions(&mut self, instructions: WindowInstructions) {
        self.instructions = Some(instructions);
    }

    fn switch_level(&mut self, level: LevelIndex) {
        self.level = level;
    }
}
