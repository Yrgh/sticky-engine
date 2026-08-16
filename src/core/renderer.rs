//! The renderer

use std::{collections::HashSet, sync::Arc};

use anyhow::Result as AResult;

use sticky_engine_macros::slot_def;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, PrimaryAutoCommandBuffer,
};

use crate::core::{level::LevelIndex, trans::STrans3, vk::VkContext};

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
        let cb1 = AutoCommandBufferBuilder::primary(
            vk_ctx.command_buffer_allocator.clone(),
            0,
            CommandBufferUsage::OneTimeSubmit,
        )?;

        let cb2 = AutoCommandBufferBuilder::primary(
            vk_ctx.command_buffer_allocator.clone(),
            0,
            CommandBufferUsage::OneTimeSubmit,
        )?;

        // TODO: If there is a primary camera
        let prq = PrimaryRenderingQueue {
            
        };

        Ok((cb1.build()?, Some(prq)))
    }
}

/// Queue to render the primary camera
pub(crate) struct PrimaryRenderingQueue {
}