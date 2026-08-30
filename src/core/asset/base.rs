//! Utilities for implementing pieces of the [`AssetManager`](super::AssetManager)

use std::sync::Arc;

use parking_lot::Mutex;

use event_listener::{Event, Listener};

struct State<T, X: Clone> {
    value: Option<(Arc<T>, X)>,
    is_initializing: bool,
}

/// The content of one entry in an asset cache.
/// 
/// `T` is stored in an [`Arc`]. `X` is stored inline. You can use `X` for
/// metadata.
///
/// While it is up to the user to provide an actual cache, the engine provides
/// this, what actually has to be stored. A simple, naive cache (which is
/// provided in the builtins) could be based on an `elsa::FrozenMap<Arc<str>,
/// AssetCacheContent>`.
pub struct AssetCacheContent<T, X: Clone> {
    mutex: Mutex<State<T, X>>,
    unlock_event: Event,
}

impl<T, X: Clone> Default for AssetCacheContent<T, X> {
    fn default() -> Self {
        Self {
            mutex: Mutex::new(State {
                value: None,
                is_initializing: false,
            }),
            unlock_event: Event::new(),
        }
    }
}

enum GetActivityResult<T, X> {
    ContinueWaiting,
    /// Value is empty, is_initializing was acquired
    EmptyAcquired,
    Success(Arc<T>, X),
}

impl<T, X: Clone> AssetCacheContent<T, X> {
    fn get_activity(&self) -> GetActivityResult<T, X> {
        let mut state = self.mutex.lock();

        if state.is_initializing {
            return GetActivityResult::ContinueWaiting;
        }

        // Not initializing

        if let Some((value, ex)) = state.value.as_ref() {
            return GetActivityResult::Success(value.clone(), ex.clone());
        }

        // Acquire initializing
        state.is_initializing = true;

        GetActivityResult::EmptyAcquired
    }

    // It is expected that after you call *_get_or_lock, you either call unlock
    // or update_and_unlock.

    /// Main logic of [`retrieve_asset_async`](super::IAssetCacher::retrieve_asset_async).
    pub async fn async_get_or_lock(&self) -> Option<(Arc<T>, X)> {
        loop {
            let listener = self.unlock_event.listen();

            match self.get_activity() {
                GetActivityResult::ContinueWaiting => listener.await,
                GetActivityResult::EmptyAcquired => return None,
                GetActivityResult::Success(value, ex) => return Some((value, ex)),
            }
        }
    }

    /// Main logic of [`retrieve_asset_blocking`](super::IAssetCacher::retrieve_asset_blocking).
    pub fn blocking_get_or_lock(&self) -> Option<(Arc<T>, X)> {
        loop {
            let listener = self.unlock_event.listen();

            match self.get_activity() {
                GetActivityResult::ContinueWaiting => listener.wait(),
                GetActivityResult::EmptyAcquired => return None,
                GetActivityResult::Success(value, ex) => return Some((value, ex)),
            }
        }
    }

    /// Main logic of [`update_asset_unlocking`](super::IAssetCacher::update_asset_unlocking).
    pub fn update_and_unlock(&self, value: Option<(Arc<T>, X)>) {
        let mut state = self.mutex.lock();

        state.value = value;

        if state.is_initializing {
            // Don't duplicate below, it causes a deadlock
            state.is_initializing = false;
            drop(state);
            self.unlock_event.notify(usize::MAX);
        }
    }

    /// Main logic of [`release_asset_lock`](super::IAssetCacher::release_asset_lock).
    pub fn unlock(&self) {
        let mut state = self.mutex.lock();

        if state.is_initializing {
            state.is_initializing = false;
            drop(state);
            self.unlock_event.notify(usize::MAX);
        }
    }

    fn update_activity(&self, value: &mut Option<(Arc<T>, X)>) -> bool {
        let mut state = self.mutex.lock();

        if !state.is_initializing {
            state.value = value.take();

            true
        } else {
            false
        }
    }

    /// Main logic of [`update_asset_async`](super::IAssetCacher::update_asset_async).
    pub async fn async_wait_and_update(&self, mut value: Option<(Arc<T>, X)>) {
        loop {
            let listener = self.unlock_event.listen();

            if self.update_activity(&mut value) {
                return;
            }

            listener.await;
        }
    }

    /// Main logic of [`update_asset_blocking`](super::IAssetCacher::update_asset_blocking).
    pub fn blocking_wait_and_update(&self, mut value: Option<(Arc<T>, X)>) {
        loop {
            let listener = self.unlock_event.listen();

            if self.update_activity(&mut value) {
                return;
            }

            listener.wait();
        }
    }
}
