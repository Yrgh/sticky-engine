//! Window management.
//!
//! A window is any surface the engine renders to. Each window owns exactly one
//! [`Level`](crate::core::level::Level), and each window is one of a few kinds:
//!
//! - [`RootWindow`] - a real, on-screen window created by the OS. Receives
//!   input.
//!
//! - [`ViewportWindow`] - an off-screen window that renders to a texture and
//!   receives no input. Its contents can be composited onto another window.
//!
//! - Virtual windows - windows with no OS presence, used when the platform has
//!   no multi-window support or when explicitly requested. These are not yet
//!   implemented, but can be added seamlessly by implementing [`IWindow`] and
//!   registering it with [`World::create_window`].
//!
//! Windows are owned by the [`World`]. Create them with
//! [`World::create_window`], [`World::create_root_window`], or
//! [`World::create_viewport_window`], and destroy them with
//! [`World::destroy_window`].

use std::{any::Any, cell::Cell, sync::Arc};

use vulkano::{
    image::Image,
    swapchain::{Surface, Swapchain},
};
use winit::{
    dpi::PhysicalSize,
    event::WindowEvent,
    window::{Window as OsWindow, WindowId as OsWindowId},
};

use crate::{
    core::{level::LevelIndex, vk::VkContext, world::World}, log,
};

/// Non-owning handle to a [`Window`] within the [`World`].
///
/// This handle is lightweight and cheap to copy. It does **not** keep the
/// window alive; use [`WindowIdOwned`] for that.
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct WindowId(pub(crate) u32, pub(crate) u32);

/// Singly-owning handle to a [`Window`].
///
/// The window lives until it is explicitly destroyed with
/// [`World::destroy_window`]. Dropping this handle without destroying or
/// leaking the window logs an error. Call [`leak`](Self::leak) to keep the
/// window alive until the [`World`] is dropped.
pub struct WindowIdOwned(pub(crate) u32, pub(crate) u32);

impl WindowIdOwned {
    /// Returns a non-owning copy of this handle.
    pub fn handle(&self) -> WindowId {
        WindowId(self.0, self.1)
    }

    /// Prevents this handle from destroying the window when dropped.
    ///
    /// The window will live until the [`World`] is dropped.
    pub fn leak(mut self) {
        self.0 = u32::MAX;
        self.1 = u32::MAX;
    }
}

impl Drop for WindowIdOwned {
    fn drop(&mut self) {
        if self.0 != u32::MAX && self.1 != u32::MAX {
            log!(err: "Leaked window {:?}", self.handle())
        }
    }
}

/// Base trait for all windows.
///
/// Windows come in several flavors, each owning exactly one
/// [`Level`](crate::core::level::Level). To add a new kind of window, such as
/// a virtual window, implement this trait and register it with
/// [`World::create_window`].
pub trait IWindow: Any {
    /// Returns the ID of this window.
    fn id(&self) -> WindowId;

    /// Returns the [`LevelIndex`] of the [`Level`] this window owns.
    fn level(&self) -> LevelIndex;

    /// Returns whether this window receives OS input events.
    ///
    /// [`RootWindow`]s receive input; off-screen windows do not.
    fn receives_input(&self) -> bool {
        false
    }

    /// Returns the OS window backing this window, if any.
    fn os_id(&self) -> Option<OsWindowId> {
        None
    }

    /// Called for each OS event targeting this window, if
    /// [`receives_input`](Self::receives_input) is `true`.
    fn on_input_event(&mut self, _world: &World, _event: &WindowEvent) {}

    /// Called when the size of this window's render target changes.
    fn on_resize(&mut self, _world: &World, _size: PhysicalSize<u32>) {}

    /// Returns `self` as `&dyn Any`, for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns `self` as `&mut dyn Any`, for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Called when
    /// [`ApplicationHandler::suspended`](winit::application::ApplicationHandler::suspended)
    /// is received.
    fn suspend(&mut self) {}

    /// Called when
    /// [`ApplicationHandler::resumed`](winit::application::ApplicationHandler::resumed)
    /// is received.
    fn resume(&mut self) {}

