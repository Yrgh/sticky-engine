//! Bidirectional type-trait reflection
//! 
//! [`Relations`] describes two types of reflection: Slot trait object to
//! implementing Components and Component to [`Convert`]s for a Slot. It is
//! lazily loaded and accessible through [`RELATIONS`].
//! 
//! # Capabilities
//! 
//! A `Convert` describes ways to convert a Component to a trait object.
//! [`Relations::get_convert`] allows you to speculatively cast to a Slot
//! without knowing the static type of the value in question.
//! 
//! `Relations` can also tell you which Component types implement a Slot through
//! [`Relations::iter_slot_types`].
//! 
//! If you wish to see whether an unknown type implements a Slot without needing
//! the `Convert`, you can use [`Relations::implements`]. For the purposes of
//! downcasting, the method treats a type as implementing itself, even if it is
//! not a registered trait object.
//! 
//! # Limitations
//! 
//! [`Relations`] uses [`linkme`]. Implementations must be manually registered
//! and non-generic, or else they won't track. The
//! [`slot_impl`](crate::slot_impl) macro will handle this for you.
//! 
//! Because it uses linker magic, some targets won't be supported. In
//! particular, `miri` and `wasm` targets will lose all impl reflection. In the
//! case of [`Relations::implements`], `Self` "implementing" `Self` will still
//! work, but all other functionality will not. On other non-supported targets,
//! you may get a runtime panic or some other behavior.

use std::{
    any::{Any, TypeId},
    cell::{Ref, RefMut},
    collections::HashMap,
    sync::LazyLock,
};

pub use linkme::distributed_slice;

use crate::core::component::{IComponent, ISlotId, ISlotTr};

#[derive(Default)]
pub(crate) struct TypeIdHasher(u64);

impl std::hash::Hasher for TypeIdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        if let Ok(arr) = bytes.try_into() {
            self.0 = u64::from_ne_bytes(arr);
        } else {
            panic!("this hasher is only designed for TypeIds")
        }
    }

    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }
}

#[derive(Default)]
pub(crate) struct TypeId2Hasher(u128);

impl std::hash::Hasher for TypeId2Hasher {
    fn finish(&self) -> u64 {
        #[allow(clippy::cast_possible_truncation)]
        let hi: u64 = self.0 as u64;
        #[allow(clippy::cast_possible_truncation)]
        let lo: u64 = (self.0 >> u64::BITS) as u64;
        hi ^ lo
    }

    fn write(&mut self, bytes: &[u8]) {
        if let Ok(arr) = bytes.try_into() {
            self.0 = self.0.rotate_left(u64::BITS) ^ u64::from_ne_bytes(arr) as u128;
        } else {
            panic!("this hasher is only designed for TypeIds")
        }
    }

    fn write_u64(&mut self, i: u64) {
        self.0 = self.0.rotate_left(u64::BITS) ^ i as u128;
    }
}

pub(crate) type BuildTypeIdHasher = std::hash::BuildHasherDefault<TypeIdHasher>;
pub(crate) type BuildTypeId2Hasher = std::hash::BuildHasherDefault<TypeId2Hasher>;

/// Entry for converting involvind a type object `T`
pub struct Convert<T: ?Sized + ISlotTr> {
    /// Converts a [`std::cell::Ref<dyn IComponent>`] to a [`std::cell::Ref<T>`].
    ///
    /// See [`IComponent`].
    pub comp_to_t_ref: for<'a> fn(Ref<'a, dyn IComponent>) -> Ref<'a, T>,
    /// Converts a [`std::cell::RefMut<dyn IComponent>`] to a [`std::cell::RefMut<T>`].
    ///
    /// See [`IComponent`].
    pub comp_to_t_mut: for<'a> fn(RefMut<'a, dyn IComponent>) -> RefMut<'a, T>,
}

/// A list of types, slot trait objects, and a boxed [`Convert`].
#[distributed_slice]
pub static SLOT_IMPLS: [fn() -> (TypeId, TypeId, Box<dyn Any + Send + Sync>)];

/// Reflection between types and traits.
///
/// There is no way to instantiate this object outside of the global [`RELATIONS`].
pub struct Relations {
    slot_to_ty: HashMap<TypeId, Vec<TypeId>, BuildTypeIdHasher>,
    ty_converts: HashMap<(TypeId, TypeId), Box<dyn Any + Send + Sync>, BuildTypeId2Hasher>,
}

impl Relations {
    fn new() -> Self {
        let mut slot_to_ty: HashMap<TypeId, Vec<TypeId>, BuildTypeIdHasher> = HashMap::default();
        let mut ty_converts = HashMap::default();

        // MIRI will panic on SLOT_IMPLS.into_iter(), so just say nothing for now.
        #[cfg(any(miri, target_arch="wasm32"))]
        return Relations {
            slot_to_ty,
            ty_converts,
        };

        for getter in SLOT_IMPLS {
            let (tyid, trid, conv) = getter();

            slot_to_ty.entry(trid).or_default().push(tyid);

            ty_converts.insert((tyid, trid), conv);
        }

        Relations {
            slot_to_ty,
            ty_converts,
        }
    }

    /// Returns the [`Convert`] for the given `tyid`, if one is registered.
    ///
    /// Note: `D` is an [`ISlotId`], not the trait.
    pub fn get_convert<D: ISlotId>(
        &'static self,
        tyid: TypeId,
    ) -> Option<&'static Convert<D::TraitObject>> {
        #[cfg(any(miri, target_arch="wasm32"))]
        tracing::error!("miri has disable features of RELATIONS, including get_convert");
        
        let boxy = self
            .ty_converts
            .get(&(tyid, TypeId::of::<D::TraitObject>()))?;
        Some(boxy.downcast_ref().expect("bad slot id"))
    }

    /// Returns an iterator over all types registered as implementing a trait.
    ///
    /// Note: `D` is an [`ISlotId`], not the trait.
    pub fn iter_slot_types<D: ISlotId>(&self) -> impl Iterator<Item = TypeId> {
        #[cfg(any(miri, target_arch="wasm32"))]
        tracing::error!("miri has disable features of RELATIONS, including iter_slot_types");
        
        self.slot_to_ty
            .get(&TypeId::of::<D::TraitObject>())
            .into_iter()
            .flatten()
            .copied()
    }

    /// Returns `true` if a type is registered as implementing a trait.
    ///
    /// `tyid` is the [`TypeId`] of the *self type*. `trid` is the `TypeId` of
    /// the *trait object* for the requested trait.
    ///
    /// There is one important, kind of hacky facet: **if `tyid == trid` this
    /// also returns `true`**. This is because a
    /// [`ComponentId<C>`](crate::core::component::ComponentId) calls `C` its
    /// "trait object", despite `C` being concrete.
    pub fn implements(&self, tyid: TypeId, trid: TypeId) -> bool {
        tyid == trid || self.ty_converts.contains_key(&(tyid, trid))
    }
}

/// The global [`Relations`] instance.
pub static RELATIONS: LazyLock<Relations> = LazyLock::new(Relations::new);
