//! Dynamic API for runtime selection of a GPU backend by the engine.

use std::{any::Any, rc::Rc};

use anyhow::Result as AResult;

use winit::{dpi::PhysicalSize, event_loop::ActiveEventLoop, window::Window as OsWindow};

use crate::core::rendering::RenderingQueue;

/// A backend for GPU interaction, including window surfaces and rendering.
///
/// The GPU API will be stored in an [`Rc`](std::sync::Rc), so it may need
/// interior mutability.
///
/// The GPU API may be created independently, but will likely be created by the
/// renderer.
pub trait IGpuApi: Any {
    /// Called periodically to clean up resources that may need it.
    fn cleanup_in_flight(&self);
}

/// Alias for a reference to a GPU API
pub type GpuApi = Rc<dyn IGpuApi>;

/// A surface for a window, including a swapchain.
///
/// When the surface is dropped **all** resources derived from the window handle
/// need to be dropped. All in-flight data needs to be handled properly.
pub trait ISurface: Any {
    /// Try to acquire the next swapchain image **without waiting**, returning
    /// `true` if one is available.
    ///
    /// If a swapchain was previously stated to be available, this should return
    /// `true` without requesting another one. Once an image has been used, this
    /// should return `false`.
    fn try_acquire(&mut self) -> bool;

    /// Called when the window is resized.
    fn on_resize(&mut self, new_size: PhysicalSize<u32>);
}

/// A renderer that will be used to draw to windows.
pub trait IRenderer: Any {
    /// Extra parameter used during initialization.
    type InitInfo
    where
        Self: Sized;

    /// Create the renderer **and** the required GPU API
    fn init(
        info: Self::InitInfo,
        event_loop: &ActiveEventLoop,
    ) -> AResult<(Rc<Self>, GpuApi)>
    where
        Self: Sized;

    /// Create a new window surface.
    fn create_surface(
        &self,
        window: &OsWindow,
        size: PhysicalSize<u32>,
    ) -> AResult<Box<dyn ISurface>>;

    /// Submit the rendering queue to render things like shadows and secondary
    /// cameras, and create an object to render the primary camera if one
    /// exists.
    fn render_level(&self, rendering_queue: RenderingQueue) -> AResult<Option<WindowInstructions>>;
}

/// Instructions for drawing to a window, produced by
/// [`IRenderer::render_level`] for [`Level`](crate::core::level::Level)s owned
/// by a window and containing a primary camera.
pub trait WindowRenderInstructions: Any {
    /// Submit the instructions to be rendered to the given window, including presentation.
    fn render(&self, surface: &dyn ISurface) -> AResult<()>;
}

/// An ali
pub type WindowInstructions = Box<dyn WindowRenderInstructions>;
