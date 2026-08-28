//! Dynamic API for runtime selection of a GPU backend by the engine.

use std::any::Any;

/// A backend for GPU interaction, including window surfaces and rendering.
pub trait IGpuApi: Any {

    /// Called periodically to clean up resources that may need it.
    fn cleanup_in_flight(&self);
}

/// A surface for a window, including a swapchain.
pub trait ISurface: Any {
    /// Try to acquire the next swapchain image **without waiting**, returning
    /// `true` if one is available.
    /// 
    /// If a swapchain was previously stated to be available, this should return
    /// `true` without requesting another one. Once an image has been used, this
    /// should return `false`.
    fn try_acquire(&mut self) -> bool;

    
}