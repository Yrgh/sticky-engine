//! Rendering queues and slots

use std::collections::HashSet;

use sticky_engine_macros::slot_def;

use crate::core::{level::LevelIndex};

#[slot_def]
/// Camera
pub trait SCameraView3d {}

/// Queue of objects to draw, for example meshes and cameras.
pub struct RenderingQueue {}

impl RenderingQueue {
    pub(crate) fn new() -> Self {
        Self {}
    }

    #[expect(unused)]
    /// Searches all submitted items for references to camera
    pub(crate) fn search_dependencies(&self) -> HashSet<LevelIndex> {
        HashSet::new()
    }
}
