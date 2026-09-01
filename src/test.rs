use tracing_subscriber::fmt::format::FmtSpan;

use crate::{builtin::assets::simple_impl::{FalseAsyncFs, FsAccessor}, core::main_loop::ManualDriver, prelude::*};

#[test]
fn manual_driver_nothing_ops() {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_span_events(FmtSpan::NONE)
            .with_target(true)
            .finish(),
    )
    .expect("failed to set global subscriber");

    let mut builder = AssetManager::builder();
    builder
        // FalseAsyncFs is only to remove the need for a runtime here.
        .with_accessor(FsAccessor::<FalseAsyncFs>::new("./"));

    let asset_manager = builder.build();
    
    let mut builder = World::builder();
    builder.with_asset_manager(asset_manager);
    
    let mut driver = ManualDriver::new(
        builder,
        |_| {}
    ).unwrap();

    assert!(!driver.tick_physics());
    assert!(!driver.tick_idle(0.0));
    assert!(!driver.tick_idle(0.05));
    assert!(!driver.tick_idle(1.0));
}