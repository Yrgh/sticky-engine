use std::{any::{Any, TypeId}, sync::Arc};

/// Metadata from the cache, stored in an [`Asset`]. 
/// 
/// It can respond to events like individual clones/drops, and will be dropped
/// when the asset is no longer in use.
pub trait AssetMetadata: Send + Sync + 'static {
    /// Called when the [`Asset`] is cloned.
    fn on_clone_one(&self);

    /// Called when the **individual** [`Asset`] is dropped.
    fn on_drop_one(&self);
}

// Wrapper so we can decide what happens during clone and drop without modifying
// Asset's clone and drop.

#[repr(transparent)]
struct Metadata {
    inner: Arc<dyn AssetMetadata>
}

impl Clone for Metadata {
    fn clone(&self) -> Self {
        self.inner.on_clone_one();

        Self {
            inner: self.inner.clone()
        }
    }
}

impl Drop for Metadata {
    fn drop(&mut self) {
        self.inner.on_drop_one();
    }
}

/// Alias to reduce the verbosity of writing an `Asset`.
pub type DynAsset = Asset<dyn Any + Send + Sync>;

/// A reference-counted asset.
///
/// Assets can be clone freely, as they behave much like an [`Arc`].
pub struct Asset<T: Any + ?Sized + Send + Sync> {
    path: Arc<str>,
    data: Arc<T>,
    tracker: Option<Metadata>,
}

impl<T: Any + ?Sized + Send + Sync> Asset<T> {
    /// Construct a new `Asset` from its parts.
    ///
    /// It is only recommended to use this function in your [`IAssetCacher`].
    ///
    /// `path` will be compared via pointer equality, so your cacher must use
    /// some form of string interning. The same `path` and `generation` must
    /// point to the same `data`.
    ///
    /// `tracker` will be stored and cloned with the asset and be dropped when
    /// all original assets are destroyed. You can use it to set up a
    /// notification system for your cache, for example.
    pub fn from_parts(
        path: Arc<str>,
        data: Arc<T>,
        tracker: Option<Arc<dyn AssetMetadata>>,
    ) -> Self {
        Self {
            path,
            data,
            tracker: tracker.map(|inner| Metadata { inner }),
        }
    }

    /// Returns the path this `Asset` matches.
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl<T: Any + Sized + Send + Sync> Asset<T> {
    /// Converts this asset into an [`OwnedAsset`] for writing or switching paths.
    pub fn into_owned(self) -> OwnedAsset<T>
    where
        T: Clone,
    {
        OwnedAsset {
            path: self.path.as_ref().to_owned(),
            data: Arc::unwrap_or_clone(self.data),
        }
    }

    /// Returns the corresponding [`DynAsset`].
    pub fn into_dyn(self) -> DynAsset {
        DynAsset {
            path: self.path,
            data: self.data,
            tracker: self.tracker,
        }
    }
}

impl DynAsset {
    /// Downcast the asset
    pub fn downcast<T: Any + Sized + Send + Sync>(self) -> Result<Asset<T>, Self> {
        match self.data.downcast() {
            Ok(data) => Ok(Asset {
                path: self.path,
                data,
                tracker: self.tracker,
            }),
            Err(data) => Err(Self {
                path: self.path,
                data,
                tracker: self.tracker,
            }),
        }
    }
}

/// Cloning is **shallow** due to reference counting.
impl<T: Any + ?Sized + Send + Sync> Clone for Asset<T> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            data: self.data.clone(),
            tracker: self.tracker.clone(),
        }
    }
}

impl<T: Any + ?Sized + Send + Sync> std::ops::Deref for Asset<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T: Any + ?Sized + Send + Sync> AsRef<T> for Asset<T> {
    fn as_ref(&self) -> &T {
        &self.data
    }
}

impl<T: Any + ?Sized + Send + Sync> PartialEq for Asset<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }
}

