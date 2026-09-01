//! [`IAsset`] and its containers: [`Asset`] and [`OwnedAsset`].

use std::{
    any::{Any, TypeId},
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
};

use event_listener::{Event, Listener};

use crate::core::asset::{AssetManager, manager::GetAssetError};

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
    inner: Arc<dyn AssetMetadata>,
}

impl Clone for Metadata {
    fn clone(&self) -> Self {
        self.inner.on_clone_one();

        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for Metadata {
    fn drop(&mut self) {
        self.inner.on_drop_one();
    }
}

/// Trait for all assets.
/// 
/// Inside your implementation, you should called the corresponding `resolve_*`
/// on all [`Asset`]s inside. For example:
/// 
/// ```rust
/// # use sticky_engine::core::asset::{*, manager::*, storage::*};
/// pub struct Texture {
///     // ...
/// }
/// 
/// # impl AutoAsset for Texture {}
/// 
/// pub struct Material {
///     texture: Asset<Texture>,
/// }
/// 
/// impl IAsset for Material {
///     fn resolve_blocking(&mut self, asset_manager: &AssetManager) -> Result<(), GetAssetError> {
///         self.texture.resolve_blocking(asset_manager)?;
///         Ok(())
///     }
/// 
///     fn resolve_async<'a>(
///         &'a mut self,
///         asset_manager: &'a AssetManager
///     ) -> BoxedFuture<'a, Result<(), GetAssetError>> {
///         Box::pin(async {
///             self.texture.resolve_async(asset_manager).await?;
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait IAsset: Any + Send + Sync {
    /// Find all unresolved [`Asset`]s contained within and resolve them,
    /// blocking until completion.
    /// 
    /// This function is automatically called during [`AssetManager::get_asset_blocking`]
    fn resolve_blocking(&mut self, asset_manager: &AssetManager) -> Result<(), GetAssetError>;

    /// Find all unresolved [`Asset`]s contained within and resolve them
    /// asynchronously.
    /// 
    /// This function is automatically called during [`AssetManager::get_asset_async`]
    fn resolve_async<'a>(
        &'a mut self,
        asset_manager: &'a AssetManager,
    ) -> BoxedFuture<'a, Result<(), GetAssetError>>;
}

impl dyn IAsset {
    /// Attempt to downcast to a specific asset type.
    pub fn downcast<T: IAsset + Sized>(self: Arc<Self>) -> Result<Arc<T>, Arc<Self>> {
        match (self.clone() as Arc<dyn Any + Send + Sync>).downcast() {
            Ok(data) => Ok(data),
            Err(_) => Err(self),
        }
    }
}

/// Alias for [`Asset<dyn IAsset>`].
pub type DynAsset = Asset<dyn IAsset>;

/// Alias for a `Pin<Box<dyn Future>>`
pub type BoxedFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
type ResolveResult = Result<DynAsset, GetAssetError>;

type ResolveBlocking = dyn Fn(&Arc<str>, &AssetManager) -> ResolveResult + Send + Sync;
type ResolveAsync =
    dyn for<'a> Fn(&'a Arc<str>, &'a AssetManager) -> BoxedFuture<'a, ResolveResult> + Send + Sync;

enum ResolveState {
    Unresolved(Box<ResolveBlocking>, Box<ResolveAsync>),
    Resolving,
    Resolved(Arc<Inner>),
    AlwaysResolved,
}

struct Inner {
    state: parking_lot::Mutex<ResolveState>,
    // Emitted when state goes from Resolving to something else
    state_fall: Event,
    type_id: TypeId,
    data: Option<(Arc<dyn IAsset>, Option<Metadata>)>,
}

const _: () = {
    const fn _verify_share<T: Send + Sync>() {}
    _verify_share::<Arc<Inner>>();
    _verify_share::<DynAsset>();
};

/// A stored asset.
///
/// `Asset` behaves largely like an [`Arc`]. It can be cloned for cheap and uses
/// reference-counting to drop the value, however it comes with some special
/// properties.
///
/// Each `Asset` has a [path](Self::path()) it belongs to. This does not mean
/// all `Asset`s with the same path are equal, since they may be acquired at
/// different times.
///
/// Each `Asset` *can* also have a piece of metadata from the
/// [cacher](super::IAssetCacher) that tracks its lifecycle.
///
/// There are two possible states: resolved and unresolved.
///
/// A resolved asset behaves as you expect, it derefs to `T`. Trying to deref an
/// unresolved asset results in a panic.
///
///
pub struct Asset<T: IAsset + ?Sized> {
    path: Arc<str>,
    inner: Arc<Inner>,
    cached: Option<(Arc<T>, Option<Metadata>)>,
    _marker: PhantomData<fn() -> T>,
}

impl<T: IAsset + ?Sized> Asset<T> {
    /// Returns the associate path of this asset.
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl<T: IAsset + Sized> Asset<T> {
    /// Create a new, already-resolved asset.
    pub fn new_resolved(
        path: Arc<str>,
        value: Arc<T>,
        metadata: Option<Arc<dyn AssetMetadata>>,
    ) -> Self {
        Self {
            path,
            inner: Arc::new(Inner {
                state: parking_lot::Mutex::new(ResolveState::AlwaysResolved),
                state_fall: Event::new(),
                type_id: TypeId::of::<T>(),
                data: Some((
                    value.clone(),
                    metadata.clone().map(|inner| Metadata { inner }),
                )),
            }),
            // Create metadata twice instead of cloning because we don't want to
            // signal on_clone_one.
            cached: Some((value, metadata.map(|inner| Metadata { inner }))),
            _marker: PhantomData,
        }
    }

    /// Create an asset that must be resolved later.
    pub fn new_unresolved(path: Arc<str>) -> Self {
        Self {
            path,
            inner: Arc::new(Inner {
                state: parking_lot::Mutex::new(ResolveState::Unresolved(
                    Box::new(|p, am| am.get_asset_blocking_dyn::<T>(p)),
                    Box::new(|p, am| Box::pin(am.get_asset_async_dyn::<T>(p))),
                )),
                state_fall: Event::new(),
                type_id: TypeId::of::<T>(),
                data: None,
            }),
            cached: None,
            _marker: PhantomData,
        }
    }

    /// Erase the type of this asset.
    pub fn into_dyn(self) -> Asset<dyn IAsset> {
        Asset {
            path: self.path,
            inner: self.inner,
            cached: self.cached.map(|c| (c.0 as Arc<dyn IAsset>, c.1)),
            _marker: PhantomData,
        }
    }

    /// Fetch the contents of the asset from the [`AssetManager`], blocking until completion.
    ///
    /// You must call this on an `Asset` that was created with
    /// [`new_unresolved`](Self::new_unresolved) and hasn't been resolved yet.
    ///
    /// Each clone resolves independently.
    pub fn resolve_blocking(&mut self, asset_manager: &AssetManager) -> Result<(), GetAssetError> {
        debug_assert!(
            !matches!(*self.inner.state.lock(), ResolveState::AlwaysResolved),
            "double resolve"
        );
        debug_assert!(self.cached.is_none(), "double resolve");

        let (blocking_fn, async_fn) = {
            loop {
                let listener = self.inner.state_fall.listen();

                {
                    let mut guard = self.inner.state.lock();

                    if let ResolveState::Resolved(to_clone) = &*guard {
                        let Some((value, meta)) = to_clone.data.as_ref() else {
                            panic!("asset was resolved but not updated");
                        };

                        let value = match value.clone().downcast() {
                            Ok(value) => value,
                            Err(_) => panic!("resolver returned the wrong type"),
                        };

                        let meta = meta.clone();

                        let new_inner = to_clone.clone();

                        drop(guard);

                        self.cached = Some((value, meta));

                        self.inner = new_inner;

                        return Ok(());
                    }

                    // Works if it fails because it replaces Resolving with Resolving
                    if let ResolveState::Unresolved(b, a) =
                        std::mem::replace(&mut *guard, ResolveState::Resolving)
                    {
                        break (b, a);
                    }
                }

                listener.wait();
            }
        };

        match blocking_fn(&self.path, asset_manager) {
            Ok(asset) => {
                let new_self = match asset.downcast() {
                    Ok(ns) => ns,
                    Err(_) => panic!("resolver returned the wrong type"),
                };

                {
                    let mut guard = self.inner.state.lock();

                    *guard = ResolveState::Resolved(new_self.inner.clone());
                }

                self.inner.state_fall.notify(usize::MAX);

                *self = new_self;

                Ok(())
            }
            Err(e) => {
                {
                    let mut guard = self.inner.state.lock();

                    *guard = ResolveState::Unresolved(blocking_fn, async_fn);
                }

                self.inner.state_fall.notify(usize::MAX);

                Err(e)
            }
        }
    }

    /// Fetch the contents of the asset from the [`AssetManager`], asynchronously.
    ///
    /// You must call this on an `Asset` that was created with
    /// [`new_unresolved`](Self::new_unresolved) and hasn't been resolved yet.
    ///
    /// Each clone resolves independently.
    pub async fn resolve_async(
        &mut self,
        asset_manager: &AssetManager,
    ) -> Result<(), GetAssetError> {
        debug_assert!(
            !matches!(*self.inner.state.lock(), ResolveState::AlwaysResolved),
            "double resolve"
        );
        debug_assert!(self.cached.is_none(), "double resolve");

        let (blocking_fn, async_fn) = {
            loop {
                let listener = self.inner.state_fall.listen();

                {
                    let mut guard = self.inner.state.lock();

                    if let ResolveState::Resolved(to_clone) = &*guard {
                        let Some((value, meta)) = to_clone.data.as_ref() else {
                            panic!("asset was resolved but not updated");
                        };

                        let value = match value.clone().downcast() {
                            Ok(value) => value,
                            Err(_) => panic!("resolver returned the wrong type"),
                        };

                        let meta = meta.clone();

                        let new_inner = to_clone.clone();

                        drop(guard);

                        self.cached = Some((value, meta));

                        self.inner = new_inner;

                        return Ok(());
                    }

                    // Works if it fails because it replaces Resolving with Resolving
                    if let ResolveState::Unresolved(b, a) =
                        std::mem::replace(&mut *guard, ResolveState::Resolving)
                    {
                        break (b, a);
                    }
                }

                listener.await;
            }
        };

        match async_fn(&self.path, asset_manager).await {
            Ok(asset) => {
                let new_self = match asset.downcast() {
                    Ok(ns) => ns,
                    Err(_) => panic!("resolver returned the wrong type"),
                };

                {
                    let mut guard = self.inner.state.lock();

                    *guard = ResolveState::Resolved(new_self.inner.clone());
                }

                self.inner.state_fall.notify(usize::MAX);

                *self = new_self;

                Ok(())
            }
            Err(e) => {
                {
                    let mut guard = self.inner.state.lock();

                    *guard = ResolveState::Unresolved(blocking_fn, async_fn);
                }

                self.inner.state_fall.notify(usize::MAX);

                Err(e)
            }
        }
    }
}

impl Asset<dyn IAsset> {
    /// Attempt to downcast this erased asset to a specific asset.
    pub fn downcast<T: IAsset + Sized>(self) -> Result<Asset<T>, Self> {
        if self.inner.type_id == TypeId::of::<T>() {
            Ok(Asset {
                path: self.path,
                inner: self.inner,
                cached: self.cached.map(|c| {
                    (
                        c.0.downcast().ok().expect("type id in inner is misleading"),
                        c.1,
                    )
                }),
                _marker: PhantomData,
            })
        } else {
            Err(self)
        }
    }

