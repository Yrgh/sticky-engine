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

pub mod traits;
use std::any::Any;

pub use traits::{IAssetAccessor, IAssetCacher, IAssetLoader, IAssetSaver};

pub mod manager;
pub use manager::AssetManager;

pub mod interner;
pub use interner::Interner;

pub mod storage;
pub use storage::{Asset, DynAsset, DynOwnedAsset, IAsset, OwnedAsset};

/// Shortcut to implement [`IAsset`] for types that don't contain nested
/// [`Asset`]s.
pub trait AutoAsset: Any + Send + Sync {}

impl<T: AutoAsset> IAsset for T {
    fn resolve_blocking(
        &mut self,
        _asset_manager: &AssetManager,
    ) -> Result<(), manager::GetAssetError> {
        Ok(())
    }

    fn resolve_async<'a>(
        &'a mut self,
        _asset_manager: &'a AssetManager,
    ) -> storage::BoxedFuture<'a, Result<(), manager::GetAssetError>> {
        Box::pin(async { Ok(()) })
    }
}
