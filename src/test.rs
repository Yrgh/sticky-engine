//! Integration-style tests split into inline modules, one per domain area.
//! Each module owns its helper types and depends on [`support`] for the
//! common world/asset-manager scaffolding.

/// `support`: shared helpers + the minimal asset and component test types.
pub mod support {
    use std::{any::Any, sync::Arc};

    use tracing_subscriber::{fmt::format::FmtSpan, util::SubscriberInitExt};

    use crate::{
        comp_def, slot_def, slot_impl,
        builtin::assets::simple_impl::{FalseAsyncFs, FsAccessor, NaiveCacher},
        core::{
            asset::{
                traits::{IAssetLoader, IAssetSaver, LoadAssetError, SaveAssetError, SaverLoader},
                AssetManager, AutoAsset, Interner,
            },
            component::{ComponentId, ComponentParent, DynComponentId, IComponent, ISlotId},
            main_loop::{ManualDriver, ManualDriverNewError},
            world::World,
        },
    };

    /// Create an empty [`AssetManager`] backed by a [`FsAccessor`] using
    /// [`FalseAsyncFs`] (so no async executor is required).
    ///
    /// No loaders, savers, cachers, or default cacher are registered.
    pub fn new_asset_manager() -> AssetManager {
        let mut builder = AssetManager::builder();
        builder
            // FalseAsyncFs is only to remove the need for a runtime here.
            .with_accessor(FsAccessor::<FalseAsyncFs>::new("./"));
        builder.build()
    }

    /// A basic headless "world" built on top of `new_asset_manager()`.
    ///
    /// Returns the [`ManualDriver`] so tests can tick it manually. The driver
    /// owns a [`World`] with a headless main `Level`.
    pub fn new_world() -> Result<ManualDriver, ManualDriverNewError> {
        let mut builder = World::builder();
        builder.with_asset_manager(new_asset_manager()).headless();

        ManualDriver::new(builder, |_| {})
    }

    /// A wrapper around `new_world()` that also installs a global tracing
    /// subscriber (required for the engine, as it assumes one is set).
    pub fn new_world_traced() -> Result<ManualDriver, ManualDriverNewError> {
        let _ = tracing_subscriber::fmt()
            .with_span_events(FmtSpan::NONE)
            .with_target(true)
            .finish()
            .try_init();

        new_world()
    }

    /// Returns a fresh temporary directory path (a unique one per call).
    pub fn temp_dir_path() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sticky_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    // ---- Minimal test asset + matching loader/saver ----

    /// A simple asset that serializes/deserializes its inner string (one byte
    /// counts as the length, followed by the bytes).
    #[derive(Debug, Clone, PartialEq)]
    pub struct TestAsset(pub String);

    impl AutoAsset for TestAsset {}

    /// Loader that parses a `TestAsset` from its byte representation.
    #[derive(Default)]
    pub struct TestAssetLoader;

    impl IAssetLoader for TestAssetLoader {
        fn load_from_bytes(
            &self,
            _path: &Arc<str>,
            bytes: &[u8],
        ) -> Result<Box<dyn Any>, LoadAssetError> {
            let len = *bytes
                .first()
                .ok_or(LoadAssetError::BadPath("empty".into()))?
                as usize;
            let s = std::str::from_utf8(&bytes[1..1 + len]).map_err(LoadAssetError::Utf8)?;
            Ok(Box::new(TestAsset(s.to_string())))
        }

        fn loads(&self, type_id: std::any::TypeId) -> bool {
            type_id == std::any::TypeId::of::<TestAsset>()
        }
    }

    /// Saver that serializes a `TestAsset` into its byte representation.
    #[derive(Default)]
    pub struct TestAssetSaver;

    impl IAssetSaver for TestAssetSaver {
        fn save_as_bytes(
            &self,
            _path: &Arc<str>,
            value: &dyn Any,
        ) -> Result<Box<[u8]>, SaveAssetError> {
            let value = value
                .downcast_ref::<TestAsset>()
                .ok_or(SaveAssetError::IncorrectType)?;
            let len = u8::try_from(value.0.len())
                .map_err(|_| SaveAssetError::BadPath("TestAsset string is too long".into()))?;
            let mut out = Vec::with_capacity(1 + value.0.len());
            out.push(len);
            out.extend_from_slice(value.0.as_bytes());
            Ok(out.into_boxed_slice())
        }

