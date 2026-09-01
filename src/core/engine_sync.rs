//! Multithread/async engine access.
//!
//! Unlike [`World`](crate::core::world::World), [`EngineSync`] is `Send + Sync`. This allows some

use std::time::Duration;

use futures::channel::mpsc;

use crate::core::{
    asset::AssetManager,
    main_loop::{MainClosedError, MainJob},
};

pub(crate) struct SyncCellDuration {
    value: parking_lot::Mutex<Duration>,
}

impl SyncCellDuration {
    fn get(&self) -> Duration {
        *self.value.lock()
    }

    fn set(&self, value: Duration) {
        *self.value.lock() = value;
    }
}

impl From<Duration> for SyncCellDuration {
    fn from(value: Duration) -> Self {
        Self {
            value: parking_lot::Mutex::new(value),
        }
    }
}

/// Engine context that can be shared across threads.
pub struct EngineSync {
    pub(crate) stable_rate: SyncCellDuration,
    pub(crate) min_idle_delay: SyncCellDuration,

    pub(crate) main_queue_async: mpsc::Sender<MainJob>,
    pub(crate) main_queue_sync: std::sync::mpsc::Sender<MainJob>,

    pub(crate) asset_manager: AssetManager,
}

impl EngineSync {
    /// Returns the rate at which physics are run.
    pub fn get_stable_tick_rate(&self) -> Duration {
        self.stable_rate.get()
    }

    /// Sets the rate at which physics are run.
    pub fn set_stable_tick_rate(&self, rate: Duration) {
        self.stable_rate.set(rate);
    }

    /// Returns the minimum delay between idle hooks.
    pub fn get_idle_min_delay(&self) -> Duration {
        self.min_idle_delay.get()
    }

    /// Sets the minimum delay between idle hooks.
    pub fn set_idle_min_delay(&self, rate: Duration) {
        self.min_idle_delay.set(rate);
    }
}

impl EngineSync {
    pub(crate) fn queue_job_sync(&self, job: MainJob) -> Result<(), MainClosedError> {
        match self.main_queue_sync.send(job) {
            Ok(()) => Ok(()),
            Err(_) => Err(MainClosedError),
        }
    }

    pub(crate) async fn queue_job_async(&self, job: MainJob) -> Result<(), MainClosedError> {
        use futures::SinkExt;

        match self.main_queue_async.clone().send(job).await {
            Ok(()) => Ok(()),
            Err(_) => Err(MainClosedError),
        }
    }
}

impl EngineSync {
    /// Returns the engine's asset manager
    pub fn asset_manager(&self) -> &AssetManager {
        &self.asset_manager
    }
}