    /// Returns whether this asset would downcast to the given type.
    ///
    /// `type_id` is the `T` parameter of this struct.
    pub fn is(&self, type_id: TypeId) -> bool {
        self.inner.type_id == type_id
    }
}

impl<T: IAsset + ?Sized> Clone for Asset<T> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            inner: self.inner.clone(),
            cached: self.cached.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T: IAsset + ?Sized> std::ops::Deref for Asset<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.cached.as_ref().expect("deref of unresolved asset").0
    }
}

impl<T: IAsset + ?Sized> AsRef<T> for Asset<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

#[cfg(feature = "serde")]
mod serde_impl {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::*;
    use crate::core::asset::GlobalInterner;

    impl<T: IAsset> Serialize for Asset<T> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_str(self.path())
        }
    }

    impl<'de, T: IAsset + Sized> Deserialize<'de> for Asset<T> {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let path = <&str>::deserialize(deserializer)?;
            let path = GlobalInterner::intern(path);
            Ok(Asset::new_unresolved(path))
        }
    }
}

#[cfg(feature = "wincode")]
mod wincode_impl {
    use wincode::{SchemaRead, SchemaWrite, config::Config};

    use crate::core::asset::GlobalInterner;

    use super::*;

    unsafe impl<T: IAsset, C: Config> SchemaWrite<C> for Asset<T> {
        type Src = Asset<T>;

