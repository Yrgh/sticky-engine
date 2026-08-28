//! The default renderer, built on top of the [`api_vk`](super::api_vk) GPU API.

use std::{
    any::Any,
    cell::Cell,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result as AResult};

use vulkano::{
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferUsage, PrimaryCommandBufferAbstract,
        RenderPassBeginInfo, SubpassBeginInfo, SubpassEndInfo,
    },
    format::ClearValue,
    image::{ImageLayout, view::ImageView},
    render_pass::{
        AttachmentDescription, AttachmentLoadOp, AttachmentReference, AttachmentStoreOp,
        Framebuffer, FramebufferCreateInfo, RenderPass, RenderPassCreateInfo, SubpassDescription,
    },
    swapchain::{Surface, Swapchain, SwapchainPresentInfo, acquire_next_image},
    sync::{GpuFuture, now},
};
use winit::{
    dpi::PhysicalSize,
    event_loop::ActiveEventLoop,
    window::Window as OsWindow,
};

use crate::core::{
    gpu_api::{BoxedInstructions, GpuApi, IRenderer, ISurface, WindowRenderInstructions},
    rendering::RenderingQueue,
};

use super::api_vk::{FinalPresentFuture, VkContext};

/// A swapchain image acquired non-blockingly, staged for presentation.
struct AcquiredSlot {
    image_index: u32,
    swap_acq_fut: vulkano::swapchain::SwapchainAcquireFuture,
}

/// A window surface backed by a Vulkan swapchain.
///
/// This implements the engine's [`ISurface`] and owns the swapchain, its
/// images, a staged non-blocking acquisition, and per-image present fences used
/// to join consecutive frames that render to the same image.
pub(crate) struct VkSurface {
    ctx: Rc<VkContext>,
    swapchain: Arc<Swapchain>,
    swapchain_images: Vec<Arc<vulkano::image::Image>>,
    size: PhysicalSize<u32>,
    needs_recreate: Cell<bool>,
    /// A staged swapchain acquisition: the image index and the future marking
    /// when the image is available. Only one frame is in flight per window, so
    /// at most one acquisition is staged at a time.
    acquired: std::cell::RefCell<Option<AcquiredSlot>>,
    /// Per swapchain image index, the most recent present fence for that image.
    ///
    /// The next frame rendered to the same image joins onto its entry so it
    /// never renders into an image while the previous present on that image is
    /// still in flight. When the swapchain changes, these are handed off to the
    /// shared [`VkContext`] in-flight list so they are cleaned up rather than
    /// dropped un-cleaned.
    last_in_flight: std::cell::RefCell<Vec<Option<Arc<FinalPresentFuture>>>>,
    /// The underlying `VkSurfaceKHR`.
    ///
    /// This is declared last so that it is dropped last: the swapchain (and
    /// everything referencing it) must be released while the surface is still
    /// alive, or the driver segfaults tearing down its Wayland presentation.
    #[expect(unused)]
    surface: Arc<Surface>,
}

impl Drop for VkSurface {
    fn drop(&mut self) {
        // Wait for the GPU to finish all queued work so that no present fence
        // is still in flight and referencing the swapchain or its images.
        if let Err(e) = unsafe { self.ctx.device.wait_idle() } {
            tracing::error!("failed to wait for the device while dropping surface: {e}");
        }

        // Release every present fence now, while both the device and the
        // surface are still alive. The shared in-flight list, the staged
        // acquisition, and the per-image fences all reference the swapchain;
        // dropping them here (rather than when the context or surface drops
        // later) lets the swapchain be destroyed before the surface.
        self.ctx.clear_in_flight_futures();
        *self.acquired.borrow_mut() = None;
        *self.last_in_flight.borrow_mut() = Vec::new();
    }
}