        fn saves(&self, type_id: std::any::TypeId) -> bool {
            type_id == std::any::TypeId::of::<TestAsset>()
        }
    }

    /// A `SaverLoader` that is both the loader and saver above.
    #[derive(Default)]
    pub struct TestAssetSaverLoader;

    impl SaverLoader for TestAssetSaverLoader {
        fn split(self) -> (Self, Self) {
            (Self, Self)
        }
    }

    impl IAssetSaver for TestAssetSaverLoader {
        fn save_as_bytes(
            &self,
            path: &Arc<str>,
            value: &dyn Any,
        ) -> Result<Box<[u8]>, SaveAssetError> {
            TestAssetSaver.save_as_bytes(path, value)
        }

        fn saves(&self, type_id: std::any::TypeId) -> bool {
            type_id == std::any::TypeId::of::<TestAsset>()
        }
    }

    impl IAssetLoader for TestAssetSaverLoader {
        fn load_from_bytes(
            &self,
            path: &Arc<str>,
            bytes: &[u8],
        ) -> Result<Box<dyn Any>, LoadAssetError> {
            TestAssetLoader.load_from_bytes(path, bytes)
        }

        fn loads(&self, type_id: std::any::TypeId) -> bool {
            type_id == std::any::TypeId::of::<TestAsset>()
        }
    }

    /// Builds an [`AssetManager`] (with a temp-dir `FsAccessor`) fully wired up
    /// for `TestAsset`: loader, saver, and cacher all registered.
    pub fn new_asset_manager_with_test_asset() -> (AssetManager, std::path::PathBuf) {
        let root = temp_dir_path();
        let mut builder = AssetManager::builder();
        let interner = builder.interner();
        builder
            .with_accessor(FsAccessor::<FalseAsyncFs>::new(&root))
            .register_all::<TestAsset>(
                TestAssetSaverLoader,
                NaiveCacher::<TestAsset>::new(interner),
                TestAssetSaverLoader,
            );
        (builder.build(), root)
    }

    /// Returns a standalone [`Interner`] from a fresh `AssetManagerBuilder`.
    pub fn asset_builder_interner() -> Arc<Interner> {
        let builder = AssetManager::builder();
        builder.interner()
    }

    // ---- Minimal Component and Slot types for tree-level tests ----

    #[slot_def]
    pub trait SValue {
        fn value(&self) -> i32;
    }

    comp_def! {
        pub struct CValue {
            components { }
            variables { pub v: i32 }
            behaviors {
                #[init]
                fn init(
                    _world: &World,
                    _parent: ComponentParent,
                    _self_id: ComponentId<Self>,
                    v: i32
                ) -> CValueInit {
                    CValueInit { v }
                }
            }
        }
    }

    #[slot_impl]
    impl SValue for CValue {
        fn value(&self) -> i32 {
            self.v
        }
    }

    comp_def! {
        pub struct CParent {
            components { static child: CValue }
            variables { }
            behaviors {
                #[init]
                fn init(
                    world: &World,
                    _parent: ComponentParent,
                    self_id: ComponentId<Self>,
                    _: ()
                ) -> CParentInit {
                    let child = CValue::spawn(world, self_id.clone().into(), 7);
                    CParentInit { child }
                }
            }
        }
    }
}

/// `assets`: the [`AssetManager`] builder, registration, get/set paths, and
/// the [`Asset`]/[`OwnedAsset`]/[`Interner`] containers.
mod assets {
    use std::{any::TypeId, sync::Arc};

    use crate::{
        builtin::assets::simple_impl::{FalseAsyncFs, FsAccessor, NaiveCacher},
        core::asset::{
            manager::{GetAssetError, SetAssetError},
            Asset, AssetManager, OwnedAsset,
        },
    };