        fn size_of(src: &Self::Src) -> wincode::WriteResult<usize> {
            <str as SchemaWrite<C>>::size_of(src.path())
        }

        fn write(writer: impl wincode::io::Writer, src: &Self::Src) -> wincode::WriteResult<()> {
            <str as SchemaWrite<C>>::write(writer, src.path())
        }
    }

    unsafe impl<'de, T: IAsset + Sized, C: Config> SchemaRead<'de, C> for Asset<T> {
        type Dst = Asset<T>;

        fn read(
            reader: impl wincode::io::Reader<'de>,
            dst: &mut std::mem::MaybeUninit<Self::Dst>,
        ) -> wincode::ReadResult<()> {
            let path = <&'de str as SchemaRead<'de, C>>::get(reader)?;
            let path = GlobalInterner::intern(path);
            dst.write(Asset::new_unresolved(path));
            Ok(())
        }

        fn get(reader: impl wincode::io::Reader<'de>) -> wincode::ReadResult<Self::Dst> {
            let path = <&'de str as SchemaRead<'de, C>>::get(reader)?;
            let path = GlobalInterner::intern(path);
            Ok(Asset::new_unresolved(path))
        }
    }
}

/// Owned asset, including its path.
///
/// Unlike [`Asset`], `OwnedAsset` owns its own asset value and path, allowing
/// you to change either at will.
pub struct OwnedAsset<T: IAsset> {
    path: String,
    data: T,
}

