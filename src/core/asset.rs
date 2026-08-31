//! Asset management traits and asset types.
//!
//! An "asset" is a piece of data that can be serialized and/or deserialized and
//! is `Any + Sync + Send`. Assets are normally stored in [`Asset`], but if a
//! mutable, singly-owned asset is desired, you can use [`OwnedAsset`] instead.
//!
//! The [`AssetManager`] is a collection of 4 pieces:
//! - [Loaders](IAssetLoader) and [Cachers](IAssetCacher)
//! - [Savers](IAssetSaver)
//! - An [Accessor](IAssetAccessor)
//!
//! When you first create the `AssetManager`, you decide what loaders and savers
//! go to what types, and what accessor and cacher to use.
//!
//! # Limitations
//!
//! Because of the nature of `serde`, there is no way to integrate it with the
//! loaders and savers, since those rely on outputs as bytes. While an
//! integration is possible, it must be done manually, per-format.

pub mod base;

mod manager_traits;
pub use manager_traits::*;

mod manager;
pub use manager::{GlobalInterner, AssetManager, AssetManagerBuilder, GetAssetError, SetAssetError};

mod storage;
pub use storage::*;