impl VkSurface {
    fn new(ctx: Rc<VkContext>, window: &OsWindow, size: PhysicalSize<u32>) -> AResult<Self> {
        // # Safety
        // `create_surface` is handed a window that outlives the returned
        // surface: the owning `RootWindow` holds an `Arc<OsWindow>` and drops
        // the surface (on suspend or drop) before the window.
        let surface = unsafe { Surface::from_window_ref(ctx.instance.clone(), window) }
            .context("failed to create Vulkan surface")?;

        let (swapchain, swapchain_images) = ctx.create_swapchain(surface.clone(), size)?;

        Ok(Self {
            ctx,
            swapchain,
            swapchain_images,
            size,
            needs_recreate: Cell::new(false),
            acquired: std::cell::RefCell::new(None),
            last_in_flight: std::cell::RefCell::new(Vec::new()),
            surface,
        })
    }

    /// Moves every per-image present fence over to the shared [`VkContext`]
    /// in-flight list.
    ///
    /// This is used when the swapchain is recreated, so that the old fences are
    /// cleaned up once the GPU finishes with them rather than being dropped
    /// (which would block or leak). The per-image list is left empty afterwards.
    fn flush_last_in_flight(&self) {
        for fut in self.last_in_flight.borrow_mut().drain(..).flatten() {
            self.ctx.push_in_flight_future(fut);
        }
    }
}

impl ISurface for VkSurface {
    fn try_acquire(&mut self) -> bool {
        if self.acquired.borrow().is_some() {
            return true;
        }

        // The window will never render while the swapchain is invalid, so
        // recreate it when requested.
        if self.needs_recreate.take() {
            self.flush_last_in_flight();

            let Ok((swapchain, swapchain_images)) = self
                .ctx
                .recreate_swapchain(self.swapchain.clone(), self.size)
            else {
                self.needs_recreate.set(true);
                return false;
            };

            self.swapchain = swapchain;
            self.swapchain_images = swapchain_images;
        }

        let (image_index, _is_suboptimal, swap_acq_fut) =
            match acquire_next_image(self.swapchain.clone(), Some(Duration::ZERO)) {
                Ok(acquired) => acquired,
                Err(_) => return false,
            };

        // TODO: Recreate if suboptimal immediately?

        self.acquired.borrow_mut().replace(AcquiredSlot {
            image_index,
            swap_acq_fut,
        });
        true
    }

