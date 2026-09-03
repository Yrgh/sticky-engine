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

use thiserror::Error;
use winit::{dpi::PhysicalSize, window::Window as OsWindow};

use crate::core::{
    gpu_api::{BoxedInstructions, IRenderer, ISurface}, level::LevelId, math::Vec2, util::gen_slot_vec::SlotIndex, world::World,
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

#[derive(Debug, Error)]
pub(crate) enum DrawError {
    #[error("no instructions set")]
    NoInstructions,
    #[error("no surface")]
    NoSurface,
    #[error("no swapchain image acquired")]
    NoSwapImage,
    #[error("render error: {0}")]
    RenderError(anyhow::Error),
}

pub(crate) trait IWindowInt: Any {
    fn id_i(&self) -> WindowId;

    fn level_i(&self) -> LevelId;

    fn as_os_i(&self) -> Option<&OsWindow> {
        None
    }

    fn on_resize(&mut self, _world: &World, _size: PhysicalSize<u32>);

    fn as_any_i(&self) -> &dyn Any;

    fn as_any_mut_i(&mut self) -> &mut dyn Any;

    fn suspend(&mut self);

    fn resume(&mut self);

    fn set_instructions(&mut self, instructions: BoxedInstructions);

    fn get_cursor_mut(&mut self) -> &mut Option<Vec2>;

    /// Attempts a non-blocking acquisition of the next swapchain image.
    ///
    /// Returns `true` if an image is available (or was already acquired, in
    /// which case this is a no-op) and is now staged for the next
    /// [`draw`](Self::draw). Returns `false` if no image is currently available
    /// and none was staged; in that case no primary rendering should happen.
    fn try_acquire_swapchain(&mut self) -> bool;

    fn draw(&mut self) -> Result<(), DrawError>;

    fn switch_level(&mut self, level: LevelId);
}

/// Base trait for all windows.
///
/// Windows come in several flavors, each owning exactly one
/// [`Level`](crate::core::level::Level).
pub trait IWindow: Sealed {
    /// Returns the ID of this window.
    fn id(&self) -> WindowId;

    /// Returns the [`LevelId`] of the [`Level`](crate::core::level::Level)
    /// this window owns.
    fn level(&self) -> LevelId;

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

    /// Returns the [`LevelId`] of the [`Level`](crate::core::level::Level) this window owns.
    fn level(&self) -> LevelId {
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
    level: LevelId,
    // None = not on window
    cursor: Option<Vec2>,
    size: PhysicalSize<u32>,
    renderer: Rc<dyn IRenderer>,
    surface: Option<Box<dyn ISurface>>,
    instructions: Option<BoxedInstructions>,
    // Declared last so it is dropped last.
    window: Arc<OsWindow>,
}

impl RootWindow {
    /// Creates a new root window.
    ///
    /// The `id` is assigned by the [`World`], and `level` is the
    /// [`Level`](crate::core::level::Level) this window owns.
    pub fn new(
        id: WindowId,
        level: LevelId,
        window: OsWindow,
        renderer: Rc<dyn IRenderer>,
    ) -> Self {
        let size = window.inner_size();
        Self {
            id,
            level,
            size,
            cursor: None,
            renderer,
            surface: None,
            instructions: None,
            window: Arc::new(window),
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

    fn level_i(&self) -> LevelId {
        self.level
    }

    fn as_os_i(&self) -> Option<&OsWindow> {
        Some(&self.window)
    }

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

    fn draw(&mut self) -> Result<(), DrawError> {
        // Try to acquire again, since this is no-op if we already have acquired a valid swapchain.
        if !self.try_acquire_swapchain() {
            return Err(DrawError::NoSwapImage);
        }

        let Some(surface) = &self.surface else {
            return Err(DrawError::NoSurface);
        };

        if let Some(instructions) = &self.instructions {
            instructions
                .render(surface.as_ref())
                .map_err(DrawError::RenderError)?;
        } else {
            return Err(DrawError::NoInstructions);
        }

        Ok(())
    }

    fn set_instructions(&mut self, instructions: BoxedInstructions) {
        self.instructions = Some(instructions);
    }

    fn get_cursor_mut(&mut self) -> &mut Option<Vec2> {
        &mut self.cursor
    }

    fn switch_level(&mut self, level: LevelId) {
        self.level = level;
    }
}
