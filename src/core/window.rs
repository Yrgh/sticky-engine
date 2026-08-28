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

use std::{any::Any, cell::Cell, sync::Arc, time::Duration};

use smallvec::SmallVec;
use vulkano::{
    image::Image,
    swapchain::{Surface, Swapchain, SwapchainAcquireFuture, acquire_next_image},
};
use winit::{dpi::PhysicalSize, event::WindowEvent, window::Window as OsWindow};

use crate::core::{
    level::LevelIndex,
    renderer::{FinalPresentFuture, PrimaryRenderingQueue},
    util::gen_slot_vec::SlotIndex,
    vk::VkContext,
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

    fn set_prq(&mut self, prq: PrimaryRenderingQueue);

    /// Attempts a non-blocking acquisition of the next swapchain image.
    ///
    /// Returns `true` if an image is available (or was already acquired, in
    /// which case this is a no-op) and is now staged for the next
    /// [`draw`](Self::draw). Returns `false` if no image is currently available
    /// and none was staged; in that case no primary rendering should happen.
    fn try_acquire_swapchain(&mut self) -> bool;

    /// Signals that 
    fn draw(&mut self);

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

pub(crate) struct RootWindowSurface {
    #[expect(unused)]
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

    /// A staged swapchain acquisition: the image index and the future marking when the image
    /// is available. Only one frame is in flight per window, so at most one acquisition is
    /// staged at a time.
    acquired: Option<AcquiredSlot>,
    /// Per swapchain image index, the most recent present fence for that image.
    ///
    /// The next frame rendered to the same image joins onto its entry so it never renders into
    /// an image while the previous present on that image is still in flight. When the swapchain
    /// changes, these are handed off to the shared [`VkContext`] in-flight list so they are
    /// cleaned up rather than dropped un-cleaned.
    last_in_flight: Vec<Option<Arc<FinalPresentFuture>>>,
}

/// A swapchain image acquired non-blockingly, staged for presentation.
struct AcquiredSlot {
    image_index: u32,
    swap_acq_fut: SwapchainAcquireFuture,
}

impl RootWindow {
    /// Creates a new root window.
    ///
    /// The `id` is assigned by the [`World`], and `level` is the
    /// [`Level`](crate::core::level::Level) this window owns.
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
            acquired: None,
            last_in_flight: Vec::new(),
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
        self.acquired = None;
        // The swapchain is going away; hand the remaining per-image fences off to the shared
        // in-flight list so they are cleaned up once finished, rather than dropped.
        self.flush_last_in_flight();
    }

    fn resume(&mut self) {
        // A fresh swapchain is created, so any per-image fences from the previous one are pushed
        // to the shared in-flight list and forgotten here.
        self.flush_last_in_flight();

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
    }

    fn try_acquire_swapchain(&mut self) -> bool {
        if self.acquired.is_some() {
            return true;
        }

        // The window will never render while the surface is `None`, so as a
        // precaution, say it will always be ready to "render".
        if self.surface.is_none() {
            return true;
        }

        if self.swapchain_invalid.take() {
            let Some(surface) = &mut self.surface else {
                return false;
            };

            // The swapchain is being rebuilt; hand the old per-image fences off to the shared
            // in-flight list and forget them here.
            for fut in self.last_in_flight.drain(..).flatten() {
                self.vk_ctx.push_in_flight_future(fut);
            }

            let (swapchain, swapchain_images) = self
                .vk_ctx
                .recreate_swapchain(surface.swapchain.clone(), self.size.get())
                .expect("failed to recreate swapchain");

            surface.swapchain = swapchain;
            surface.swapchain_images = swapchain_images.into();
        }

        let (image_index, _is_suboptimal, swap_acq_fut) = {
            let Some(surface) = &self.surface else {
                return false;
            };
            match acquire_next_image(surface.swapchain.clone(), Some(Duration::ZERO)) {
                Ok((idx, suboptimal, fut)) => (idx, suboptimal, fut),
                Err(_) => return false,
            }
        };

        // TODO: Recreate if suboptimal immediately?

        self.acquired = Some(AcquiredSlot {
            image_index,
            swap_acq_fut,
        });
        true
    }

    fn draw(&mut self) {
        // Try to acquire again, since this is no-op if we already have acquired a valid swapchain.
        if !self.try_acquire_swapchain() {
            return;
        }

        let Some(surface) = &self.surface else {
            return;
        };

        let Some(prq) = self.prq.take() else {
            // No primary rendering queue was staged for this frame.
            return;
        };

        let Some(AcquiredSlot {
            image_index,
            swap_acq_fut,
        }) = self.acquired.take()
        else {
            return;
        };

        // The previous present on this image index is joined onto so we never render into it
        // while it is still in flight.
        if self.last_in_flight.len() <= image_index as usize {
            self.last_in_flight.resize(image_index as usize + 1, None);
        }
        let prev = self.last_in_flight[image_index as usize].clone();

        match prq.build_root(
            self.vk_ctx.clone(),
            surface,
            &self.window,
            image_index,
            swap_acq_fut,
            prev,
        ) {
            Ok(fence) => {
                // Register a copy with the shared in-flight list (cleaned up, non-blockingly, by
                // the main loop), and keep one per image index for the next frame to join onto.
                self.vk_ctx.push_in_flight_future(fence.clone());
                self.last_in_flight[image_index as usize] = Some(fence);
                self.window().pre_present_notify();
            }
            Err(e) => {
                tracing::error!(window_id = ?self.id, image_index, "failed to execute PRQ: {e}");
            }
        }
    }

    fn set_prq(&mut self, prq: PrimaryRenderingQueue) {
        self.prq = Some(prq);
    }

    fn switch_level(&mut self, level: LevelIndex) {
        self.level = level;
    }
}

impl RootWindow {
    /// Moves every per-image present fence over to the shared [`VkContext`] in-flight list.
    ///
    /// This is used when the swapchain is created, recreated, or torn down, so that the old
    /// fences are cleaned up once the GPU finishes with them rather than being dropped (which
    /// would block or leak). The per-image list is left empty afterwards.
    fn flush_last_in_flight(&mut self) {
        for fut in self.last_in_flight.drain(..).flatten() {
            self.vk_ctx.push_in_flight_future(fut);
        }
    }
}