impl<T: Any + ?Sized + Send + Sync> Eq for Asset<T> {}

impl<T: std::hash::Hash + Any + ?Sized + Send + Sync> std::hash::Hash for Asset<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.data.as_ref().hash(state);
    }
}

impl<T: std::fmt::Debug + Any + ?Sized + Send + Sync> std::fmt::Debug for Asset<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("`{}`: ", self.path))?;
        self.data.as_ref().fmt(f)
    }
}

impl<T: std::fmt::Display + Any + ?Sized + Send + Sync> std::fmt::Display for Asset<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.data.as_ref().fmt(f)
    }
}

#[cfg(feature = "nightly")]
mod nightly {
    use std::{marker::Unsize, ops::CoerceUnsized};

    use super::*;

    impl<T: Unsize<U> + ?Sized + Any + Send + Sync, U: ?Sized + Any + Send + Sync>
        CoerceUnsized<Asset<U>> for Asset<T>
    {
    }
}

/// Owned asset, including its path.
///
/// Unlike [`Asset`], `OwnedAsset` owns its own asset, and doesn't use an
/// interned string. This allows you to mutate the inner value.
pub struct OwnedAsset<T: Any + Send + Sync> {
    path: String,
    data: T,
}

impl<T: Any + Send + Sync> OwnedAsset<T> {
    /// Create an entirely new `OwnedAsset`.
    pub fn new(path: impl ToString, data: T) -> Self {
        Self {
            path: path.to_string(),
            data,
        }
    }

    /// Returns a reference to this type's path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns a mutable reference to this type's path.
    pub fn path_mut(&mut self) -> &mut String {
        &mut self.path
    }

    /// Transform this owned asset into another owned asset.
    pub fn into_other<U>(self) -> OwnedAsset<U>
    where
        T: Into<U>,
        U: Any + Send + Sync,
    {
        OwnedAsset {
            path: self.path,
            data: self.data.into(),
        }
    }

    /// Convert this into the raw value, dropping the path.
    pub fn into_inner(self) -> T {
        self.data
    }
}

impl<T: Any + Send + Sync> std::ops::Deref for OwnedAsset<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T: Any + Send + Sync> std::ops::DerefMut for OwnedAsset<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl<T: Any + Send + Sync> AsRef<T> for OwnedAsset<T> {
    fn as_ref(&self) -> &T {
        &self.data
    }
}

impl<T: Any + Send + Sync> AsMut<T> for OwnedAsset<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

/// Unlike [`Asset`], `OwnedAsset` performs a **deep** clone.
impl<T: Any + Clone + Send + Sync> Clone for OwnedAsset<T> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            data: self.data.clone(),
        }
    }
}

trait OwnedAssetTr: Any + Send + Sync {
    fn expected_type(&self) -> TypeId;
    fn path(&self) -> &str;
}

impl<T: Any + Send + Sync> OwnedAssetTr for OwnedAsset<T> {
    fn expected_type(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn path(&self) -> &str {
        self.path()
    }
}

/// Type-erased owned asset
pub struct DynOwnedAsset {
    asset: Box<dyn OwnedAssetTr>,
}

impl DynOwnedAsset {
    /// Convert an owned asset
    pub fn from_owned<T: Any + Send + Sync>(asset: OwnedAsset<T>) -> Self {
        Self {
            asset: Box::new(asset),
        }
    }

    /// Try to get a specific owned asset.
    pub fn downcast<T: Any + Sized + Send + Sync>(self) -> Result<OwnedAsset<T>, Self> {
        if self.asset.expected_type() == TypeId::of::<T>() {
            Ok(*(self.asset as Box<dyn Any>)
                .downcast()
                .expect("expected type should indicate a valid downcast"))
        } else {
            Err(Self { asset: self.asset })
        }
    }

    /// Returns the path this asset owns
    pub fn path(&self) -> &str {
        self.asset.path()
    }
}