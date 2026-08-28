//! The renderer

use std::{collections::HashSet, sync::Arc};

use anyhow::{Result as AResult, bail};

use sticky_engine_macros::slot_def;
use vulkano::{
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferExecFuture, CommandBufferUsage,
        PrimaryAutoCommandBuffer, PrimaryCommandBufferAbstract, RenderPassBeginInfo,
        SubpassBeginInfo, SubpassEndInfo,
    },
    format::ClearValue,
    image::{ImageLayout, view::ImageView},
    render_pass::{
        AttachmentDescription, AttachmentLoadOp, AttachmentReference, AttachmentStoreOp,
        Framebuffer, FramebufferCreateInfo, RenderPass, RenderPassCreateInfo, SubpassDescription,
    },
    swapchain::{PresentFuture, SwapchainAcquireFuture, SwapchainPresentInfo},
    sync::{GpuFuture, future::FenceSignalFuture},
};
use winit::window::Window as OsWindow;

use crate::core::{level::LevelIndex, trans::STrans3, vk::VkContext, window::RootWindowSurface};

#[slot_def]
/// Camera
pub trait SCameraView3d: STrans3 {}

/// Queue of objects to draw, for example meshes and cameras.
pub struct RenderingQueue {}

impl RenderingQueue {
    pub(crate) fn new() -> Self {
        Self {}
    }

    /// Searches all submitted items for references to camera
    pub(crate) fn search_dependencies(&self) -> HashSet<LevelIndex> {
        HashSet::new()
    }

    pub(crate) fn build(
        &self,
        vk_ctx: Arc<VkContext>,
    ) -> AResult<(Arc<PrimaryAutoCommandBuffer>, Option<PrimaryRenderingQueue>)> {
        let cb = AutoCommandBufferBuilder::primary(
            vk_ctx.command_buffer_allocator.clone(),
            0,
            CommandBufferUsage::OneTimeSubmit,
        )?;

        // TODO: Prepare scene

        // TODO: Render cameras

        // TODO: If there is a primary camera
        let prq = PrimaryRenderingQueue { exec_after: None };

        Ok((cb.build()?, Some(prq)))
    }
}

type FencedCommandBufferFuture =
    FenceSignalFuture<CommandBufferExecFuture<Box<dyn GpuFuture + Send + Sync>>>;

/// The final present fence for a single swapchain image.
///
/// This must be kept alive (never dropped directly), since dropping an un-cleaned
/// `FenceSignalFuture` blocks the thread until the GPU finishes. Instead, it is registered on
/// the [`VkContext`], which calls [`cleanup_finished`](GpuFuture::cleanup_finished) on it and
/// removes it once its fence is signalled.
pub(crate) type FinalPresentFuture = FenceSignalFuture<
    PresentFuture<CommandBufferExecFuture<Box<dyn GpuFuture + Send + Sync>>>,
>;

/// Queue to render the primary camera
pub(crate) struct PrimaryRenderingQueue {
    pub(crate) exec_after: Option<Arc<FencedCommandBufferFuture>>,
}

impl PrimaryRenderingQueue {
    /// Builds the primary render + present for a window.
    ///
    /// `next_idx` and `swap_acq_fut` come from a prior non-blocking
    /// [`acquire_next_image`](vulkano::swapchain::acquire_next_image). `prev_in_flight` is the
    /// previous present fence for `next_idx`, which this frame joins onto so it never renders
    /// into an image while the prior present on it is still in flight. The returned present
    /// fence must be registered on the [`VkContext`] for lifetime tracking; it must not be
    /// dropped.
    pub(crate) fn build_root(
        &self,
        vk_ctx: Arc<VkContext>,
        window: &RootWindowSurface,
        os: &OsWindow,
        next_idx: u32,
        swap_acq_fut: SwapchainAcquireFuture,
        prev_in_flight: Option<Arc<FinalPresentFuture>>,
    ) -> AResult<Arc<FinalPresentFuture>> {
        let mut cb = AutoCommandBufferBuilder::primary(
            vk_ctx.command_buffer_allocator.clone(),
            0,
            CommandBufferUsage::OneTimeSubmit,
        )?;

        let Some(future) = self.exec_after.clone() else {
            bail!("no exec_after future")
        };

        let future = future.join(swap_acq_fut).boxed_send_sync();
        let future = match prev_in_flight {
            Some(prev) => future.join(prev).boxed_send_sync(),
            None => future,
        };

        let swap_image = window.swapchain_images[next_idx as usize].clone();

        let format = window.swapchain.image_format();
        let render_pass = RenderPass::new(
            vk_ctx.device.clone(),
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

        let queue = vk_ctx.queues[0].clone();

        let future = cb.build()?.execute_after(future, queue.clone())?;
        future.flush()?;

        os.pre_present_notify();

        let future = future
            .then_swapchain_present(
                queue,
                SwapchainPresentInfo::swapchain_image_index(window.swapchain.clone(), next_idx),
            )
            .then_signal_fence_and_flush()?;

        Ok(Arc::new(future))
    }
}
