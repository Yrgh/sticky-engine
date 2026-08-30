use std::{any::{Any, TypeId}, pin::Pin, str::Utf8Error, sync::Arc};

use thiserror::Error;

use super::*;

#[derive(Error, Debug)]
/// Error returned when saving or loading raw bytes via an [`IAssetAccessor`].
pub enum BytesError {
    #[error("bad path: {}", .0.as_ref().map(|s| s.as_str()).unwrap_or(""))]
    /// Returned if the given path was invalid, not necessarily missing.
    BadPath(Option<String>),
    #[error("the given path does not allow saving")]
    /// Returned if the asset cannot be saved at given location.
    ReadOnly,
    #[error("I/O error: {0}")]
    /// Returned if an I/O error occurred.
    Io(#[from] std::io::Error),
    #[error("{0}")]
    /// Any other error
    Other(anyhow::Error),
}

impl BytesError {
    /// Create an error from any other error.
    pub fn other(other: impl Into<anyhow::Error>) -> Self {
        Self::Other(other.into())
    }
}

/// Alias used for [`IAssetAccessor::load_bytes_async`] and
/// [`IAssetAccessor::load_bytes_blocking`] outputs.
pub type AccessorLoadBytesResult = Result<Box<[u8]>, BytesError>;

/// Alias used for [`IAssetAccessor::save_bytes_async`] and
/// [`IAssetAccessor::save_bytes_blocking`] outputs.
pub type AccessorSaveBytesResult = Result<(), BytesError>;

/// Accessor for assets.
///
/// The accessor is in charge of manipulating assets on disk. Generally, you
/// should pick an accessor and stick with it, since the way paths behave is
/// entirely up to its implementation.
///
/// The most basic (and default) accessor loads the corresponding file from
/// disk, few questions asked, but it can go as crazy as loading data over
/// network.
pub trait IAssetAccessor: Any + Send + Sync {
    /// Load bytes asynchronously.
    fn load_bytes_async<'a>(
        &'a self,
        asset_path: &'a str,
    ) -> Pin<Box<dyn Future<Output = AccessorLoadBytesResult> + Send + 'a>>;
    /// Load bytes, blocking the thread until completion.
    fn load_bytes_blocking(&self, asset_path: &str) -> AccessorLoadBytesResult;

    /// Save bytes asynchronously.
    fn save_bytes_async<'a>(
        &'a self,
        asset_path: &'a str,
        bytes: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = AccessorSaveBytesResult> + Send + 'a>>;
    /// Save bytes, blocking the thread until completion.
    fn save_bytes_blocking(&self, asset_path: &str, bytes: &[u8]) -> AccessorSaveBytesResult;
}

#[derive(Error, Debug)]
/// Error returned by [`IAssetLoader::load_from_bytes`].
pub enum LoadAssetError {
    #[error("the given asset does not match the loader's type")]
    /// Returned if the asset's path or bytes are indicative that there is a
    /// type mismatch.
    IncorrectType,
    #[error("error in path: {0}")]
    /// Returned if the given asset path had a problem, with a description of
    /// what happened.
    BadPath(String),
    #[error("utf8 decode error: {0}")]
    /// Returned if there is a string decode error
    Utf8(#[from] Utf8Error),
    #[error("{0}")]
    /// Any other error
    Other(anyhow::Error),
}

impl LoadAssetError {
    /// Create an error from any other error.
    pub fn other(other: impl Into<anyhow::Error>) -> Self {
        Self::Other(other.into())
    }
}

/// A specific loader for a specific asset.
///
/// You can only have one registered loader per asset type. If you want to load
/// a config file either via binary or plaintext, you can create a loader that
/// selects based on header and/or filepath, and/or try both.
pub trait IAssetLoader: Any + Send + Sync {
    /// Load an asset from its bytes, using the path as a hint if desired.
    fn load_from_bytes(
        &self,
        asset_path: &str,
        bytes: &[u8],
    ) -> Result<Box<dyn Any>, LoadAssetError>;

    /// Returns whether this asset loader can load the given type.
    fn loads(&self, type_id: TypeId) -> bool;
}

#[derive(Error, Debug)]
/// Error returned by [`IAssetSaver::save_as_bytes`].
pub enum SaveAssetError {
    #[error("the given asset does not match the saver's type")]
    /// Returned if the given asset is not of the expected type.
    IncorrectType,
    #[error("error in path: {0}")]
    /// Returned if the given asset path had a problem, with a description of
    /// what happened.
    BadPath(String),
    #[error("{0}")]
    /// Any other error
    Other(anyhow::Error),
}

impl SaveAssetError {
    /// Create an error from any other error.
    pub fn other(other: impl Into<anyhow::Error>) -> Self {
        Self::Other(other.into())
    }
}

/// A specific saver for a specific asset.
///
/// You can only have one registered saver per asset type. If you want to save a
/// config file either via binary or plaintext, you can create newtypes around
/// the config file for each saving mode.
pub trait IAssetSaver: Any + Send + Sync {
    /// Serialize an asset as bytes
    fn save_as_bytes(&self, asset_path: &str, value: &dyn Any) -> Result<Box<[u8]>, SaveAssetError>;

