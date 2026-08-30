//! Compatibility for assets with the [`ron`] crate.

use std::{
    any::{Any, TypeId},
    marker::PhantomData,
};

use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::core::asset::{IAssetLoader, IAssetSaver, LoadAssetError, SaveAssetError};

/// Saver and loader using [`ron`].
pub struct RonSaveLoad<T> {
    _marker: PhantomData<T>,
}

impl<T> Default for RonSaveLoad<T> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<T> Clone for RonSaveLoad<T> {
    fn clone(&self) -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<T: Serialize + Any + Send + Sync> IAssetSaver for RonSaveLoad<T> {
    fn save_as_bytes(
        &self,
        _asset_path: &str,
        value: &dyn Any,
    ) -> Result<Box<[u8]>, SaveAssetError> {
        let value: &T = value.downcast_ref().ok_or(SaveAssetError::IncorrectType)?;
        match ron::ser::to_string(value) {
            Ok(string) => Ok(string.into_bytes().into_boxed_slice()),
            Err(e) => Err(SaveAssetError::other(e)),
        }
    }

    fn saves(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<T>()
    }
}

impl<T: for<'de> Deserialize<'de> + Any + Send + Sync> IAssetLoader for RonSaveLoad<T> {
    fn load_from_bytes(
        &self,
        _asset_path: &str,
        bytes: &[u8],
    ) -> Result<Box<dyn Any>, LoadAssetError> {
        let s = str::from_utf8(bytes)?;
        match ron::de::from_str::<T>(s) {
            Ok(value) => Ok(Box::new(value)),
            Err(e) => Err(LoadAssetError::other(e)),
        }
    }

    fn loads(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<T>()
    }
}

/// Pretty saver using [`ron`].
pub struct RonSavePretty<T> {
    config: PrettyConfig,
    _marker: PhantomData<T>,
}

impl<T> RonSavePretty<T> {
    /// Construct a new saver with the given config.
    pub fn new(config: PrettyConfig) -> Self {
        Self {
            config,
            _marker: PhantomData,
        }
    }
}

impl<T> Default for RonSavePretty<T> {
    fn default() -> Self {
        Self {
            config: Default::default(),
            _marker: PhantomData,
        }
    }
}

impl<T> Clone for RonSavePretty<T> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T: Serialize + Any + Send + Sync> IAssetSaver for RonSavePretty<T> {
    fn save_as_bytes(
        &self,
        _asset_path: &str,
        value: &dyn Any,
    ) -> Result<Box<[u8]>, SaveAssetError> {
        let value: &T = value.downcast_ref().ok_or(SaveAssetError::IncorrectType)?;
        match ron::ser::to_string_pretty(value, self.config.clone()) {
            Ok(string) => Ok(string.into_bytes().into_boxed_slice()),
            Err(e) => Err(SaveAssetError::other(e)),
        }
    }

    fn saves(&self, type_id: TypeId) -> bool {
        type_id == TypeId::of::<T>()
    }
}