    fn on_resize(&mut self, new_size: PhysicalSize<u32>) {
        self.size = new_size;
        self.needs_recreate.set(true);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The instructions to render a level's primary camera to a window.
pub(crate) struct PrimaryRenderingQueue {}

impl PrimaryRenderingQueue {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl WindowRenderInstructions for PrimaryRenderingQueue {
    fn render(&self, surface: &dyn ISurface) -> AResult<()> {
        let surface = surface
            .as_any()
            .downcast_ref::<VkSurface>()
            .expect("renderer given a surface it did not create");

        let Some(AcquiredSlot {
            image_index,
            swap_acq_fut,
        }) = surface.acquired.borrow_mut().take()
        else {
            return Ok(());
        };

        // The previous present on this image index is joined onto so we never
        // render into it while it is still in flight.
        let prev = {
            let mut last = surface.last_in_flight.borrow_mut();
            if last.len() <= image_index as usize {
                last.resize(image_index as usize + 1, None);
            }
            last[image_index as usize].clone()
        };

        let fence = build_root(surface, image_index, swap_acq_fut, prev)?;

        // Register a copy with the shared in-flight list (cleaned up,
        // non-blockingly, by the main loop), and keep one per image index for
        // the next frame to join onto.
        surface.ctx.push_in_flight_future(fence.clone());
        surface.last_in_flight.borrow_mut()[image_index as usize] = Some(fence);

        Ok(())
    }
}

/// Builds and submits a render pass that clears the given swapchain image, then
/// presents it.
///
/// `next_idx` and `swap_acq_fut` come from a prior non-blocking
/// [`acquire_next_image`]. `prev_in_flight` is the previous present fence for
/// `next_idx`, which this frame joins onto so it never renders into an image
/// while the prior present on it is still in flight. The returned present fence
/// must be registered on the [`VkContext`] for lifetime tracking; it must not
/// be dropped.
fn build_root(
    surface: &VkSurface,
    next_idx: u32,
    swap_acq_fut: vulkano::swapchain::SwapchainAcquireFuture,
    prev_in_flight: Option<Arc<FinalPresentFuture>>,
) -> AResult<Arc<FinalPresentFuture>> {
    let ctx = &surface.ctx;

    let queue_family_index = ctx.queues[0].queue_family_index();
    let mut cb = AutoCommandBufferBuilder::primary(
        ctx.command_buffer_allocator.clone(),
        queue_family_index,
        CommandBufferUsage::OneTimeSubmit,
    )?;

    let future = now(ctx.device.clone()).boxed_send_sync();
    let future = future.join(swap_acq_fut).boxed_send_sync();
    let future = match prev_in_flight {
        Some(prev) => future.join(prev).boxed_send_sync(),
        None => future,
    };

    let swap_image = surface.swapchain_images[next_idx as usize].clone();

    let format = surface.swapchain.image_format();
    let render_pass = RenderPass::new(
        ctx.device.clone(),
        RenderPassCreateInfo {
            attachments: vec![AttachmentDescription {
                format,
                load_op: AttachmentLoadOp::Clear,
                store_op: AttachmentStoreOp::Store,
                initial_layout: ImageLayout::Undefined,
                final_layout: ImageLayout::PresentSrc,
                ..Default::default()
            }],
            subpasses: vec![SubpassDescription {
                color_attachments: vec![Some(AttachmentReference {
                    attachment: 0,
                    layout: ImageLayout::ColorAttachmentOptimal,
                    ..Default::default()
                })],
                ..Default::default()
            }],
            ..Default::default()
        },
    )?;

    let image_view = ImageView::new_default(swap_image.clone())?;
    let framebuffer = Framebuffer::new(
        render_pass.clone(),
        FramebufferCreateInfo {
            attachments: vec![image_view],
            ..Default::default()
        },
    )?;

    let clear_color = match next_idx % 3 {
        0 => [0.54, 0.48, 0.48, 1.0],
        1 => [0.48, 0.54, 0.48, 1.0],
        _ => [0.48, 0.48, 0.54, 1.0],
    };

    cb.begin_render_pass(
        RenderPassBeginInfo {
            clear_values: vec![Some(ClearValue::Float(clear_color))],
            ..RenderPassBeginInfo::framebuffer(framebuffer)
        },
        SubpassBeginInfo::default(),
    )?;
    cb.end_render_pass(SubpassEndInfo::default())?;

    let queue = ctx.queues[0].clone();

    let future = cb.build()?.execute_after(future, queue.clone())?;
    future.flush()?;

    let future = future
        .then_swapchain_present(
            queue,
            SwapchainPresentInfo::swapchain_image_index(surface.swapchain.clone(), next_idx),
        )
        .then_signal_fence_and_flush()?;

    Ok(Arc::new(future))
}

/// A renderer that draws windows using [`vulkano`].
///
/// It is created via [`WorldBuilder::with_renderer`], pairing a
/// [`VulkanApi`](super::api_vk::VulkanApi) GPU API with this renderer.
pub struct VkRenderer {
    ctx: Rc<VkContext>,
}

impl IRenderer for VkRenderer {
    type InitInfo = ();

    fn init(_info: (), event_loop: &ActiveEventLoop) -> AResult<(Rc<Self>, GpuApi)> {
        let ctx = Rc::new(VkContext::new(Some(event_loop))?);
        let api: GpuApi = Rc::new(super::api_vk::VulkanApi::new_with_ctx(ctx.clone()));
        Ok((Rc::new(Self { ctx }), api))
    }

    fn create_surface(
        &self,
        window: &OsWindow,
        size: PhysicalSize<u32>,
    ) -> AResult<Box<dyn ISurface>> {
        Ok(Box::new(VkSurface::new(self.ctx.clone(), window, size)?))
    }

    fn render_level(
        &self,
        _rendering_queue: &RenderingQueue,
    ) -> AResult<(Box<dyn Any>, Option<BoxedInstructions>)> {
        // TODO: Render shadows and secondary cameras; detect a primary camera
        // and only return window instructions when one exists.
        Ok((Box::new(()), Some(Box::new(PrimaryRenderingQueue::new()))))
    }

    fn submit_level_instructions(
        &self,
        _instructions: &mut dyn Iterator<Item = Box<dyn Any>>,
    ) -> AResult<()> {
        // TODO: Submit secondary level instructions to the GPU.
        Ok(())
    }
}
