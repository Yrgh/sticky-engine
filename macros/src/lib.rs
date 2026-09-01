extern crate proc_macro;

use proc_macro::TokenStream as TokenStream1;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span as Span2, TokenStream as TokenStream2};
use quote::{ToTokens, quote, quote_spanned};
use syn::{
    FieldModifiers, FnArg, GenericArgument, Ident, ImplItem, ItemImpl, ItemTrait, ItemUse, Path,
    PathArguments, PathSegment, ReturnType, Token, Type, VisRestricted, Visibility, braced,
    parenthesized, parse::Parse, parse_macro_input, parse_quote, punctuated::Punctuated,
    spanned::Spanned, token,
};
use uuid::Uuid;

fn error(tokens: impl quote::ToTokens, message: impl std::fmt::Display) -> TokenStream1 {
    syn::Error::new_spanned(tokens, message)
        .to_compile_error()
        .into()
}

mod slots;
mod components;

#[proc_macro_attribute]
/// Defines a new Slot - a trait for Components.
///
/// It can be applied to any trait definition, so long as the trait has no
/// generic arguments. It inserts two bounds: `IComponent` and a generated trait
/// with a diagnostic message. The macro also creates an ID type, called
/// `SlotNameId` (where SlotName is the name of the trait).
///
/// If you get messages saying the `sticky-engine` crate cannot be found, you
/// can add `(in path)` to the macro arguments, where `path` is the path to the
/// `sticky-engine` crate.
pub fn slot_def(attr: TokenStream1, input: TokenStream1) -> TokenStream1 {
    slots::slot_def_inner(attr, input)
}

#[proc_macro_attribute]
/// Attribute to be added to all `impl SlotName for Component` blocks.
///
/// This adds additional information to the impl, including implementing
/// `AsDyn<dyn SlotName>` on your Component and generating reflection info.
///
/// If you get errors saying the `sticky-engine` crate cannot be found, you
/// can add `(in path)` to the macro arguments, where `path` is the path to the
/// `sticky-engine` crate.
pub fn slot_impl(attr: TokenStream1, input: TokenStream1) -> TokenStream1 {
    slots::slot_impl_inner(attr, input)
}

#[proc_macro]
/// Defines a Component.
///
/// The basic structure is
///
/// ```rust
/// comp_def! {
///     struct CComponent {
///         // Child components, stored as private fields with getters and setters.
///         components {
///             static component1: CExample, // MUST be a concrete type.
///             dyn component2: SExample, // Concrete type or dyn SlotName.
///             dyn? component3: SExample, // Same as component2, but behaves like an Option.
///             dyn* component4: SExample, // Same as component2, but behaves like a Vec.
///         }
///         // Fields added to your structure
///         variables {
///             var1: Type1,
///             pub var2: Type2,
///         }
///         // Special impl block
///         behaviors {
///             // Called when CComponent is spawned. CComponentInit holds initial values for all
///             // variables and ALL child Components. Spawn static children inside init via
///             // `CExample::spawn(self_id.into(), info)`.
///             //
///             // Note: self_id can be passed around but will return None on all accesses until after
///             // init completes
///             #[init]
///             fn init(
///                 world: &World,
///                 parent: ComponentParent,
///                 self_id: ComponentId<Self>,
///                 info: ()
///             ) -> CComponentInit {
///                 ...
///             }
///         }
///     }
/// }
/// ```
///
/// If you get messages saying the `sticky-engine` crate cannot be found, you
/// can add `(in path)` to the beginning of the macro, where `path` is the path
/// to the `sticky-engine` crate.
///
/// # Behaviors
///
/// Behaviors are recognized by attribute, not by name. Exactly one function
/// must be marked `#[init]`; the following are optional:
///
/// - `#[destroy]` - Called before anything happens when the component is
///   about to be removed.
///
/// - `#[post_init]` - Called after the component, all its children, and all
///   its ancestors have initialized.
///
/// - `#[pre_phys]` (f32) - Called at a stable interval, depth-first, before
///   the physics engine runs. Children are processed after parents.
///
/// - `#[post_phys]` (f32) - Runs *after* `pre_phys` and the physics engine,
///   with children being processed *before* parents.
///
/// - `#[idle]` (f32) - Runs before the draw queue is created, even on
///   non-visual objects. Not suitable for game logic.
///
/// Each behavior's signature is checked against its expected shape via a
/// function pointer coercion: `#[init]` must be
/// `fn(ComponentParent, ComponentId<Self>) -> {Name}Init`, `#[destroy]` and
/// `#[post_init]` must be `fn(&mut self)`, and `#[pre_phys]`, `#[post_phys]`,
/// and `#[idle]` must be `fn(&mut self, f32)`.
///
/// # Components
///
/// All child Components must be provided through the `{Name}Init` struct,
/// regardless of modifier. `static` children are typically spawned inside
/// init via `Child::spawn(self_id.into(), info)`.
///
/// The macro generates getters and setters on the struct for each component,
/// depending on how it was written.
///
/// `static`:
///
/// - `get_name_id` - Returns the ID of `name`.
///
/// - `get_name` - Borrows `name`'s Component immediately.
///
/// - `get_name_mut` - Mutably borrows `name`'s Component immediately.
///
/// `dyn`:
///
/// - `get_name_id` - Returns the ID of `name`.
///
/// - `get_name` - Borrows `name`'s Component immediately.
///
/// - `get_name_mut` - Mutably borrows `name`'s Component immediately.
///
/// - `spawn_in_name` - Spawns a new component of the given type and replaces it
///   in `name`.
///
/// `dyn?`:
///
/// - `get_name_id` - Returns the ID of `name`, if it is present.
///
/// - `get_name` - Borrows `name`'s Component immediately, if it is present.
///
/// - `get_name_mut` - Mutably borrows `name`'s Component immediately, if it is
///   present.
///
/// - `spawn_in_name` - Spawns a new component of the given type and replaces it
///   in `name`.
///
/// - `clear_name` - Removes `name`.
///
/// - `has_name` - Returns whether `name` is present or not.
///
/// `dyn*`:
///
/// - `name_iter_ids` - Returns an iterator over all components in `name`.
///
/// - `name_len` - Returns the number of components in `name`.
///
/// - `name_get_id_at` - Returns the ID of the component at the given index in
///   `name`.
///
/// - `name_get_at` - Borrows the component at the given index in `name`
///   immediately.
///
/// - `name_get_at_mut` - Mutably borrows the component at the given index in
///   `name` immediately.
///
/// - `name_move_from` - Moves the component at the given index to a different
///   index in `name`, shifting elements to accommodate.
///
/// - `name_remove_id` - Removes the component in `name`, shifting elements to
///   accommodate.
///
/// - `name_remove_at` - Removes the component at the given index in `name`,
///   shifting elements to accommodate.
///
/// - `name_spawn_at` - Inserts a new component at the given index in `name`.
///
/// - `name_find_id` - Returns the index of a given component, if it is present
///   in `name`.
pub fn comp_def(input: TokenStream1) -> TokenStream1 {
    components::comp_def_inner(input)
}