    use super::support::*;

    // ---- AssetManager builder / registration ----

    #[test]
    #[should_panic]
    fn build_panics_without_accessor() {
        let _ = AssetManager::builder().build();
    }

    #[test]
    fn build_errors_require_balanced_parts() {
        // A loader with no cacher and no default cacher must panic.
        let mut builder = AssetManager::builder();
        builder
            .with_accessor(FsAccessor::<FalseAsyncFs>::new("./"))
            .register_loader::<TestAsset>(TestAssetLoader);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| builder.build()));
        assert!(res.is_err(), "loader without cacher should panic");

        // A cacher with no loader must panic.
        let mut b2 = AssetManager::builder();
        b2.with_accessor(FsAccessor::<FalseAsyncFs>::new("./"));
        let interner = b2.interner();
        b2.register_cacher::<TestAsset>(NaiveCacher::<TestAsset>::new(interner));
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| b2.build()));
        assert!(res.is_err(), "cacher without loader should panic");
    }

    #[test]
    #[should_panic]
    fn register_loader_twice_panics() {
        let mut builder = AssetManager::builder();
        builder
            .with_accessor(FsAccessor::<FalseAsyncFs>::new("./"))
            .register_loader::<TestAsset>(TestAssetLoader)
            .register_loader::<TestAsset>(TestAssetLoader);
    }

    #[test]
    #[should_panic]
    fn register_loader_rejects_wrong_type() {
        // `TestAssetLoader` doesn't `loads` `u32`.
        let mut builder = AssetManager::builder();
        builder
            .with_accessor(FsAccessor::<FalseAsyncFs>::new("./"))
            .register_loader::<u32>(TestAssetLoader);
    }

    // ---- Asset get/set paths ----

    #[test]
    fn get_returns_no_loader_with_empty_manager() {
        let am = new_asset_manager();
        // `Asset` doesn't impl `Debug`, so pattern-match instead of `unwrap_err`.
        match am.get_asset_blocking::<TestAsset>("hi.txt") {
            Err(GetAssetError::NoLoader) => {}
            _ => panic!("expected NoLoader"),
        }
    }

    #[test]
    fn set_returns_no_saver_with_empty_manager() {
        let am = new_asset_manager();
        match am.set_asset_blocking(OwnedAsset::new("hi.txt", TestAsset("hi".into()))) {
            Err(SetAssetError::NoSaver) => {}
            _ => panic!("expected NoSaver"),
        }
    }

    #[test]
    fn set_get_round_trip() {
        let (am, _root) = new_asset_manager_with_test_asset();

        let saved = am
            .set_asset_blocking(OwnedAsset::new("config", TestAsset("hello".into())))
            .expect("should save");
        assert_eq!(saved.as_ref(), &TestAsset("hello".into()));
        assert_eq!(saved.path(), "config");

        let loaded = am
            .get_asset_blocking::<TestAsset>("config")
            .expect("should load from disk");
        assert_eq!(loaded.as_ref(), &TestAsset("hello".into()));
        assert_eq!(loaded.path(), "config");
    }

    #[test]
    fn get_missing_file_on_disk() {
        let (am, _root) = new_asset_manager_with_test_asset();
        match am.get_asset_blocking::<TestAsset>("does_not_exist") {
            Err(GetAssetError::BytesError(_)) => {}
            _ => panic!("expected BytesError"),
        }
    }

    // ---- Interner ----

    #[test]
    fn interner_deduplicates() {
        let interner = asset_builder_interner();
        let path = "some/asset/path.blob";
        let a = interner.intern(path);
        let b = interner.intern(path);
        let c = interner.intern("some/other/path");

        assert!(Arc::ptr_eq(&a, &b), "same string should intern to same Arc");
        assert!(!Arc::ptr_eq(&a, &c), "different strings should differ");
        assert_eq!(&*a, path);
    }

    // ---- Asset / OwnedAsset containers ----

    #[test]
    fn asset_new_resolved_path_and_deref() {
        let asset = Asset::new_resolved(
            Arc::from("tex.png"),
            Arc::new(TestAsset("data".into())),
            None,
        );
        assert_eq!(asset.path(), "tex.png");
        assert_eq!(asset.as_ref(), &TestAsset("data".into()));
        // Deref gives access to the underlying TestAsset.
        assert_eq!(asset.0.as_str(), "data");
    }

    #[test]
    fn asset_into_dyn_downcast_is() {
        let asset = Asset::new_resolved(
            Arc::from("tex.png"),
            Arc::new(TestAsset("data".into())),
            None,
        );
        let dyn_asset = asset.clone().into_dyn();
        assert!(dyn_asset.is(TypeId::of::<TestAsset>()));
        assert!(!dyn_asset.is(TypeId::of::<u32>()));

        let downcast = match dyn_asset.clone().downcast::<TestAsset>() {
            Ok(d) => d,
            Err(_) => panic!("should downcast to TestAsset"),
        };
        assert_eq!(downcast.as_ref(), &TestAsset("data".into()));
    }

    #[test]
    fn asset_partial_eq_clones_equal() {
        let asset = Asset::new_resolved(
            Arc::from("tex.png"),
            Arc::new(TestAsset("data".into())),
            None,
        );
        let clone = asset.clone();
        assert!(asset == clone);

        let other = Asset::new_resolved(
            Arc::from("tex.png"),
            Arc::new(TestAsset("data".into())),
            None,
        );
        assert!(asset != other, "different inners are not equal");
    }

    #[test]
    fn owned_asset_basics() {
        let mut owned = OwnedAsset::new("a.asset", TestAsset("x".into()));
        assert_eq!(owned.path(), "a.asset");

        *owned.path_mut() = "b.asset".into();
        assert_eq!(owned.path(), "b.asset");

        assert_eq!(owned.as_ref(), &TestAsset("x".into()));

        let raw: TestAsset = owned.clone().into_inner();
        assert_eq!(raw, TestAsset("x".into()));

        let other: OwnedAsset<TestAsset> = owned.into_other();
        assert_eq!(other.path(), "b.asset");
    }
}