    /// Returns whether this asset saver can save the given type.
    fn saves(&self, type_id: TypeId) -> bool;
}

/// A type that is both a saver and a loader and can be split into halves for
/// each.
pub trait SaverLoader: IAssetSaver + IAssetLoader + Sized {
    /// Create two versions of this type: one for saving (left) and loading
    /// (right).
    fn split(self) -> (Self, Self);
}

#[derive(Debug, Error)]
/// Returned by [`IAssetCacher::update_asset`]
pub enum UpdateError {
    #[error("given asset type does not match existing asset type")]
    /// Returned if the cached asset and the given asset have different types
    WrongType,
}

/// Cacher for assets of a specific type.
///
/// Secondarily, it is the asset cacher's job to intern the strings (see
/// [`Asset::from_parts`]).
///
/// It is important to remember this model: retrieve_asset_* will either return
/// an asset, *or* lock the asset's slot. update_asset_privileged and
/// release_asset_lock will not wait on the lock, and unlock the asset if it
/// was.
pub trait IAssetCacher: Any + Send + Sync {
    /// Return the asset if it is cached, lock the asset if it is not, blocking
    /// while the asset is locked.
    fn retrieve_asset_blocking(&self, asset_path: &str) -> Option<DynAsset>;

    /// Return the asset if it is cached, lock the asset if it is not, waiting
    /// while the asset is locked.
    fn retrieve_asset_async<'a>(
        &'a self,
        asset_path: &'a str,
    ) -> Pin<Box<dyn Future<Output = Option<DynAsset>> + Send + 'a>>;

    /// Update the cached asset to point to a new value without waiting or
    /// blocking on the asset lock, unlocking the asset upon completion.
    ///
    /// The [`AssetManager`] will call this *or*
    /// [`release_asset_lock`](IAssetCacher::release_asset_lock) *exactly once*
    /// after `retrieve_asset_*`, but you may not use this fact as a safety
    /// guarantee.
    fn update_asset_unlocking(
        &self,
        asset: DynOwnedAsset,
    ) -> Result<DynAsset, (UpdateError, DynOwnedAsset)>;

    /// Update the cached asset to point to a new value, blocking while the asset
    /// is locked.
    fn update_asset_blocking(
        &self,
        asset: DynOwnedAsset,
    ) -> Result<DynAsset, (UpdateError, DynOwnedAsset)>;

    /// Update the cached asset to point to a new value, waiting while the asset is
    /// locked.
    fn update_asset_async<'a>(
        &'a self,
        asset: DynOwnedAsset,
    ) -> Pin<Box<dyn Future<Output = Result<DynAsset, (UpdateError, DynOwnedAsset)>> + Send + 'a>>;

    /// If the asset is locked, release the lock.
    fn release_asset_lock(&self, asset_path: &str);

    /// Returns whether the asset type is cached by this manager.
    fn caches(&self, type_id: TypeId) -> bool;
}

mod private { pub trait Sealed {} }

/// Helper trait for types that can be converted to `Arc<dyn IAssetCacher`.
pub trait IntoCacher: private::Sealed {
    /// Convert this type into a cacher that can be stored.
    fn into_cacher(self) -> Arc<dyn IAssetCacher>;
}

impl<T: IAssetCacher + Sized> private::Sealed for T {}

impl<T: IAssetCacher + Sized> IntoCacher for T {
    fn into_cacher(self) -> Arc<dyn IAssetCacher> {
        Arc::new(self)
    }
}

impl private::Sealed for Box<dyn IAssetCacher> {}

impl IntoCacher for Box<dyn IAssetCacher> {
    fn into_cacher(self) -> Arc<dyn IAssetCacher> {
        self.into()
    }
}

impl<T: IAssetCacher> private::Sealed for Arc<T> {}

impl<T: IAssetCacher> IntoCacher for Arc<T> {
    fn into_cacher(self) -> Arc<dyn IAssetCacher> {
        self
    }
}

impl<T: IAssetCacher> private::Sealed for &Arc<T> {}

impl<T: IAssetCacher> IntoCacher for &Arc<T> {
    fn into_cacher(self) -> Arc<dyn IAssetCacher> {
        self.clone()
    }
}