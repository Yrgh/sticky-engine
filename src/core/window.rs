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

use smallvec::SmallVec;
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
    core::{
        level::LevelIndex, renderer::PrimaryRenderingQueue, task::spawn, vk::VkContext,
        world::World,
    },
    log,
};

mod private {
    #[doc(hidden)]
    pub trait Sealed {}
}

pub use private::Sealed;

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

    fn set_prq(&mut self, prq: PrimaryRenderingQueue);

    fn draw(&mut self);
}

/// Base trait for all windows.
///
/// Windows come in several flavors, each owning exactly one
/// [`Level`](crate::core::level::Level). To add a new kind of window, such as
/// a virtual window, implement this trait and register it with
/// [`World::create_window`].
pub trait IWindow: Sealed {
    /// Returns the ID of this window.
    fn id(&self) -> WindowId;

    /// Returns the [`LevelIndex`] of the [`Level`] this window owns.
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

    /// Returns the [`LevelIndex`] of the [`Level`] this window owns.
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

pub(crate) struct RootWindowSurface {
    pub(crate) surface: Arc<Surface>,
    pub(crate) swapchain: Arc<Swapchain>,
    pub(crate) swapchain_images: SmallVec<[Arc<Image>; 3]>,
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
    prq: Option<PrimaryRenderingQueue>,
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
            prq: None,
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
        self.size.set(size);
        self.swapchain_invalid.set(true);
    }

    fn as_any_i(&self) -> &dyn Any {
        self
    }

    fn as_any_mut_i(&mut self) -> &mut dyn Any {
        self
    }

    fn suspend(&mut self) {
        self.surface = None;
        log!(dbg: "suspended");
    }

    fn resume(&mut self) {
        log!(dbg: "resuming... (size: {:?})", self.size.get());
        
        let surface = Surface::from_window(self.vk_ctx.instance.clone(), self.window.clone())
            .expect("failed to create surface");

        let (swapchain, swapchain_images) = self
            .vk_ctx
            .create_swapchain(surface.clone(), self.size.get())
            .expect("failed to create swapchain");

        self.surface = Some(RootWindowSurface {
            surface,
            swapchain,
            swapchain_images: swapchain_images.into(),
        });

        log!(dbg: "resumed");
    }

    fn draw(&mut self) {
        let Some(surface) = &mut self.surface else {
            log!(wrn: "no surface during draw");
            return;
        };

        if self.swapchain_invalid.take() {
            log!(dbg: "recreating swapchain... (size: {:?})", self.size.get());
            
            let (swapchain, swapchain_images) = self
                .vk_ctx
                .recreate_swapchain(surface.swapchain.clone(), self.size.get())
                .expect("failed to create swapchain");

            surface.swapchain = swapchain;
            surface.swapchain_images = swapchain_images.into();
            log!(dbg: "swapchain created...");
        }

        if let Some(prq) = &self.prq {
            let fence = prq
                .build_root(self.vk_ctx.clone(), surface, &self.window)
                .expect("failed to execute PRQ");
            spawn(fence);
        }
    }

    fn set_prq(&mut self, prq: PrimaryRenderingQueue) {
        self.prq = Some(prq);
    }
}
