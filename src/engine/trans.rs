//! Transform utilities

use macros::slot_def;

use crate::engine::world::World;

/// Transform of a 3D object
pub type Trans3 = glamx::Pose3;

#[slot_def]
/// Core Slot for Components with a 3D transform
pub trait STrans3 {
    /// Returns the transform relative to the owning [`Level`](crate::engine::level::Level).
    fn get_global_trans(&self, world: &World) -> Trans3;
    /// Returns the transform relative to the parent.
    fn get_local_trans(&self, world: &World) -> Trans3;
    /// Sets the transform relative to the owning [`Level`](crate::engine::level::Level).
    fn set_global_trans(&mut self, trans: Trans3, world: &World);
    /// Sets the transform relative to the parent.
    fn set_local_trans(&mut self, trans: Trans3, world: &World);
    
}