fn random_name() -> String {
    Uuid::now_v7().simple().to_string()
}

/// Records a behavior function's ident, erroring if the slot is already filled.
fn set_behavior(
    slot: &mut Option<Ident>,
    fn_ident: Ident,
    attr_ident: &Ident,
    errors: &mut Option<syn::Error>,
) {
    if slot.is_some() {
        let err = syn::Error::new_spanned(attr_ident, format_args!("duplicate #[{}]", attr_ident));

        match errors {
            Some(errors) => errors.combine(err),
            None => *errors = Some(err),
        }

        return;
    }

    *slot = Some(fn_ident);
}

/// Adds an error at `span`, combining with any previously recorded errors.
fn add_behavior_error(errors: &mut Option<syn::Error>, span: Span2, message: String) {
    let err = syn::Error::new(span, message);

    match errors {
        Some(errors) => errors.combine(err),
        None => *errors = Some(err),
    }
}

/// Final path segment of a path type, if `ty` is one.
fn tail_path_segment(ty: &Type) -> Option<&PathSegment> {
    match ty {
        Type::Path(type_path) => type_path.path.segments.last(),
        _ => None,
    }
}

/// Whether `ty` is a path ending in `name` with no generic arguments.
fn is_plain_path(ty: &Type, name: &str) -> bool {
    matches!(
        tail_path_segment(ty),
        Some(segment)
            if segment.ident == name && matches!(segment.arguments, PathArguments::None)
    )
}

/// Whether `ty` is `&World` or `&SomePath::World`.
fn is_world_ref(ty: &Type) -> bool {
    let Type::Reference(r) = ty else {
        return false;
    };
    if r.mutability.is_some() {
        return false;
    }
    is_plain_path(&r.elem, "World")
}

/// Whether `ty` is `ComponentId<Self>` or `ComponentId<{struct_name}>`.
fn is_component_id_of(ty: &Type, struct_name: &Ident) -> bool {
    let Some(segment) = tail_path_segment(ty) else {
        return false;
    };

    if segment.ident != "ComponentId" {
        return false;
    }

    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };

    if args.args.len() != 1 {
        return false;
    }

    match args.args.first() {
        Some(GenericArgument::Type(inner)) => {
            let rendered = quote!(#inner).to_string();
            rendered == "Self" || *struct_name == rendered
        }
        _ => false,
    }
}