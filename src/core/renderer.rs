//! The renderer

use std::{collections::HashSet, sync::Arc};

use anyhow::{Result as AResult, bail};

use sticky_engine_macros::slot_def;
use vulkano::{
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferExecFuture, CommandBufferUsage,
        PrimaryAutoCommandBuffer, PrimaryCommandBufferAbstract,
    },
    swapchain::{SwapchainPresentInfo, acquire_next_image},
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

type FencedCommandBufferFuture = FenceSignalFuture<CommandBufferExecFuture<Box<dyn GpuFuture + Send + Sync>>>;

/// Queue to render the primary camera
pub(crate) struct PrimaryRenderingQueue {
    pub(crate) exec_after: Option<Arc<FencedCommandBufferFuture>>,
}

impl PrimaryRenderingQueue {
    pub(crate) fn build_root(
        &self,
        vk_ctx: Arc<VkContext>,
        window: &RootWindowSurface,
        os: &OsWindow,
    ) -> AResult<FenceSignalFuture<impl GpuFuture + Send + Sync + 'static>> {
        let cb = AutoCommandBufferBuilder::primary(
            vk_ctx.command_buffer_allocator.clone(),
            0,
            CommandBufferUsage::OneTimeSubmit,
        )?;

        let Some(future) = self.exec_after.clone() else {
            bail!("no exec_after future");
        };

        // TODO: Send is_suboptimal to the window
        let (next_idx, is_suboptimal, swap_acq_fut) =
            acquire_next_image(window.swapchain.clone(), None)?;
        debug_assert!(!is_suboptimal, "suboptimal swapchain");
        let future = future.join(swap_acq_fut);
        let swap_image = window.swapchain_images[next_idx as usize].clone();

        let _ = swap_image;

        // TODO: Render camera

        let queue = vk_ctx.queues[0].clone();

        let future = cb
            .build()?
            .execute_after(future, queue.clone())?;
        future.flush()?;

        os.pre_present_notify();

        let future = future
            .then_swapchain_present(
                queue,
                SwapchainPresentInfo::swapchain_image_index(window.swapchain.clone(), next_idx),
            )
            .then_signal_fence_and_flush()?;

        Ok(future)
    }
}