/// `world`: [`ManualDriver`] construction, `EngineSync` settings, and the
/// headless [`World`] / main [`Level`] lifecycle.
mod world {
    use std::time::Duration;

    use crate::{
        core::{main_loop::{ManualDriver, ManualDriverNewError}, world::World},
    };

    use super::support::*;

    #[test]
    fn manual_driver_nothing_ops() {
        let mut driver = new_world_traced().unwrap();

        assert!(!driver.tick_physics());
        assert!(!driver.tick_idle(0.0));
        assert!(!driver.tick_idle(0.05));
        assert!(!driver.tick_idle(1.0));
    }

    #[test]
    fn errors_without_asset_manager() {
        let mut builder = World::builder();
        builder.headless();
        let res = ManualDriver::new(builder, |_| {});
        assert!(matches!(res, Err(ManualDriverNewError::NoAssetManager)));
    }

    #[test]
    fn engine_tick_rates_round_trip() {
        let driver = new_world_traced().unwrap();
        let engine = driver.engine();

        assert_eq!(engine.get_stable_tick_rate(), Duration::from_millis(15));
        assert_eq!(engine.get_idle_min_delay(), Duration::from_millis(15));

        engine.set_stable_tick_rate(Duration::from_millis(33));
        engine.set_idle_min_delay(Duration::from_millis(22));
        assert_eq!(engine.get_stable_tick_rate(), Duration::from_millis(33));
        assert_eq!(engine.get_idle_min_delay(), Duration::from_millis(22));
    }

    #[test]
    fn headless_creates_inactive_main_level() {
        let driver = new_world_traced().unwrap();
        let world = driver.world();

        let level = world.main_level().expect("headless creates a main level");
        assert!(!level.is_active(), "a headless level starts inactive");
        assert!(level.set_active(true));
        assert!(level.is_active());
    }

