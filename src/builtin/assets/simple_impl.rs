//! Simple utilities for quick-and-dirty asset management.
//!
//! See the [`asset`](crate::core::asset) module.
//!
//! This module contains 3 tools:
//!
//! - [`FsAccessor`]: an [`IAssetAccessor`] that reads from disk.
//!
//! - [`NaiveCacher`]: an [`IAssetCacher`] that caches every value that goes in
//!   permanently.
//!
//! - [`ExtensionSwitcher`]: switches between any number of loaders/savers based
//!   on the asset path.

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
    path::{Component, Path, PathBuf},
    pin::Pin,
    sync::Arc,
};

use elsa::sync::FrozenMap;

use crate::core::asset::{
    base::AssetCacheContent,
    traits::{BytesError, LoadAssetError, SaveAssetError, SaverLoader, UpdateError},
    *,
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

/// An [`AsyncFs`] that isn't implemented.
/// 
/// This panics if any method of `AsyncFs` is called. It is highly discouraged
/// for production code *unless* you are *certain* no async is ever used.
pub struct FalseAsyncFs;

impl AsyncFs for FalseAsyncFs {
    fn read_file<'a>(
        _path: &'a Path,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<Box<[u8]>>> + Send + 'a>> {
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

    fn resolve(&self, asset_path: &Arc<str>) -> Result<PathBuf, BytesError> {
        let path = Path::new(asset_path.as_ref());
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
        asset_path: &'a Arc<str>,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Box<[u8]>, BytesError>> + Send + 'a>> {
        Box::pin(async move { Ok(Fs::read_file(&self.resolve(asset_path)?).await?) })
    }

    fn load_bytes_blocking(&self, asset_path: &Arc<str>) -> Result<Box<[u8]>, BytesError> {
        Ok(std::fs::read(self.resolve(asset_path)?)?.into())
    }

    fn save_bytes_async<'a>(
        &'a self,
        asset_path: &'a Arc<str>,
        bytes: &'a [u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), BytesError>> + Send + 'a>> {
        Box::pin(async move { Ok(Fs::write_file(&self.resolve(asset_path)?, bytes).await?) })
    }

    fn save_bytes_blocking(&self, asset_path: &Arc<str>, bytes: &[u8]) -> Result<(), BytesError> {
        let path = self.resolve(asset_path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(std::fs::write(self.resolve(asset_path)?, bytes)?)
    }
}

/// A naive implementation of an [`IAssetCacher`].
pub struct NaiveCacher<T: IAsset> {
    assets: FrozenMap<Arc<str>, Box<AssetCacheContent<T, ()>>>,
    interner: Arc<Interner>,
}

impl<T: IAsset> NaiveCacher<T> {
    /// Returns an empty cache.
    pub fn new(interner: Arc<Interner>) -> Self {
        Self {
            assets: FrozenMap::new(),
            interner,
        }
    }

    fn get_or_insert(&self, asset_path: &Arc<str>) -> (Arc<str>, &AssetCacheContent<T, ()>) {
        let inner = self
            .assets
            .insert_with(asset_path.clone(), Default::default);
        (asset_path.clone(), inner)
    }
}

impl<T: IAsset> IAssetCacher for NaiveCacher<T> {
    fn retrieve_asset_blocking(&self, asset_path: &Arc<str>) -> Option<DynAsset> {
        let (path, inner) = self.get_or_insert(asset_path);
        inner
            .blocking_get_or_lock()
            .map(|(data, _)| Asset::new_resolved(path, data, None).into_dyn())
    }

    fn retrieve_asset_async<'a>(
        &'a self,
        asset_path: &'a Arc<str>,
    ) -> Pin<Box<dyn Future<Output = Option<DynAsset>> + Send + 'a>> {
        Box::pin(async move {
            let (path, inner) = self.get_or_insert(asset_path);
            inner
                .async_get_or_lock()
                .await
                .map(|(data, _)| Asset::new_resolved(path, data, None).into_dyn())
        })
    }

    fn update_asset_unlocking(
        &self,
        asset: DynOwnedAsset,
    ) -> Result<DynAsset, (UpdateError, DynOwnedAsset)> {
        let (path, inner) = self.get_or_insert(&self.interner.intern(asset.path()));

        let data: Arc<T> = asset
            .downcast::<T>()
            .map_err(|d| (UpdateError::WrongType, d))?
            .into_inner()
            .into();

        inner.update_and_unlock(Some((data.clone(), ())));

        Ok(Asset::new_resolved(path, data, None).into_dyn())
    }

    fn update_asset_blocking(
        &self,
        asset: DynOwnedAsset,
    ) -> Result<DynAsset, (UpdateError, DynOwnedAsset)> {
        let (path, inner) = self.get_or_insert(&self.interner.intern(asset.path()));

        let data: Arc<T> = asset
            .downcast::<T>()
            .map_err(|d| (UpdateError::WrongType, d))?
            .into_inner()
            .into();

        inner.blocking_wait_and_update(Some((data.clone(), ())));

        Ok(Asset::new_resolved(path, data, None).into_dyn())
    }

    fn update_asset_async<'a>(
        &'a self,
        asset: DynOwnedAsset,
    ) -> Pin<Box<dyn Future<Output = Result<DynAsset, (UpdateError, DynOwnedAsset)>> + Send + 'a>>
    {
        Box::pin(async move {
            let (path, inner) = self.get_or_insert(&self.interner.intern(asset.path()));

            let data: Arc<T> = asset
                .downcast::<T>()
                .map_err(|d| (UpdateError::WrongType, d))?
                .into_inner()
                .into();

            inner.async_wait_and_update(Some((data.clone(), ()))).await;

            Ok(Asset::new_resolved(path, data, None).into_dyn())
        })
    }

    fn release_asset_lock(&self, asset_path: &Arc<str>) {
        let (_, inner) = self.get_or_insert(asset_path);
        inner.unlock();
    }

    fn caches(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<T>()
    }

    fn uses_interner(&self, other: &Arc<Interner>) -> bool {
        Arc::ptr_eq(&self.interner, other)
    }
}

/// A saver/loader that switches between several others based on the extension
/// of the asset path.
///
/// Take an image, for example. You can load and save it both as PNG or BMP.
pub struct ExtensionSwitcher<T: IAsset> {
    loader_by_ext: HashMap<String, Box<dyn IAssetLoader>>,
    saver_by_ext: HashMap<String, Box<dyn IAssetSaver>>,
    _marker: PhantomData<T>,
}

impl<T: IAsset> ExtensionSwitcher<T> {
    /// Create an empty list of loaders and switchers
    pub fn new() -> Self {
        Self {
            loader_by_ext: HashMap::new(),
            saver_by_ext: HashMap::new(),
            _marker: PhantomData,
        }
    }

    /// Add a new loader.
    ///
    /// Panics if the given extension already has a loader set or if the loader
    /// doesn't load the given type.
    pub fn add_loader(&mut self, ext: impl Into<String>, loader: impl IAssetLoader) {
        let ext = ext.into();
        if self.loader_by_ext.contains_key(&ext) {
            panic!("loader for `{ext}` already exists");
        }

        if !loader.loads(TypeId::of::<T>()) {
            panic!("loader for `{ext}` does not load the expected type");
        }

        self.loader_by_ext.insert(ext, Box::new(loader));
    }

    /// Add a new saver.
    ///
    /// Panics if the given extension already has a saver set or if the saver
    /// doesn't save the given type.
    pub fn add_saver(&mut self, ext: impl Into<String>, saver: impl IAssetSaver) {
        let ext = ext.into();
        if self.saver_by_ext.contains_key(&ext) {
            panic!("saver for `{ext}` already exists");
        }

        if !saver.saves(TypeId::of::<T>()) {
            panic!("saver for `{ext}` does not save the expected type");
        }

        self.saver_by_ext.insert(ext, Box::new(saver));
    }

    /// Add both a loader and a saver.
    ///
    /// See [`Self::add_loader`] and [`Self::add_saver`] for important details.
    pub fn add_loader_saver(
        &mut self,
        ext: impl Into<String>,
        loader: impl IAssetLoader,
        saver: impl IAssetSaver,
    ) {
        let ext = ext.into();
        self.add_loader(ext.clone(), loader);
        self.add_saver(ext, saver);
    }
}

impl<T: IAsset> SaverLoader for ExtensionSwitcher<T> {
    fn split(self) -> (Self, Self) {
        (
            Self {
                loader_by_ext: HashMap::new(),
                saver_by_ext: self.saver_by_ext,
                _marker: PhantomData,
            },
            Self {
                loader_by_ext: self.loader_by_ext,
                saver_by_ext: HashMap::new(),
                _marker: PhantomData,
            },
        )
    }
}

impl<T: IAsset> IAssetSaver for ExtensionSwitcher<T> {
    fn save_as_bytes(
        &self,
        asset_path: &Arc<str>,
        value: &dyn Any,
    ) -> Result<Box<[u8]>, SaveAssetError> {
        let value: &T = value.downcast_ref().ok_or(SaveAssetError::IncorrectType)?;

        let ext = Path::new(asset_path.as_ref())
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| {
                SaveAssetError::BadPath(format!("`{asset_path}` has an invalid extension"))
            })?;

        let saver = self.saver_by_ext.get(ext).ok_or_else(|| {
            SaveAssetError::BadPath(format!("`{ext}` is not a tracked extension"))
        })?;

        saver.save_as_bytes(asset_path, value)
    }

    fn saves(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<T>()
    }
}

impl<T: IAsset> IAssetLoader for ExtensionSwitcher<T> {
    fn load_from_bytes(
        &self,
        asset_path: &Arc<str>,
        bytes: &[u8],
    ) -> Result<Box<dyn Any>, LoadAssetError> {
        let ext = Path::new(asset_path.as_ref())
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| {
                LoadAssetError::BadPath(format!("`{asset_path}` has an invalid extension"))
            })?;

        let loader = self.loader_by_ext.get(ext).ok_or_else(|| {
            LoadAssetError::BadPath(format!("`{ext}` is not a tracked extension"))
        })?;

        loader.load_from_bytes(asset_path, bytes)
    }

    fn loads(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<T>()
    }
}

impl<T: IAsset> Default for ExtensionSwitcher<T> {
    fn default() -> Self {
        Self::new()
    }
}
