//! Built-in, non-core features for the engine.
//!
//! While core features such as the main loop, renderer, and transforms are in
//! the [`engine`](crate::core) module, these features can be manually
//! implemented by the user, and can be gated behind feature flags. For example,
//! this module contains any and all Components the engine will provide.

#[cfg(feature = "gpu-vulkan")]
pub mod api_vk;

#[cfg(feature = "vulkan-renderer")]
pub mod renderer_vk;

pub mod assets;