    #[test]
    fn create_get_iterate_destroy_level() {
        let mut driver = new_world_traced().unwrap();
        let world = driver.world();

        let owned = world.create_level();
        let handle = owned.handle();
        assert!(world.get_level(handle).is_some());

        // iter_levels includes the headless main level plus our new one.
        let count = world.iter_levels().count();
        assert!(count >= 2);

        // Destroy and flush the action queue to free it.
        world.destroy_level(owned);
        driver.flush();
        assert!(driver.world().get_level(handle).is_none());
    }
}

/// `level`: component tree, `Level` scripting, and component IDs/reflection.
mod level {
    use std::any::TypeId;

    use crate::{
        core::{
            component::{ComponentId, ComponentParent, DynComponentId, IComponent, ISlotId},
            input::InputEvent,
            level::{script::IScript, AddScriptError, Level},
            relations::RELATIONS,
            trans::STrans3Id,
            world::World,
        },
    };

    use super::support::*;

    // ---- Component tree ----

    #[test]
    fn spawn_and_iterate_components() {
        let driver = new_world_traced().unwrap();
        let world = driver.world();
        let level = world.main_level().expect("main level exists");

        let id = level.spawn_top_level::<CValue>(world, 42);
        assert_eq!(id.get(world).unwrap().value(), 42);

        let mut iter = level.iter_top_level();
        let got: DynComponentId = iter.next().unwrap();
        let expected: DynComponentId = id.clone().into();
        assert_eq!(got, expected);
        assert!(iter.next().is_none());

        // Iterate by type.
        let typed: Vec<i32> = level
            .iter_type::<CValue>()
            .map(|c| c.value())
            .collect();
        assert_eq!(typed, vec![42]);
    }

    #[test]
    fn remove_top_level() {
        let driver = new_world_traced().unwrap();
        let world = driver.world();
        let level = world.main_level().expect("main level exists");

        let id = level.spawn_top_level::<CValue>(world, 1);
        let dyn_id: DynComponentId = id.into();

        assert!(level.remove_top_level(world, &dyn_id));
        assert!(level.iter_top_level().next().is_none());
    }

    #[test]
    fn parent_and_children() {
        let driver = new_world_traced().unwrap();
        let world = driver.world();
        let level = world.main_level().expect("main level exists");

        let parent_id = level.spawn_top_level::<CParent>(world, ());
        let child_id: ComponentId<CValue> = parent_id.get(world).unwrap().get_child_id();

        // Child's parent must be the parent component (same level id).
        let child_parent = child_id.get(world).unwrap().parent();
        assert_eq!(child_parent.level_id(), parent_id.level_id());
        assert!(matches!(child_parent, ComponentParent::Component(_)));

        let children: Vec<DynComponentId> = parent_id
            .get(world)
            .unwrap()
            .children()
            .collect();
        let expected_child: DynComponentId = child_id.clone().into();
        assert_eq!(children, vec![expected_child]);
    }

    // ---- Component IDs & reflection ----

    #[test]
    fn component_id_cast_and_into() {
        let driver = new_world_traced().unwrap();
        let world = driver.world();
        let level = world.main_level().expect("main level exists");

        let id = level.spawn_top_level::<CValue>(world, 3);

        // Converting to a DynComponentId works via `Into`.
        let dyn_id: DynComponentId = id.clone().into();
        assert_eq!(dyn_id.level_id(), id.level_id());

        // Casting to a *concrete* slot ID succeeds only when the slot is
        // registered for the Component (SValue is, via slot_impl).
        assert!(id.clone().cast::<SValueId>().is_ok());
        // Casting to an unregistered slot should fail rather than silently succeed.
        assert!(id.clone().cast::<STrans3Id>().is_err());
    }

    #[test]
    fn relations_implements() {
        assert!(RELATIONS.implements(TypeId::of::<CValue>(), TypeId::of::<dyn SValue>()));
        assert!(!RELATIONS.implements(TypeId::of::<CValue>(), TypeId::of::<u32>()));
    }

    // ---- Scripts ----

    /// A script that records whether its hooks were called.
    #[derive(Default)]
    struct CallScript {
        post_init: bool,
        idle_calls: u32,
    }