impl<T: IAsset> OwnedAsset<T> {
    /// Create an entirely new `OwnedAsset`.
    pub fn new(path: impl Into<String>, data: T) -> Self {
        Self {
            path: path.into(),
            data,
        }
    }

    /// Returns a reference to this asset's path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns a mutable reference to this asset's path.
    pub fn path_mut(&mut self) -> &mut String {
        &mut self.path
    }

    /// Transform this owned asset into another owned asset.
    pub fn into_other<U>(self) -> OwnedAsset<U>
    where
        T: Into<U>,
        U: IAsset,
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

impl<T: IAsset> std::ops::Deref for OwnedAsset<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T: IAsset> std::ops::DerefMut for OwnedAsset<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl<T: IAsset> AsRef<T> for OwnedAsset<T> {
    fn as_ref(&self) -> &T {
        &self.data
    }
}

impl<T: IAsset> AsMut<T> for OwnedAsset<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

impl<T: IAsset + Clone> Clone for OwnedAsset<T> {
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
    fn path_mut(&mut self) -> &mut String;
}

impl<T: IAsset> OwnedAssetTr for OwnedAsset<T> {
    fn expected_type(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn path_mut(&mut self) -> &mut String {
        &mut self.path
    }
}

/// Type-erased owned asset
pub struct DynOwnedAsset {
    asset: Box<dyn OwnedAssetTr>,
}

impl DynOwnedAsset {
    /// Convert an owned asset
    pub fn from_owned<T: IAsset>(asset: OwnedAsset<T>) -> Self {
        Self {
            asset: Box::new(asset),
        }
    }

    /// Try to get a specific owned asset.
    pub fn downcast<T: IAsset + Sized>(self) -> Result<OwnedAsset<T>, Self> {
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

    /// Returns a mutable reference to the path this asset owns
    pub fn path_mut(&mut self) -> &mut String {
        self.asset.path_mut()
    }
}
