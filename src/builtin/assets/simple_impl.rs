//! Simple utilities for quick-and-dirty asset management.
//!
//! See the [`asset`](crate::core::asset) module.

use std::{
    any::{Any, TypeId},
    collections::HashSet,
    marker::PhantomData,
    path::{Component, Path, PathBuf},
    pin::Pin,
    sync::Arc,
};

use elsa::sync::FrozenMap;

use parking_lot::RwLock;

use crate::core::asset::{
    Asset, BytesError, DynAsset, DynOwnedAsset, IAssetAccessor, IAssetCacher, UpdateError,
    base::AssetCacheContent,
};

/// Describes how to read and write file asynchronously.
pub trait AsyncFs: Sync + 'static {
    /// Read a file to a boxed slice asynchronously.
    fn read_file<'a>(
        path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<Box<[u8]>>> + Send + 'a>>;
    /// Write a slice to a file asynchronously.
    ///
    /// Note: It should also create the necessary directories to get to that
    /// file.
    fn write_file<'a>(
        path: &'a Path,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + 'a>>;
}

/// An [`AsyncFs`] that just panics.
pub struct FalseAsyncFs;

impl AsyncFs for FalseAsyncFs {
    fn read_file<'a>(
        _path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<Box<[u8]>>> + Send + 'a>>
    {
        unimplemented!("FalseAsyncFs is intentionally unimplemented")
    }

    fn write_file<'a>(
        _path: &'a Path,
        _data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = std::io::Result<()>> + Send + 'a>> {
        unimplemented!("FalseAsyncFs is intentionally unimplemented")
    }
}

/// Simple accessor that reads assets from files.
pub struct FsAccessor<Fs: AsyncFs> {
    root: PathBuf,
    _fs: PhantomData<&'static Fs>,
}

impl<Fs: AsyncFs> FsAccessor<Fs> {
    /// Create a new `FsAccessor`
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            _fs: PhantomData,
        }
    }

    /// Returns the root path that asset paths are appended to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, asset_path: &str) -> Result<PathBuf, BytesError> {
        let path = Path::new(asset_path);
        if path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            Err(BytesError::BadPath(Some(
                "FsAccessor asset paths must be relative".to_string(),
            )))
        } else {
            Ok(self.root.join(path))
        }
    }
}

impl<Fs: AsyncFs> IAssetAccessor for FsAccessor<Fs> {
    fn load_bytes_async<'a>(
        &'a self,
        asset_path: &'a str,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Box<[u8]>, BytesError>> + Send + 'a>> {
        Box::pin(async move { Ok(Fs::read_file(&self.resolve(asset_path)?).await?) })
    }

    fn load_bytes_blocking(&self, asset_path: &str) -> Result<Box<[u8]>, BytesError> {
        Ok(std::fs::read(self.resolve(asset_path)?)?.into())
    }

    fn save_bytes_async<'a>(
        &'a self,
        asset_path: &'a str,
        bytes: &'a [u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), BytesError>> + Send + 'a>> {
        Box::pin(async move { Ok(Fs::write_file(&self.resolve(asset_path)?, bytes).await?) })
    }

    fn save_bytes_blocking(&self, asset_path: &str, bytes: &[u8]) -> Result<(), BytesError> {
        let path = self.resolve(asset_path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(std::fs::write(self.resolve(asset_path)?, bytes)?)
    }
}

/// A naive implementation of an [`IAssetCacher`].
pub struct NaiveCacher<T: Any + Send + Sync> {
    assets: FrozenMap<Arc<str>, Box<AssetCacheContent<T, ()>>>,
    intern: RwLock<HashSet<Arc<str>>>,
}

impl<T: Any + Send + Sync> NaiveCacher<T> {
    /// Returns an empty cache.
    pub fn new() -> Self {
        Self {
            assets: FrozenMap::new(),
            intern: RwLock::new(HashSet::new()),
        }
    }

    fn intern_str(&self, s: &str) -> Arc<str> {
        if let Some(arc) = self.intern.read().get(s) {
            return arc.clone();
        }

        let mut guard = self.intern.write();

        if let Some(arc) = guard.get(s) {
            arc.clone()
        } else {
            let arc: Arc<str> = s.into();
            guard.insert(arc.clone());
            arc
        }
    }

    fn intern_and_get(&self, asset_path: &str) -> (Arc<str>, &AssetCacheContent<T, ()>) {
        let path = self.intern_str(asset_path);
        (
            path.clone(),
            self.assets.insert_with(path, Default::default),
        )
    }
}

impl<T: Any + Send + Sync> IAssetCacher for NaiveCacher<T> {
    fn retrieve_asset_blocking(&self, asset_path: &str) -> Option<DynAsset> {
        let (path, inner) = self.intern_and_get(asset_path);
        inner
            .blocking_get_or_lock()
            .map(|(data, _)| Asset::from_parts(path, data, None).into_dyn())
    }

    fn retrieve_asset_async<'a>(
        &'a self,
        asset_path: &'a str,
    ) -> Pin<Box<dyn Future<Output = Option<DynAsset>> + Send + 'a>> {
        Box::pin(async move {
            let (path, inner) = self.intern_and_get(asset_path);
            inner
                .async_get_or_lock()
                .await
                .map(|(data, _)| Asset::from_parts(path, data, None).into_dyn())
        })
    }

    fn update_asset_unlocking(
        &self,
        asset: DynOwnedAsset,
    ) -> Result<DynAsset, (UpdateError, DynOwnedAsset)> {
        let (path, inner) = self.intern_and_get(asset.path());

        let data: Arc<T> = asset
            .downcast::<T>()
            .map_err(|d| (UpdateError::WrongType, d))?
            .into_inner()
            .into();

        inner.update_and_unlock(Some((data.clone(), ())));

        Ok(Asset::from_parts(path, data, None).into_dyn())
    }

    fn update_asset_blocking(
        &self,
        asset: DynOwnedAsset,
    ) -> Result<DynAsset, (UpdateError, DynOwnedAsset)> {
        let (path, inner) = self.intern_and_get(asset.path());

        let data: Arc<T> = asset
            .downcast::<T>()
            .map_err(|d| (UpdateError::WrongType, d))?
            .into_inner()
            .into();

        inner.blocking_wait_and_update(Some((data.clone(), ())));

        Ok(Asset::from_parts(path, data, None).into_dyn())
    }

    fn update_asset_async<'a>(
        &'a self,
        asset: DynOwnedAsset,
    ) -> Pin<Box<dyn Future<Output = Result<DynAsset, (UpdateError, DynOwnedAsset)>> + Send + 'a>>
    {
        Box::pin(async move {
            let (path, inner) = self.intern_and_get(asset.path());

            let data: Arc<T> = asset
                .downcast::<T>()
                .map_err(|d| (UpdateError::WrongType, d))?
                .into_inner()
                .into();

            inner.async_wait_and_update(Some((data.clone(), ()))).await;

            Ok(Asset::from_parts(path, data, None).into_dyn())
        })
    }

    fn release_asset_lock(&self, asset_path: &str) {
        let (_, inner) = self.intern_and_get(asset_path);
        inner.unlock();
    }

    fn caches(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<T>()
    }
}

impl<T: Any + Send + Sync> Default for NaiveCacher<T> {
    fn default() -> Self {
        Self::new()
    }
}