    /// Called before the renderer begins to put things on the screen.
    fn before_draw(&mut self) {}
}

pub(crate) struct RootWindowSurface {
    pub(crate) surface: Arc<Surface>,
    pub(crate) swapchain: Arc<Swapchain>,
    pub(crate) swapchain_images: Vec<Arc<Image>>,
}

/// A real, on-screen window backed by the OS.
///
/// This is the standard window type. It receives input events from the OS.
pub struct RootWindow {
    id: WindowId,
    level: LevelIndex,
    window: Arc<OsWindow>,
    size: Cell<PhysicalSize<u32>>,
    swapchain_invalid: Cell<bool>,
    vk_ctx: Arc<VkContext>,
    surface: Option<RootWindowSurface>,
}

impl RootWindow {
    /// Creates a new root window.
    ///
    /// The `id` is assigned by the [`World`], and `level` is the [`Level`]
    /// this window owns.
    pub fn new(id: WindowId, level: LevelIndex, window: OsWindow, vk_ctx: Arc<VkContext>) -> Self {
        let size = window.inner_size();
        Self {
            id,
            level,
            window: Arc::new(window),
            size: Cell::new(size),
            swapchain_invalid: Cell::new(true),
            vk_ctx,
            surface: None,
        }
    }

    /// Returns the OS window backing this root window.
    pub fn window(&self) -> &OsWindow {
        &self.window
    }

    /// Returns the current size of this window.
    pub fn size(&self) -> PhysicalSize<u32> {
        self.size.get()
    }
}

impl IWindow for RootWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn level(&self) -> LevelIndex {
        self.level
    }

    fn receives_input(&self) -> bool {
        true
    }

    fn os_id(&self) -> Option<OsWindowId> {
        Some(self.window.id())
    }

    fn on_resize(&mut self, _world: &World, size: PhysicalSize<u32>) {
        self.size.set(size);
        self.swapchain_invalid.set(true);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn suspend(&mut self) {
        self.surface = None;
    }

    fn resume(&mut self) {
        let surface = Surface::from_window(self.vk_ctx.instance.clone(), self.window.clone())
            .expect("failed to create surface");

        let (swapchain, swapchain_images) = self
            .vk_ctx
            .create_swapchain(surface.clone(), self.size.get())
            .expect("failed to create swapchain");

        self.surface = Some(RootWindowSurface {
            surface,
            swapchain,
            swapchain_images,
        });
    }

    fn before_draw(&mut self) {
        if let Some(surface) = &mut self.surface && self.swapchain_invalid.take() {
            let (swapchain, swapchain_images) = self
                .vk_ctx
                .create_swapchain(surface.surface.clone(), self.size.get())
                .expect("failed to create swapchain");

            surface.swapchain = swapchain;
            surface.swapchain_images = swapchain_images;
        }
    }
}

/// An off-screen window that renders to a texture.
///
/// Viewport windows are not backed by an OS window and never receive input.
/// Their contents can be composited onto other windows' surfaces. The render
/// target texture itself is created by the renderer (not yet implemented).
pub struct ViewportWindow {
    id: WindowId,
    level: LevelIndex,
    size: Cell<PhysicalSize<u32>>,
    size_changed: Cell<bool>,
}

impl ViewportWindow {
    /// Creates a new viewport window with the given initial size.
    ///
    /// The `id` is assigned by the [`World`], and `level` is the [`Level`]
    /// this window owns.
    pub fn new(id: WindowId, level: LevelIndex, size: PhysicalSize<u32>) -> Self {
        Self {
            id,
            level,
            size: Cell::new(size),
            size_changed: Cell::new(true)
        }
    }

    /// Returns the size of the render target.
    pub fn size(&self) -> PhysicalSize<u32> {
        self.size.get()
    }
}

impl IWindow for ViewportWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn level(&self) -> LevelIndex {
        self.level
    }

    fn on_resize(&mut self, _world: &World, size: PhysicalSize<u32>) {
        self.size.set(size);
        self.size_changed.set(true);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn before_draw(&mut self) {
        // TODO
        let _ = self.size_changed.take();
    }
}