    impl IScript for CallScript {
        fn post_init(&mut self, _world: &World, _level: &Level) {
            self.post_init = true;
        }
        fn destroy(&mut self, _world: &World, _level: &Level) {}
        fn idle(&mut self, _world: &World, _level: &Level, _delta: f32) {
            self.idle_calls += 1;
        }
        fn pre_phys(&mut self, _world: &World, _level: &Level, _delta: f32) {}
        fn post_phys(&mut self, _world: &World, _level: &Level, _delta: f32) {}
        fn raw_input(&mut self, _world: &World, _level: &Level, _event: &InputEvent) {}
    }

    #[test]
    fn scripts_add_get_set_remove() {
        let mut driver = new_world_traced().unwrap();

        {
            let world = driver.world();
            let level = world.main_level().expect("main level exists");

            assert!(level.add_script(world, CallScript::default()).is_ok());
            assert!(level.get_script::<CallScript>().is_ok());

            // Adding a second script of the same type fails with AlreadyExists.
            let dup = level.add_script(world, CallScript::default());
            assert!(matches!(dup, Err(AddScriptError::AlreadyExists(_))));

            // Hooks are dispatched only for active levels.
            level.set_active(true);
        }

        // Ticking requires a mutable driver, so do it after releasing borrows.
        driver.tick_idle(0.1);
        driver.tick_idle(0.1);

        let world = driver.world();
        let level = world.main_level().expect("main level exists");
        assert_eq!(level.get_script::<CallScript>().unwrap().idle_calls, 2);

        level.remove_script::<CallScript>(world).unwrap();
        assert!(level.get_script::<CallScript>().is_err());
    }

    #[test]
    fn inactive_level_does_not_run_hooks_via_manual_driver() {
        let mut driver = new_world_traced().unwrap();

        {
            let world = driver.world();
            let level = world.main_level().expect("main level exists");
            // Level starts inactive, so its script hooks must NOT fire.
            assert!(level.add_script(world, CallScript::default()).is_ok());
        }

        driver.tick_idle(0.1);
        driver.tick_idle(0.1);

        {
            let world = driver.world();
            let level = world.main_level().expect("main level exists");
            assert_eq!(
                level.get_script::<CallScript>().unwrap().idle_calls,
                0,
                "inactive level's idle hooks should not run"
            );
            // Now activate; hooks fire once active.
            level.set_active(true);
        }

        driver.tick_idle(0.1);

        let world = driver.world();
        let level = world.main_level().expect("main level exists");
        assert_eq!(level.get_script::<CallScript>().unwrap().idle_calls, 1);
    }
}

/// `transform`: the `Trans3` providers and their local/global cache behavior.
mod transform {
    use crate::{
        core::{
            component::ComponentParent,
            trans::{ITrans3Provider, Trans3ProviderRelative, Trans3ProviderTop},
        },
    };

    use super::support::*;

    #[test]
    fn relative_no_parent_local_equals_global() {
        let driver = new_world_traced().unwrap();
        let world = driver.world();
        let level = world.main_level().expect("main level exists");
        level.set_active(true);

        // A provider whose parent is the level itself (no ancestor STrans3).
        let parent = ComponentParent::Level(level.id());
        let mut rel = Trans3ProviderRelative::new(&parent);
        rel.set_local_trans(
            world,
            crate::glamx::Pose3::from_translation(crate::glamx::Vec3::X * 3.0),
        );

        assert_eq!(rel.get_local_trans(world), rel.get_global_trans(world));
    }

    #[test]
    fn top_no_parent_local_equals_global() {
        let driver = new_world_traced().unwrap();
        let world = driver.world();
        let level = world.main_level().expect("main level exists");

        let parent = ComponentParent::Level(level.id());
        let mut top = Trans3ProviderTop::new(&parent);
        top.set_global_trans(
            world,
            crate::glamx::Pose3::from_translation(crate::glamx::Vec3::Y * 5.0),
        );

        assert_eq!(top.get_global_trans(world), top.get_local_trans(world));
    }
}
