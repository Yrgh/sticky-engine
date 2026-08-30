//! Compatibility for assets with the [`wincode`] crate.

use std::{
    any::{Any, TypeId},
    marker::PhantomData,
};

use wincode::{SchemaRead, SchemaWrite, config::Config, io::Writer};

use crate::core::asset::{IAssetLoader, IAssetSaver, LoadAssetError, SaveAssetError};

/// Saver and loader for a given [`wincode`] schema and config.
pub struct WincodeSaveLoad<S, C: Config> {
    _marker: PhantomData<(S, C)>,
}

impl<T, C: Config> Default for WincodeSaveLoad<T, C> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<T, C: Config> Clone for WincodeSaveLoad<T, C> {
    fn clone(&self) -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<S, C> IAssetSaver for WincodeSaveLoad<S, C>
where
    S: SchemaWrite<C> + Any + Sync,
    S::Src: Sized,
    C: Config + Sync + Clone,
{
    fn save_as_bytes(&self, value: &dyn Any) -> Result<Box<[u8]>, SaveAssetError> {
        let src: &S::Src = value.downcast_ref().ok_or(SaveAssetError::IncorrectType)?;

        let size = S::size_of(src).map_err(SaveAssetError::other)?;
        let mut buf = Vec::with_capacity(size);

        S::write(buf.by_ref(), src).map_err(SaveAssetError::other)?;

        Ok(buf.into())
    }

    fn saves(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<S::Src>()
    }
}

impl<S, C> IAssetLoader for WincodeSaveLoad<S, C>
where
    S: for<'de> SchemaRead<'de, C> + Any + Sync,
    for<'de> <S as SchemaRead<'de, C>>::Dst: Sized + Any,
    C: Config + Sync + Clone,
{
    fn load_from_bytes(
        &self,
        _asset_path: &str,
        bytes: &[u8],
    ) -> Result<Box<dyn Any>, LoadAssetError> {
        match S::get(bytes) {
            Ok(value) => Ok(Box::new(value)),
            Err(e) => Err(LoadAssetError::other(e)),
        }
    }

    fn loads(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<S::Dst>()
    }
}
