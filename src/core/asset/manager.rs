//! The [`AssetManager`] and [`GlobalInterner`].

use std::{
    any::{Any, TypeId, type_name}, collections::{HashMap, HashSet, hash_map::Entry}, sync::{Arc},
};

use parking_lot::RwLock;
use thiserror::Error;

use crate::core::asset::traits::{
    BytesError, IntoCacher, LoadAssetError, SaveAssetError, SaverLoader, UpdateError,
};

use super::*;

#[derive(Debug, Error)]
/// Error returned by `AssetManager::get_asset_*`.
pub enum GetAssetError {
    #[error("load error: {0}")]
    /// Returned if there was an error in the [`IAssetLoader`].
    LoadError(#[from] LoadAssetError),
    #[error("read error: {0}")]
    /// Returned if there was an error in the [`IAssetAccessor`].
    BytesError(#[from] BytesError),
    #[error("failed to upload loaded value to cache; {0}")]
    /// Returned if there was an error uploading the new value to the [`IAssetCacher`].
    CacheError(#[from] UpdateError),
    #[error("no loader for asset type")]
    /// Returned if the loader for the asset type was not set
    NoLoader,
    #[error("mismatch between cached type and expected type")]
    /// Returned if the cached asset has a different type than the requested one.
    CachedMismatch,
}

#[derive(Debug, Error)]
/// Error returned by `AssetManager::set_asset_*`.
pub enum SetAssetError {
    #[error("save error: {0}")]
    /// Returned if there was an error in the [`IAssetSaver`].
    SaveError(#[from] SaveAssetError),
    #[error("write error: {0}")]
    /// Returned if there was an error in the [`IAssetAccessor`].
    BytesError(#[from] BytesError),
    #[error("failed to upload new value to cache; {0}")]
    /// Returned if there was an error uploading the new value to the [`IAssetCacher`].
    CacheError(#[from] UpdateError),
    #[error("no loader for asset type")]
    /// Returned if the saver for the asset type was not set
    NoSaver,
}

/// A global string interner for asset paths.
///
/// All asset paths are sent into this pool, reducing allocations application-wide. Y
pub struct Interner {
    inner: RwLock<HashSet<Arc<str>>>,
}

impl Interner {
    /// Intern a string, returning a shared [`Arc<str>`].
    ///
    /// If the string has been interned before, the existing [`Arc<str>`]
    /// is returned. Otherwise, a new allocation is made and stored.
    pub fn intern(&self, s: &str) -> Arc<str> {
        if let Some(arc) = self.inner.read().get(s) {
            return arc.clone();
        }

        let mut guard = self.inner.write();

        if let Some(arc) = guard.get(s) {
            arc.clone()
        } else {
            let arc: Arc<str> = s.into();
            guard.insert(arc.clone());
            arc
        }
    }

    fn new() -> Self {
        Self {
            inner: RwLock::new(HashSet::new())
        }
    }
}

/// An asset manager that can be used to save and load various assets.
pub struct AssetManager {
    loaders_cachers: HashMap<TypeId, (Box<dyn IAssetLoader>, Arc<dyn IAssetCacher>)>,
    savers: HashMap<TypeId, Box<dyn IAssetSaver>>,
    accessor: Box<dyn IAssetAccessor>,
    interner: Arc<Interner>,
}

const _: () = {
    const fn _assert_sync<T: Send + Sync>() {}
    _assert_sync::<AssetManager>();
};

impl AssetManager {
    /// Create a new [`AssetManagerBuilder`].
    ///
    /// # Example
    ///
    /// ```rust
    /// # use sticky_engine::core::asset::{*, manager::*, traits::*, storage::*};
    /// # use std::sync::Arc;
    /// # trait HasNew { fn new(i: Arc<Interner>) -> Self; }
    /// # fn _test<
    /// # FsAccessor: IAssetAccessor + Default,
    /// # AnyCacher: IAssetCacher + HasNew,
    /// # Texture: IAsset,
    /// # PngLoader: IAssetLoader + Default,
    /// # PngSaver: IAssetSaver + Default,
    /// # Config: IAsset,
    /// # ConfigSaverLoader: SaverLoader + Default,
    /// # >() {
    /// let mut builder = AssetManager::builder();
    /// let interner = builder.interner();
    /// builder
    ///     .with_accessor(FsAccessor::default())
    ///     .with_default_cacher(AnyCacher::new(interner));
    ///
    /// builder
    ///     .register_loader::<Texture>(PngLoader::default())
    ///     .register_saver::<Texture>(PngSaver::default());
    ///
    /// builder.register_saver_loader::<Config>(ConfigSaverLoader::default());
    ///
    /// let asset_manager: AssetManager = builder.build();
    /// # }
    /// ```
    pub fn builder() -> AssetManagerBuilder {
        AssetManagerBuilder {
            loaders: HashMap::new(),
            savers: HashMap::new(),
            cachers: HashMap::new(),
            default_cacher: None,
            accessor: None,
            interner: Arc::new(Interner::new()),
        }
    }

    pub(crate) fn get_asset_blocking_dyn<T: IAsset + Sized>(
        &self,
        asset_path: &Arc<str>,
    ) -> Result<DynAsset, GetAssetError> {
        let type_id = TypeId::of::<T>();
        let Some((loader, cacher)) = self.loaders_cachers.get(&type_id) else {
            return Err(GetAssetError::NoLoader);
        };

        // Check cache
        if let Some(cached) = cacher.retrieve_asset_blocking(asset_path) {
            if cached.is(type_id) {
                return Ok(cached);
            } else {
                return Err(GetAssetError::CachedMismatch);
            }
        }

        // Asset is locked
        struct AssetUnlocker<'a> {
            on_drop: Option<&'a (dyn Fn() + Send + Sync)>,
        }
        impl Drop for AssetUnlocker<'_> {
            fn drop(&mut self) {
                if let Some(on_drop) = self.on_drop.take() {
                    on_drop()
                }
            }
        }

        let release_closure = || cacher.release_asset_lock(asset_path);
        let mut asset_unlocker = AssetUnlocker {
            on_drop: Some(&release_closure),
        };

        let bytes = self.accessor.load_bytes_blocking(asset_path)?;

        let loaded = loader.load_from_bytes(asset_path, &bytes)?;

        let mut loaded = match loaded.downcast::<T>() {
            Ok(asset) => *asset,
            Err(_) => panic!("loader returned the wrong type"),
        };

        loaded.resolve_blocking(self)?;

        let owned = DynOwnedAsset::from_owned(OwnedAsset::new(asset_path.as_ref(), loaded));

        let asset = match cacher.update_asset_unlocking(owned) {
            Ok(asset) => asset,
            Err((e, _)) => return Err(e.into()),
        };

        asset_unlocker.on_drop = None;

        Ok(asset)
    }

    /// Get an asset from cache or do a blocking load from disk.
    pub fn get_asset_blocking<T: IAsset + Sized>(
        &self,
        asset_path: &str,
    ) -> Result<Asset<T>, GetAssetError> {
        let path = self.interner.intern(asset_path);
        let asset = self.get_asset_blocking_dyn::<T>(&path)?;

        let Ok(asset) = asset.downcast() else {
            panic!("loader or cacher returned the wrong type");
        };

        Ok(asset)
    }

    pub(crate) async fn get_asset_async_dyn<T: IAsset + Sized>(
        &self,
        asset_path: &Arc<str>,
    ) -> Result<DynAsset, GetAssetError> {
        let type_id = TypeId::of::<T>();
        let Some((loader, cacher)) = self.loaders_cachers.get(&type_id) else {
            return Err(GetAssetError::NoLoader);
        };

        // Check cache
        if let Some(cached) = cacher.retrieve_asset_async(asset_path).await {
            if cached.is(type_id) {
                return Ok(cached);
            } else {
                return Err(GetAssetError::CachedMismatch);
            }
        }

        // Asset is locked
        struct AssetUnlocker<'a> {
            on_drop: Option<&'a (dyn Fn() + Send + Sync)>,
        }
        impl Drop for AssetUnlocker<'_> {
            fn drop(&mut self) {
                if let Some(on_drop) = self.on_drop.take() {
                    on_drop()
                }
            }
        }

        let release_closure = || cacher.release_asset_lock(asset_path);
        let mut asset_unlocker = AssetUnlocker {
            on_drop: Some(&release_closure),
        };

        let bytes = self.accessor.load_bytes_async(asset_path).await?;

        let loaded = loader.load_from_bytes(asset_path, &bytes)?;

        let mut loaded = match loaded.downcast::<T>() {
            Ok(asset) => *asset,
            Err(_) => panic!("loader returned the wrong type"),
        };

        loaded.resolve_async(self).await?;

        let owned = DynOwnedAsset::from_owned(OwnedAsset::new(asset_path.as_ref(), loaded));

        let asset = match cacher.update_asset_unlocking(owned) {
            Ok(asset) => asset,
            Err((e, _)) => return Err(e.into()),
        };

        asset_unlocker.on_drop = None;

        Ok(asset)
    }

    /// Get an asset from cache or do an async load from disk.
    pub async fn get_asset_async<T: IAsset + Sized>(
        &self,
        asset_path: &str,
    ) -> Result<Asset<T>, GetAssetError> {
        let path = self.interner.intern(asset_path);
        let asset = self.get_asset_async_dyn::<T>(&path).await?;

        let Ok(asset) = asset.downcast() else {
            panic!("loader or cacher returned the wrong type");
        };

        Ok(asset)
    }

    /// Set and save an asset in cache and on disk, blocking until completion.
    pub fn set_asset_blocking<T: IAsset + Sized>(
        &self,
        asset: OwnedAsset<T>,
    ) -> Result<Asset<T>, SetAssetError> {
        let Some(saver) = self.savers.get(&TypeId::of::<T>()) else {
            return Err(SetAssetError::NoSaver);
        };

        let asset = if let Some((_, cacher)) = self.loaders_cachers.get(&TypeId::of::<T>()) {
            let asset = match cacher.update_asset_blocking(DynOwnedAsset::from_owned(asset)) {
                Ok(asset) => asset,
                Err((e, _)) => return Err(e.into()),
            };

            let asset: Asset<T> = match asset.downcast() {
                Ok(asset) => asset,
                Err(_) => panic!("cacher returned the wrong type"),
            };

            asset
        } else {
            let path = self.interner.intern(asset.path());

            Asset::new_resolved(path, asset.into_inner().into(), None)
        };

        let path = asset.path_arc();
        let bytes = saver.save_as_bytes(path, asset.as_ref())?;

        self.accessor.save_bytes_blocking(path, &bytes)?;

        Ok(asset)
    }

    /// Set and save an asset in cache and on disk, asynchronously.
    pub async fn set_asset_async<T: IAsset + Sized>(
        &self,
        asset: OwnedAsset<T>,
    ) -> Result<Asset<T>, SetAssetError> {
        let Some(saver) = self.savers.get(&TypeId::of::<T>()) else {
            return Err(SetAssetError::NoSaver);
        };

        let asset = if let Some((_, cacher)) = self.loaders_cachers.get(&TypeId::of::<T>()) {
            let asset = match cacher
                .update_asset_async(DynOwnedAsset::from_owned(asset))
                .await
            {
                Ok(asset) => asset,
                Err((e, _)) => return Err(e.into()),
            };

            let asset: Asset<T> = match asset.downcast() {
                Ok(asset) => asset,
                Err(_) => panic!("cacher returned the wrong type"),
            };

            asset
        } else {
            let path = self.interner.intern(asset.path());

            Asset::new_resolved(path, asset.into_inner().into(), None)
        };

        let path = asset.path_arc();
        let bytes = saver.save_as_bytes(path, asset.as_ref())?;

        self.accessor.save_bytes_async(path, &bytes).await?;

        Ok(asset)
    }
}

/// Builder for an [`AssetManager`].
///
/// It is created through [`AssetManager::builder`].
pub struct AssetManagerBuilder {
    loaders: HashMap<TypeId, Box<dyn IAssetLoader>>,
    savers: HashMap<TypeId, Box<dyn IAssetSaver>>,
    cachers: HashMap<TypeId, Arc<dyn IAssetCacher>>,
    default_cacher: Option<Arc<dyn IAssetCacher>>,
    accessor: Option<Box<dyn IAssetAccessor>>,
    interner: Arc<Interner>,
}

impl AssetManagerBuilder {
    /// Register the accessor for the [`AssetManager`].
    ///
    /// You may only register one accessor for the entire manager. Trying to
    /// register a second will cause a panic.
    pub fn with_accessor(&mut self, accessor: impl IAssetAccessor) -> &mut Self {
        debug_assert!(self.accessor.is_none(), "multiple accessors added");
        self.accessor = Some(Box::new(accessor));
        self
    }

    /// Register the default cacher for the [`AssetManager`].
    ///
    /// You may only register one default cacher for the entire manager. Trying to
    /// register a second will cause a panic.
    ///
    /// The default cacher will be cloned for any type that has a loader but no
    /// cacher.
    pub fn with_default_cacher(&mut self, default_cacher: impl IntoCacher) -> &mut Self {
        debug_assert!(
            self.default_cacher.is_none(),
            "multiple default cachers added"
        );
        self.default_cacher = Some(default_cacher.into_cacher());
        self
    }

    /// Register a loader for the given asset type.
    ///
    /// You may only register one loader per asset type. If you attempt to
    /// provide a second, this will panic.
    ///
    /// There is no default loader. You must register a loader for all assets
    /// you use to avoid a panic, including engine assets.
    pub fn register_loader<T: Any + Send + Sync>(
        &mut self,
        loader: impl IAssetLoader,
    ) -> &mut Self {
        let loader = Box::new(loader);
        let type_id = TypeId::of::<T>();

        if !loader.loads(type_id) {
            panic!(
                "loader for {} does not support loading that type",
                type_name::<T>()
            );
        }

        match self.loaders.entry(type_id) {
            Entry::Occupied(_) => panic!("loader for {} is already set", type_name::<T>()),
            Entry::Vacant(v) => v.insert(loader),
        };

        self
    }

    /// Register a saver for the given asset type.
    ///
    /// You may only register one saver per asset type. If you attempt to
    /// provide a second, this will panic.
    pub fn register_saver<T: Any + Send + Sync>(&mut self, saver: impl IAssetSaver) -> &mut Self {
        let saver = Box::new(saver);
        let type_id = TypeId::of::<T>();

        if !saver.saves(type_id) {
            panic!(
                "saver for {} does not support saving that type",
                type_name::<T>()
            );
        }

        match self.savers.entry(type_id) {
            Entry::Occupied(_) => panic!("saver for {} is already set", type_name::<T>()),
            Entry::Vacant(v) => v.insert(saver),
        };

        self
    }

    /// Register a cacher for the given asset type.
    ///
    /// You may only register one cacher per asset type. If you attempt to
    /// provide a second, this will panic.
    pub fn register_cacher<T: Any + Send + Sync>(&mut self, cacher: impl IntoCacher) -> &mut Self {
        let cacher = cacher.into_cacher();
        let type_id = TypeId::of::<T>();

        if !cacher.caches(type_id) {
            panic!(
                "cacher for {} does not support saving that type",
                type_name::<T>()
            );
        }

        if !cacher.uses_interner(&self.interner) {
            panic!(
                "cacher for {} does not use the same interner as the asset manager",
                type_name::<T>()
            );
        }

        match self.cachers.entry(type_id) {
            Entry::Occupied(_) => panic!("cacher for {} is already set", type_name::<T>()),
            Entry::Vacant(v) => v.insert(cacher),
        };

        self
    }

    /// Register a loader and saver from a value that is both.
    pub fn register_saver_loader<T: Any + Send + Sync>(
        &mut self,
        saver_loader: impl SaverLoader,
    ) -> &mut Self {
        let (saver, loader) = saver_loader.split();
        self.register_loader::<T>(loader).register_saver::<T>(saver)
    }

    /// Register both a loader and a cacher.
    pub fn register_loader_cacher<T: Any + Send + Sync>(
        &mut self,
        loader: impl IAssetLoader,
        cacher: impl IntoCacher,
    ) -> &mut Self {
        self.register_loader::<T>(loader)
            .register_cacher::<T>(cacher)
    }

    /// Register a loader, cacher, and saver all at once
    pub fn register_all<T: Any + Send + Sync>(
        &mut self,
        loader: impl IAssetLoader,
        cacher: impl IntoCacher,
        saver: impl IAssetSaver,
    ) -> &mut Self {
        self.register_loader::<T>(loader)
            .register_cacher::<T>(cacher)
            .register_saver::<T>(saver)
    }

    /// Build an [`AssetManager`].
    ///
    /// You must have set an accessor, or else this function will panic.
    ///
    /// Every loader must have a cacher, and vice versa, or else this function
    /// will panic.
    pub fn build(mut self) -> AssetManager {
        let accessor = self.accessor.expect("no accessor set");
        let loaders_cachers = self
            .loaders
            .into_iter()
            .map(|(type_id, l)| {
                // Explicit cacher
                if let Some(cacher) = self.cachers.remove(&type_id) {
                    (type_id, (l, cacher))
                } else if let Some(default_cacher) = self.default_cacher.as_ref() {
                    if !default_cacher.caches(type_id) {
                        panic!(
                            "default cacher does not cache {type_id:?}, \
                            which has a loader but no explicit cacher"
                        );
                    }

                    (type_id, (l, default_cacher.clone()))
                } else {
                    panic!("no cacher for loader for {type_id:?}");
                }
            })
            .collect();

        if !self.cachers.is_empty() {
            panic!("there are cachers without loaders");
        }

        AssetManager {
            loaders_cachers,
            savers: self.savers,
            accessor,
            interner: self.interner
        }
    }

    /// Returns a clone of the [`Interner`] that should be used for creating caches.
    pub fn interner(&self) -> Arc<Interner> {
        self.interner.clone()
    }
}
