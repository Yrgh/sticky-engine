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
    let input_orig: TokenStream2 = input.clone().into();
    let input = parse_macro_input!(input as ItemTrait);

    if !input.generics.params.is_empty() {
        return error(input.generics.params, "generics are not allowed on Slots");
    }

    if input.generics.where_clause.is_some() {
        return error(
            input.generics.where_clause,
            "generics are not allowed on Slots",
        );
    }

    struct ParentCrateSpecifier(Option<(Token![in], Path)>);

    impl Parse for ParentCrateSpecifier {
        fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
            let lookahead = input.lookahead1();
            if lookahead.peek(Token![in]) {
                let token_in = input.parse()?;
                let path = input.parse()?;
                Ok(Self(Some((token_in, path))))
            } else {
                Ok(Self(None))
            }
        }
    }

    let engine_crate = if let Some((_, path)) = parse_macro_input!(attr as ParentCrateSpecifier).0 {
        path.into_token_stream()
    } else {
        let Ok(parent_crate_name) = crate_name("sticky-engine") else {
            return error(input_orig, "could not find sticky-engine crate");
        };

        match parent_crate_name {
            FoundCrate::Itself => quote! { crate },
            FoundCrate::Name(name) => quote! { ::#name },
        }
    };

    let mut input_out = input.clone();

    let slot_name = input.ident;

    input_out
        .supertraits
        .push(parse_quote! { #engine_crate::core::component::IComponent });

    let forgot_name = syn::Ident::new(
        &format!("YouForgotSlotImpl__{slot_name}"),
        Span2::call_site(),
    );

    input_out.supertraits.push(parse_quote! { #forgot_name });

    for item in &input.items {
        if !matches!(item, syn::TraitItem::Fn(_)) {
            return error(item, "only functions allowed in Slot traits");
        }
    }

    let module_name = syn::Ident::new(
        &format!("__slot_{slot_name}{}", random_name()),
        Span2::call_site(),
    );
    let slot_id_name = syn::Ident::new(&format!("{slot_name}Id"), Span2::call_site());

    let (visibility1, visibility2) = match input.vis {
        Visibility::Public(p) => (p.to_token_stream(), p.to_token_stream()),
        Visibility::Inherited => (quote! { pub(super) }, quote! {}),
        Visibility::Restricted(_) => {
            return error(
                input.vis,
                "restricted visibility is not supported; use either pub or inherited visibility",
            );
        }
    };

    let forgot_label = format!("this type doesn't implement `{forgot_name}`");
    let import_note =
        format!("you may need to import `{forgot_name}` from the same module as {slot_name}");
    let forgot_doc = format!("Enforces that {slot_name} is implemented via `#[slot_impl]`");
    let id_doc = format!("ID for Components implementing {slot_name}");

    quote! {
        #input_out

        #[doc(hidden)]
        #[doc = #forgot_doc]
        #[diagnostic::on_unimplemented(
            message = "`{Self}` is missing the `#[slot_impl]` attribute",
            label = #forgot_label,
            note = "add `#[slot_impl]` above the impl to add information about the slot",
            note = #import_note,
        )]
        pub trait #forgot_name {}

        mod #module_name {
            use super::*;
            use #engine_crate::core::component::*;
            use #engine_crate::core::world::World;

            #[doc = #id_doc]
            #visibility1 struct #slot_id_name {
                pidx: u32,
                gidx: u32,
                lidx: #engine_crate::core::level::LevelIndex,
                tyid: ::std::any::TypeId,
                conv: &'static #engine_crate::core::relations::Convert<dyn super::#slot_name>,
            }

            impl<C: ToDyn<dyn super::#slot_name> + IComponent>
                From<ComponentId<C>> for #slot_id_name
            {
                fn from(value: ComponentId<C>) -> Self {
                    let lidx = value.level_id();
                    let (pidx, gidx, tyid) = value.acquire_parts();

                    Self {
                        pidx,
                        gidx,
                        lidx,
                        tyid,
                        conv: #engine_crate::core::relations::RELATIONS
                            .get_convert::<#slot_id_name>(tyid)
                            .expect("bad reflection data"),
                    }
                }
            }

            impl Clone for #slot_id_name {
                fn clone(&self) -> Self {
                    Self {
                        pidx: self.pidx,
                        gidx: self.gidx,
                        lidx: self.lidx,
                        tyid: self.tyid,
                        conv: self.conv,
                    }
                }
            }

            unsafe impl ISlotId for #slot_id_name {
                type TraitObject = dyn super::#slot_name;

                unsafe fn from_parts(
                    pidx: u32,
                    gidx: u32,
                    lidx: #engine_crate::core::level::LevelIndex,
                    tyid: ::std::any::TypeId,
                ) -> Self where Self: Sized {
                    Self {
                        pidx,
                        gidx,
                        lidx,
                        tyid,
                        conv: #engine_crate::core::relations::RELATIONS
                            .get_convert::<#slot_id_name>(tyid)
                            .expect("bad reflection data"),
                    }
                }

                fn level_id(&self) -> #engine_crate::core::level::LevelIndex {
                    self.lidx
                }

                fn acquire_parts(&self) -> (u32, u32, ::std::any::TypeId) {
                    (self.pidx, self.gidx, self.tyid)
                }

                fn get<'w>(&self, world: &'w #engine_crate::core::world::World)
                    -> Result<::std::cell::Ref<'w, Self::TraitObject>, #engine_crate::core::ComponentGetError>
                {
                    Ok((self.conv.comp_to_t_ref)(
                        world
                            .get_level(self.lidx)
                            .ok_or(#engine_crate::core::ComponentGetError::NotFound)?
                            .acquire_component_internal_dyn(self.tyid, self.pidx, self.gidx)?
                    ))
                }

                fn get_mut<'w>(&self, world: &'w #engine_crate::core::world::World)
                    -> Result<::std::cell::RefMut<'w, Self::TraitObject>, #engine_crate::core::ComponentGetMutError>
                {
                    Ok((self.conv.comp_to_t_mut)(
                        world
                            .get_level(self.lidx)
                            .ok_or(#engine_crate::core::ComponentGetMutError::NotFound)?
                            .acquire_component_internal_dyn_mut(self.tyid, self.pidx, self.gidx)?
                    ))
                }
            }

            impl ISlotTr for dyn super::#slot_name {
                type Id = #slot_id_name;
            }

            impl From<#slot_id_name> for DynComponentId {
                fn from(value: #slot_id_name) -> Self {
                    unsafe {
                        DynComponentId::from_parts(value.pidx, value.gidx, value.lidx, value.tyid)
                    }
                }
            }

            impl ::std::cmp::PartialEq for #slot_id_name {
                fn eq(&self, other: &Self) -> bool {
                    self.pidx == other.pidx && self.gidx == other.gidx && self.lidx == other.lidx
                }
            }

            impl ::std::cmp::Eq for #slot_id_name {}

            impl ::std::hash::Hash for #slot_id_name {
                fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) {
                    state.write_u32(self.pidx);
                    state.write_u32(self.gidx);
                    self.lidx.hash(state);
                }
            }
        }

        #visibility2 use #module_name::#slot_id_name;
    }
    .into()
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
    let input_orig: TokenStream2 = input.clone().into();
    let input = parse_macro_input!(input as ItemImpl);

    if !input.generics.params.is_empty() {
        return error(input.generics.params, "generics are not allowed here");
    }

    if input.generics.where_clause.is_some() {
        return error(input.generics.where_clause, "generics are not allowed here");
    }

    let Some(trait_) = &input.trait_ else {
        return error(input, "expected impl Slot for Component");
    };

    struct ParentCrateSpecifier(Option<(Token![in], Path)>);

    impl Parse for ParentCrateSpecifier {
        fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
            let lookahead = input.lookahead1();
            if lookahead.peek(Token![in]) {
                let token_in = input.parse()?;
                let path = input.parse()?;
                Ok(Self(Some((token_in, path))))
            } else {
                Ok(Self(None))
            }
        }
    }

    let engine_crate = if let Some((_, path)) = parse_macro_input!(attr as ParentCrateSpecifier).0 {
        path.into_token_stream()
    } else {
        let Ok(parent_crate_name) = crate_name("sticky-engine") else {
            return error(input_orig, "could not find sticky-engine crate");
        };

        match parent_crate_name {
            FoundCrate::Itself => quote! { crate },
            FoundCrate::Name(name) => quote! { ::#name },
        }
    };

    let comp_ty = &input.self_ty;
    let slot_tr = &trait_.0;

    let fn_name = syn::Ident::new(&format!("__impl_{}", random_name()), Span2::call_site());

    let mut forgot_name = slot_tr.clone();
    let last = forgot_name.segments.last_mut().unwrap();
    last.ident = syn::Ident::new(
        &format!("YouForgotSlotImpl__{}", last.ident),
        Span2::call_site(),
    );

    let mod_name = syn::Ident::new(&format!("__mod_{}", random_name()), Span2::call_site());

    quote! {
        #input

        mod #mod_name {
            use super::*;
            use #engine_crate::core::relations::*;

            impl #engine_crate::core::component::ToDyn<dyn #slot_tr> for #comp_ty {
                fn to_dyn(&self) -> &(dyn #slot_tr + 'static) {
                    self
                }

                fn to_dyn_mut(&mut self) -> &mut (dyn #slot_tr + 'static) {
                    self
                }
            }

            impl #forgot_name for #comp_ty {}

            #[distributed_slice(SLOT_IMPLS)]
            fn #fn_name() -> (
                ::std::any::TypeId,
                ::std::any::TypeId,
                Box<dyn ::std::any::Any + Send + Sync>
            ) {
                use ::std::cell::{Ref, RefMut};
                let tyid = ::std::any::TypeId::of::<#comp_ty>();
                let trid = ::std::any::TypeId::of::<dyn #slot_tr>();
                let conv = Convert::<dyn #slot_tr> {
                    comp_to_t_ref: |r| -> Ref<'_, dyn #slot_tr> {
                        Ref::map(r, |r| r.downcast_ref::<#comp_ty>().expect("bad cast"))
                    },
                    comp_to_t_mut: |r| -> RefMut<'_, dyn #slot_tr> {
                        RefMut::map(r, |r| r.downcast_mut::<#comp_ty>().expect("bad cast"))
                    },
                };

                (tyid, trid, Box::new(conv))
            }
        }
    }
    .into()
}

enum ChildComponentModifier {
    Static,
    DynOne,
    DynOpt,
    DynPlural,
}

impl Parse for ChildComponentModifier {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let lookahead = input.lookahead1();

        if lookahead.peek(Token![static]) {
            let _s: Token![static] = input.parse()?;
            return Ok(ChildComponentModifier::Static);
        }

        if !lookahead.peek(Token![dyn]) {
            return Err(lookahead.error());
        }

        let _d: Token![dyn] = input.parse()?;

        let lookahead = input.lookahead1();

        if lookahead.peek(Token![?]) {
            let _q: Token![?] = input.parse()?;
            return Ok(ChildComponentModifier::DynOpt);
        }

        if lookahead.peek(Token![*]) {
            let _a: Token![*] = input.parse()?;
            return Ok(ChildComponentModifier::DynPlural);
        }

        Ok(ChildComponentModifier::DynOne)
    }
}

struct ComponentsSectionItem {
    modifier: ChildComponentModifier,
    name: Ident,
    colon: Token![:],
    id_ty: Type,
}

impl Parse for ComponentsSectionItem {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let modifier = input.parse()?;

        let name = input.parse()?;

        let colon = input.parse()?;

        let id_ty = input.parse()?;

        Ok(ComponentsSectionItem {
            modifier,
            name,
            colon,
            id_ty,
        })
    }
}

struct VariablesSectionItem {
    vis: Visibility,
    name: Ident,
    colon: Token![:],
    ty: Type,
}

impl Parse for VariablesSectionItem {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let vis_orig: Visibility = input.parse()?;

        let name = input.parse()?;

        let colon = input.parse()?;

        let ty = input.parse()?;

        let vis = match vis_orig {
            Visibility::Public(_) => vis_orig,
            Visibility::Inherited => Visibility::Restricted(VisRestricted {
                pub_token: Token![pub](vis_orig.span()),
                paren_token: token::Paren(vis_orig.span()),
                in_token: None,
                path: Box::new(Token![super](vis_orig.span()).into()),
            }),
            Visibility::Restricted(_) => {
                return Err(syn::Error::new(
                    vis_orig.span(),
                    "restricted visibility is not supported",
                ));
            }
        };

        Ok(VariablesSectionItem {
            vis,
            name,
            colon,
            ty,
        })
    }
}

impl ToTokens for VariablesSectionItem {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        self.vis.to_tokens(tokens);
        self.name.to_tokens(tokens);
        self.colon.to_tokens(tokens);
        self.ty.to_tokens(tokens);
    }
}

struct EngineSpecifier {
    path: Path,
}

impl Parse for EngineSpecifier {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let content;
        let _paren = parenthesized!(content in input);
        let _in_token: Token![in] = content.parse()?;
        let path = content.parse()?;

        Ok(Self { path })
    }
}

struct ComponentDef {
    engine_specifier: Option<EngineSpecifier>,
    vis: Visibility,
    struct_token: Token![struct],
    name: Ident,
    components_section_items: Punctuated<ComponentsSectionItem, Token![,]>,
    variables_section_items: Punctuated<VariablesSectionItem, Token![,]>,
    behaviors_section_items: Vec<ImplItem>,
    behaviors_uses: Vec<ItemUse>,
}

impl Parse for ComponentDef {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let engine_specifier = if input.peek(token::Paren) {
            Some(input.parse()?)
        } else {
            None
        };

        let vis_orig = input.parse()?;

        let vis = match vis_orig {
            Visibility::Public(_) => vis_orig,
            Visibility::Inherited => Visibility::Restricted(VisRestricted {
                pub_token: Token![pub](vis_orig.span()),
                paren_token: token::Paren(vis_orig.span()),
                in_token: None,
                path: Box::new(Token![super](vis_orig.span()).into()),
            }),
            Visibility::Restricted(_) => {
                return Err(syn::Error::new(
                    vis_orig.span(),
                    "restricted visibility is not supported",
                ));
            }
        };

        let struct_token = input.parse()?;

        let name = input.parse()?;

        let content;
        let _outer_braces = braced!(content in input);

        let Ok(components_section_keyword) = content.parse::<Ident>() else {
            return Err(syn::Error::new(content.span(), "expected 'components'"));
        };
        let components_label = components_section_keyword.to_string();
        if components_label != "components" {
            return Err(syn::Error::new_spanned(
                components_section_keyword,
                format_args!(
                    "expected the first section to be labeled 'components', found '{components_label}'",
                ),
            ));
        }

        let components_section;
        let _components_section_braces = braced!(components_section in content);

        let components_section_items =
            components_section.parse_terminated(ComponentsSectionItem::parse, Token![,])?;

        let Ok(variables_section_keyword) = content.parse::<Ident>() else {
            return Err(syn::Error::new(content.span(), "expected 'variables'"));
        };
        let variables_label = variables_section_keyword.to_string();
        if variables_label != "variables" {
            return Err(syn::Error::new_spanned(
                variables_section_keyword,
                format_args!(
                    "expected the second section to be labeled 'variables', found '{variables_label}'"
                ),
            ));
        }
        let variables_section;
        let _variables_section_braces = braced!(variables_section in content);

        let variables_section_items =
            variables_section.parse_terminated(VariablesSectionItem::parse, Token![,])?;

        let Ok(behaviors_section_keyword) = content.parse::<Ident>() else {
            return Err(syn::Error::new(content.span(), "expected 'behaviors'"));
        };
        let behaviors_label = behaviors_section_keyword.to_string();
        if behaviors_label != "behaviors" {
            return Err(syn::Error::new_spanned(
                behaviors_section_keyword,
                format_args!(
                    "expected the third section to be labeled 'behaviors', found '{behaviors_label}'"
                ),
            ));
        }
        let behaviors_section;
        let _behaviors_section_braces = braced!(behaviors_section in content);

        let mut behaviors_section_items = Vec::new();
        let mut behaviors_uses = Vec::new();

        while !behaviors_section.is_empty() {
            if behaviors_section.peek(syn::Token![use]) {
                behaviors_uses.push(behaviors_section.parse()?);
            } else {
                behaviors_section_items.push(behaviors_section.parse()?);
            }
        }

        Ok(ComponentDef {
            engine_specifier,
            vis,
            struct_token,
            name,
            components_section_items,
            variables_section_items,
            behaviors_section_items,
            behaviors_uses,
        })
    }
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
    let input_orig: TokenStream2 = input.clone().into();
    let ComponentDef {
        engine_specifier,
        vis,
        struct_token,
        name,
        components_section_items,
        variables_section_items,
        mut behaviors_section_items,
        behaviors_uses,
        ..
    } = parse_macro_input!(input as ComponentDef);

    let vis2 = match &vis {
        Visibility::Public(_) => vis.clone(),
        Visibility::Inherited => unreachable!(),
        Visibility::Restricted(_) => Visibility::Inherited,
    };

    // Actual structure placed in a module, exported. Components are fully private, variables have
    // +super visibility.
    //
    // Behaviors are then copied outside into an impl block. Any specified behaviors will then be
    // used to implement IComponent

    let engine_crate = if let Some(es) = engine_specifier {
        es.path.to_token_stream()
    } else {
        let Ok(parent_crate_name) = crate_name("sticky-engine") else {
            return error(input_orig, "could not find sticky-engine crate");
        };

        match parent_crate_name {
            FoundCrate::Itself => quote! { crate },
            FoundCrate::Name(name) => quote! { ::#name },
        }
    };

    let comp_mod = quote! { #engine_crate::core::component };

    let mod_name = syn::Ident::new(&format!("__{name}_{}", random_name()), Span2::call_site());

    let mut fields = Vec::new();
    let mut gen_gs_fns = Vec::new();
    let mut static_verifications = Vec::new();

    let mut initer_fields = Vec::new();
    let mut init_final = Vec::new();

    let mut iter_children = Vec::new();

    {
        // Self
        fields.push(syn::Field {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            modifiers: FieldModifiers::default(),
            ident: Some(syn::Ident::new("c_self", Span2::call_site())),
            colon_token: Some(Token![:](Span2::call_site())),
            ty: parse_quote! { #comp_mod::ComponentId<Self> },
            default: None,
        });

        gen_gs_fns.push(quote! {
            pub fn get_id(&self) -> #comp_mod::ComponentId<Self> {
                self.c_self.clone()
            }
        });
    }

    {
        // Parent
        fields.push(syn::Field {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            modifiers: FieldModifiers::default(),
            ident: Some(syn::Ident::new("c_parent", Span2::call_site())),
            colon_token: Some(Token![:](Span2::call_site())),
            ty: parse_quote! { #comp_mod::ComponentParent },
            default: None,
        });
    }

    for ComponentsSectionItem {
        modifier,
        name,
        colon,
        id_ty,
    } in components_section_items
    {
        let id_ty_span = id_ty.span();

        let id_ty: Type = parse_quote! { <#id_ty as #comp_mod::ISlotTr>::Id };
        let stored = match modifier {
            ChildComponentModifier::Static | ChildComponentModifier::DynOne => id_ty.clone(),
            ChildComponentModifier::DynOpt => parse_quote! { Option<#id_ty> },
            ChildComponentModifier::DynPlural => parse_quote! { Vec<#id_ty> },
        };

        fields.push(syn::Field {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            modifiers: FieldModifiers::default(),
            ident: Some(name.clone()),
            colon_token: Some(colon),
            ty: stored.clone(),
            default: None,
        });

        match modifier {
            ChildComponentModifier::Static => {
                let getter_name = syn::Ident::new(&format!("get_{name}_id"), Span2::call_site());
                let get_name = syn::Ident::new(&format!("get_{name}"), Span2::call_site());
                let get_name_mut = syn::Ident::new(&format!("get_{name}_mut"), Span2::call_site());

                gen_gs_fns.push(quote! {
                    pub fn #getter_name(&self) -> #stored {
                        self.#name.clone()
                    }

                    #[doc = "Borrows the child Component immediately.\n\n# Errors\n\nReturns Err if the child Component is already mutably borrowed."]
                    pub fn #get_name<'w>(
                        &self,
                        world: &'w #engine_crate::core::world::World,
                    ) -> ::std::result::Result<
                        ::std::cell::Ref<'w, <#id_ty as #comp_mod::ISlotId>::TraitObject>,
                        ::std::cell::BorrowError,
                    > {
                        match self.#name.get(world) {
                            Ok(tr) => Ok(tr),
                            Err(#engine_crate::core::ComponentGetError::NotFound) => {
                                panic!("child component should always be accessible")
                            }
                            Err(#engine_crate::core::ComponentGetError::BorrowError(err)) => {
                                Err(err)
                            }
                        }
                    }

                    #[doc = "Mutably borrows the child Component immediately.\n\n# Errors\n\nReturns Err if the child Component is already borrowed."]
                    pub fn #get_name_mut<'w>(
                        &self,
                        world: &'w #engine_crate::core::world::World,
                    ) -> ::std::result::Result<
                        ::std::cell::RefMut<'w, <#id_ty as #comp_mod::ISlotId>::TraitObject>,
                        ::std::cell::BorrowMutError,
                    > {
                        match self.#name.get_mut(world) {
                            Ok(tr) => Ok(tr),
                            Err(#engine_crate::core::ComponentGetMutError::NotFound) => {
                                panic!("child component should always be accessible")
                            }
                            Err(#engine_crate::core::ComponentGetMutError::BorrowMutError(err)) => {
                                Err(err)
                            }
                        }
                    }
                });

                static_verifications.push(quote_spanned! {
                    id_ty_span =>
                    const _: () = _assert_valid_static::<#id_ty>();
                });

                initer_fields.push(quote! {
                    pub #name: #stored,
                });

                init_final.push(quote! {
                    #name: init.#name
                });

                iter_children.push(quote! {
                    .chain(::std::iter::once(self.#name.clone().into()))
                });
            }
            ChildComponentModifier::DynOne => {
                let get_name_id = syn::Ident::new(&format!("get_{name}_id"), Span2::call_site());
                let get_name = syn::Ident::new(&format!("get_{name}"), Span2::call_site());
                let get_name_mut = syn::Ident::new(&format!("get_{name}_mut"), Span2::call_site());
                let spawn_in_name =
                    syn::Ident::new(&format!("spawn_in_{name}"), Span2::call_site());

                gen_gs_fns.push(quote! {
                    pub fn #get_name_id(&self) -> #stored {
                        self.#name.clone()
                    }

                    #[doc = "Borrows the child Component immediately.\n\n# Errors\n\nReturns Err if the child Component is already mutably borrowed."]
                    pub fn #get_name<'w>(
                        &self,
                        world: &'w #engine_crate::core::world::World,
                    ) -> ::std::result::Result<
                        ::std::cell::Ref<'w, <#id_ty as #comp_mod::ISlotId>::TraitObject>,
                        ::std::cell::BorrowError,
                    > {
                        match self.#name.get(world) {
                            Ok(tr) => Ok(tr),
                            Err(#engine_crate::core::ComponentGetError::NotFound) => {
                                panic!("child component should always be accessible")
                            }
                            Err(#engine_crate::core::ComponentGetError::BorrowError(err)) => {
                                Err(err)
                            }
                        }
                    }

                    #[doc = "Mutably borrows the child Component immediately.\n\n# Errors\n\nReturns Err if the child Component is already borrowed."]
                    pub fn #get_name_mut<'w>(
                        &self,
                        world: &'w #engine_crate::core::world::World,
                    ) -> ::std::result::Result<
                        ::std::cell::RefMut<'w, <#id_ty as #comp_mod::ISlotId>::TraitObject>,
                        ::std::cell::BorrowMutError,
                    > {
                        match self.#name.get_mut(world) {
                            Ok(tr) => Ok(tr),
                            Err(#engine_crate::core::ComponentGetMutError::NotFound) => {
                                panic!("child component should always be accessible")
                            }
                            Err(#engine_crate::core::ComponentGetMutError::BorrowMutError(err)) => {
                                Err(err)
                            }
                        }
                    }

                    /// Replaces the existing Component with a newly `spawn`ed one.
                    ///
                    /// # Borrows
                    /// Mutably borrows the old Component.
                    pub fn #spawn_in_name<C>(
                        &mut self,
                        world: &#engine_crate::core::world::World,
                        info: C::SpawnInfo,
                    )
                    where
                        C: #comp_mod::ToDyn<<#stored as #comp_mod::ISlotId>::TraitObject>
                            + #comp_mod::IComponent,
                        #comp_mod::ComponentId<C>: Into<#id_ty>
                    {
                        Self::c_destroy_child(world, &self.#name);

                        self.#name = Self::c_spawn_child(world, self.c_self.clone().into(), info);
                    }
                });

                initer_fields.push(quote! {
                    pub #name: #stored,
                });

                init_final.push(quote! {
                    #name: init.#name
                });

                iter_children.push(quote! {
                    .chain(::std::iter::once(self.#name.clone().into()))
                });
            }
            ChildComponentModifier::DynOpt => {
                let get_name_id = syn::Ident::new(&format!("get_{name}_id"), Span2::call_site());
                let get_name = syn::Ident::new(&format!("get_{name}"), Span2::call_site());
                let get_name_mut = syn::Ident::new(&format!("get_{name}_mut"), Span2::call_site());
                let spawn_in_name =
                    syn::Ident::new(&format!("spawn_in_{name}"), Span2::call_site());
                let clear_name = syn::Ident::new(&format!("clear_{name}"), Span2::call_site());
                let has_name = syn::Ident::new(&format!("has_{name}"), Span2::call_site());

                gen_gs_fns.push(quote! {
                    pub fn #get_name_id(&self) -> #stored {
                        self.#name.clone()
                    }

                    #[doc = "Borrows the child Component immediately, if present.\n\nReturns None if no Component is present.\n\n# Errors\n\nThe inner Result returns Err if the child Component is already mutably borrowed."]
                    pub fn #get_name<'w>(
                        &self,
                        world: &'w #engine_crate::core::world::World,
                    ) -> ::std::option::Option<::std::result::Result<
                        ::std::cell::Ref<'w, <#id_ty as #comp_mod::ISlotId>::TraitObject>,
                        ::std::cell::BorrowError,
                    >> {
                        Some(match self.#name.as_ref()?.get(world) {
                            Ok(tr) => Ok(tr),
                            Err(#engine_crate::core::ComponentGetError::NotFound) => {
                                panic!("child component should always be accessible")
                            }
                            Err(#engine_crate::core::ComponentGetError::BorrowError(err)) => {
                                Err(err)
                            }
                        })
                    }

                    #[doc = "Mutably borrows the child Component immediately, if present.\n\nReturns None if no Component is present.\n\n# Errors\n\nThe inner Result returns Err if the child Component is already borrowed."]
                    pub fn #get_name_mut<'w>(
                        &self,
                        world: &'w #engine_crate::core::world::World,
                    ) -> ::std::option::Option<::std::result::Result<
                        ::std::cell::RefMut<'w, <#id_ty as #comp_mod::ISlotId>::TraitObject>,
                        ::std::cell::BorrowMutError,
                    >> {
                        Some(match self.#name.as_ref()?.get_mut(world) {
                            Ok(tr) => Ok(tr),
                            Err(#engine_crate::core::ComponentGetMutError::NotFound) => {
                                panic!("child component should always be accessible")
                            }
                            Err(#engine_crate::core::ComponentGetMutError::BorrowMutError(err)) => {
                                Err(err)
                            }
                        })
                    }

                    #[doc = "Spawns a new Component of the given type and replaces the previous one, if present.\n\n# Borrows\n\nMutably borrows the replaced child Component and all of its descendants, recursively, while destroying it."]
                    pub fn #spawn_in_name<C>(
                        &mut self,
                        world: &#engine_crate::core::world::World,
                        info: C::SpawnInfo,
                    )
                    where
                        C: #comp_mod::ToDyn<<#id_ty as #comp_mod::ISlotId>::TraitObject>
                            + #comp_mod::IComponent,
                        #comp_mod::ComponentId<C>: Into<#id_ty>
                    {
                        if let Some(old_id) = &self.#name {
                            Self::c_destroy_child(world, old_id);
                        }

                        self.#name =
                            Some(Self::c_spawn_child(world, self.c_self.clone().into(), info));
                    }

                    #[doc = "Clears the child Component, if present.\n\n# Borrows\n\nMutably borrows the removed child Component and all of its descendants, recursively, while destroying it."]
                    pub fn #clear_name(
                        &mut self,
                        world: &#engine_crate::core::world::World,
                    ) -> bool {
                        match &self.#name {
                            Some(old_id) => {
                                Self::c_destroy_child(world, old_id);
                                self.#name = None;
                                true
                            }
                            None => false,
                        }
                    }

                    pub fn #has_name(&self) -> bool {
                        self.#name.is_some()
                    }
                });

                initer_fields.push(quote! {
                    pub #name: #stored,
                });

                init_final.push(quote! {
                    #name: init.#name
                });

                iter_children.push(quote! {
                    .chain(self.#name.clone().into_iter().map(Into::into))
                });
            }
            ChildComponentModifier::DynPlural => {
                let name_iter_ids =
                    syn::Ident::new(&format!("{name}_iter_ids"), Span2::call_site());
                let name_remove_id =
                    syn::Ident::new(&format!("{name}_remove_id"), Span2::call_site());
                let name_remove_at =
                    syn::Ident::new(&format!("{name}_remove_at"), Span2::call_site());
                let name_find_id = syn::Ident::new(&format!("{name}_find_id"), Span2::call_site());
                let name_spawn_at =
                    syn::Ident::new(&format!("{name}_spawn_at"), Span2::call_site());
                let name_move_from =
                    syn::Ident::new(&format!("{name}_move_from"), Span2::call_site());
                let name_get_at = syn::Ident::new(&format!("{name}_get_id_at"), Span2::call_site());
                let name_get_at_tr =
                    syn::Ident::new(&format!("{name}_get_at"), Span2::call_site());
                let name_get_at_mut_tr =
                    syn::Ident::new(&format!("{name}_get_at_mut"), Span2::call_site());
                let name_len = syn::Ident::new(&format!("{name}_len"), Span2::call_site());

                gen_gs_fns.push(quote! {
                    pub fn #name_iter_ids(&self) -> impl Iterator<Item=&#id_ty> {
                        self.#name.iter()
                    }

                    pub fn #name_len(&self) -> usize {
                        self.#name.len()
                    }

                    pub fn #name_get_at(&self, index: usize) -> Option<&#id_ty> {
                        self.#name.get(index)
                    }

                    #[doc = "Borrows the child Component at the given index immediately.\n\nReturns None if the index is out of bounds.\n\n# Errors\n\nThe inner Result returns Err if the child Component is already mutably borrowed."]
                    pub fn #name_get_at_tr<'w>(
                        &self,
                        world: &'w #engine_crate::core::world::World,
                        index: usize,
                    ) -> ::std::option::Option<::std::result::Result<
                        ::std::cell::Ref<'w, <#id_ty as #comp_mod::ISlotId>::TraitObject>,
                        ::std::cell::BorrowError,
                    >> {
                        Some(match self.#name.get(index)?.get(world) {
                            Ok(tr) => Ok(tr),
                            Err(#engine_crate::core::ComponentGetError::NotFound) => {
                                panic!("child component should always be accessible")
                            }
                            Err(#engine_crate::core::ComponentGetError::BorrowError(err)) => {
                                Err(err)
                            }
                        })
                    }

                    #[doc = "Mutably borrows the child Component at the given index immediately.\n\nReturns None if the index is out of bounds.\n\n# Errors\n\nThe inner Result returns Err if the child Component is already borrowed."]
                    pub fn #name_get_at_mut_tr<'w>(
                        &self,
                        world: &'w #engine_crate::core::world::World,
                        index: usize,
                    ) -> ::std::option::Option<::std::result::Result<
                        ::std::cell::RefMut<'w, <#id_ty as #comp_mod::ISlotId>::TraitObject>,
                        ::std::cell::BorrowMutError,
                    >> {
                        Some(match self.#name.get(index)?.get_mut(world) {
                            Ok(tr) => Ok(tr),
                            Err(#engine_crate::core::ComponentGetMutError::NotFound) => {
                                panic!("child component should always be accessible")
                            }
                            Err(#engine_crate::core::ComponentGetMutError::BorrowMutError(err)) => {
                                Err(err)
                            }
                        })
                    }

                    pub fn #name_find_id(&self, id: &#id_ty) -> Option<usize> {
                        self.#name.iter().position(|id2| id2 == id)
                    }

                    #[doc = "Removes the Component at the given index.\n\n# Borrows\n\nMutably borrows the removed child Component and all of its descendants, recursively, while destroying it."]
                    pub fn #name_remove_at(
                        &mut self,
                        world: &#engine_crate::core::world::World,
                        index: usize,
                    ) -> bool {
                        if index >= self.#name.len() {
                            return false;
                        }

                        let old_id = self.#name.remove(index);

                        Self::c_destroy_child(world, &old_id);

                        true
                    }

                    #[doc = "Removes the given Component, if it is present.\n\n# Borrows\n\nMutably borrows the removed child Component and all of its descendants, recursively, while destroying it."]
                    pub fn #name_remove_id(
                        &mut self,
                        world: &#engine_crate::core::world::World,
                        id: &#id_ty,
                    ) -> bool {
                        if let Some(index) = self.#name_find_id(id) {
                            self.#name_remove_at(world, index)
                        } else {
                            false
                        }
                    }

                    #[doc = "Inserts a new Component at the given index.\n\n# Borrows\n\nMutably borrows the new Component's slot in its Level while spawning."]
                    pub fn #name_spawn_at<C>(
                        &mut self,
                        world: &#engine_crate::core::world::World,
                        index: usize,
                        info: C::SpawnInfo,
                    )
                    where
                        C: #comp_mod::ToDyn<<#id_ty as #comp_mod::ISlotId>::TraitObject>
                            + #comp_mod::IComponent,
                        #comp_mod::ComponentId<C>: Into<#id_ty>
                    {
                        let new_id: #id_ty =
                            Self::c_spawn_child(world, self.c_self.clone().into(), info);

                        self.#name.insert(index, new_id);
                    }

                    pub fn #name_move_from(&mut self, src: usize, dst: usize) {
                        let old = self.#name.remove(src);
                        self.#name.insert(dst, old);
                    }
                });

                init_final.push(quote! {
                    #name: init.#name
                });

                initer_fields.push(quote! {
                    pub #name: #stored,
                });

                iter_children.push(quote! {
                    .chain(self.#name.clone().into_iter().map(Into::into))
                });
            }
        }
    }

    for VariablesSectionItem {
        vis,
        name,
        colon,
        ty,
    } in variables_section_items
    {
        fields.push(syn::Field {
            attrs: Vec::new(),
            vis,
            modifiers: FieldModifiers::default(),
            ident: Some(name.clone()),
            colon_token: Some(colon),
            ty: ty.clone(),
            default: None,
        });

        initer_fields.push(quote! {
            pub #name: #ty
        });

        init_final.push(quote! {
            #name: init.#name
        });
    }

    let initer_name = syn::Ident::new(&format!("{name}Init"), Span2::call_site());

    const BEHAVIOR_ATTRS: [&str; 6] = [
        "init",
        "destroy",
        "post_init",
        "pre_phys",
        "post_phys",
        "idle",
    ];

    let mut user_init = None;
    let mut user_destroy = None;
    let mut user_post_init = None;
    let mut user_pre_phys = None;
    let mut user_post_phys = None;
    let mut user_idle = None;

    let mut behavior_checks = Vec::new();
    let mut behavior_errors: Option<syn::Error> = None;
    let mut spawn_info_ty: Option<Type> = None;
    let mut trait_hook_items: Vec<ImplItem> = Vec::new();
    let mut moved_to_trait: Vec<usize> = Vec::new();

    for (idx, item) in behaviors_section_items.iter_mut().enumerate() {
        let ImplItem::Fn(func) = item else {
            continue;
        };

        let fn_ident = func.sig.ident.clone();

        let mut markers: Vec<Ident> = Vec::new();

        func.attrs.retain(|attr| match attr.path().get_ident() {
            Some(attr_ident) if BEHAVIOR_ATTRS.contains(&attr_ident.to_string().as_str()) => {
                markers.push(attr_ident.clone());
                false
            }
            _ => true,
        });

        if markers.len() > 1 {
            let err = syn::Error::new_spanned(
                &func.sig.ident,
                "only one behavior attribute is allowed per function",
            );

            match &mut behavior_errors {
                Some(errors) => errors.combine(err),
                None => behavior_errors = Some(err),
            }

            continue;
        }

        for attr_ident in &markers {
            let check = match attr_ident.to_string().as_str() {
                "init" => {
                    set_behavior(
                        &mut user_init,
                        fn_ident.clone(),
                        attr_ident,
                        &mut behavior_errors,
                    );

                    let attr_span = attr_ident.span();
                    let sig = &func.sig;

                    let mut sig_ok = true;

                    if sig.variadic.is_some() || sig.inputs.len() != 4 {
                        add_behavior_error(
                            &mut behavior_errors,
                            attr_span,
                            "#[init] must take exactly four arguments".to_string(),
                        );
                        sig_ok = false;
                    }

                    for arg in &sig.inputs {
                        if matches!(arg, FnArg::Receiver(_)) {
                            add_behavior_error(
                                &mut behavior_errors,
                                attr_span,
                                "#[init] must not take a receiver".to_string(),
                            );
                            sig_ok = false;
                        }
                    }

                    if sig_ok {
                        let tys: Vec<&Type> = sig
                            .inputs
                            .iter()
                            .map(|arg| match arg {
                                FnArg::Typed(arg) => arg.ty.as_ref(),
                                FnArg::Receiver(_) => unreachable!(),
                            })
                            .collect();

                        if !is_world_ref(tys[0]) {
                            add_behavior_error(
                                &mut behavior_errors,
                                attr_span,
                                "the first parameter of #[init] must have type `&World`"
                                    .to_string(),
                            );
                        }

                        if !is_plain_path(tys[1], "ComponentParent") {
                            add_behavior_error(
                                &mut behavior_errors,
                                attr_span,
                                "the second parameter of #[init] must have type `ComponentParent`"
                                    .to_string(),
                            );
                        }

                        if !is_component_id_of(tys[2], &name) {
                            add_behavior_error(
                                &mut behavior_errors,
                                attr_span,
                                "the third parameter of #[init] must have type `ComponentId<Self>`"
                                    .to_string(),
                            );
                        }

                        let ret_ok = match &sig.output {
                            ReturnType::Type(_, ty) => is_plain_path(ty, &initer_name.to_string()),
                            ReturnType::Default => false,
                        };

                        if !ret_ok {
                            add_behavior_error(
                                &mut behavior_errors,
                                attr_span,
                                format!("#[init] must return `{initer_name}`"),
                            );
                        }

                        spawn_info_ty = Some(tys[3].clone());
                    }

                    let info_ty =
                        spawn_info_ty.clone().unwrap_or_else(|| parse_quote! { () });

                    quote_spanned! { attr_span=>
                        const _: fn(
                            &#engine_crate::core::world::World,
                            #comp_mod::ComponentParent,
                            #comp_mod::ComponentId<#name>,
                            #info_ty,
                        ) -> #initer_name = #name::#fn_ident;
                    }
                }
                "destroy" | "post_init" | "pre_phys" | "post_phys" | "idle" => {
                    let hook_ident = syn::Ident::new(&format!("{attr_ident}_hook"), fn_ident.span());

                    let slot = match attr_ident.to_string().as_str() {
                        "destroy" => &mut user_destroy,
                        "post_init" => &mut user_post_init,
                        "pre_phys" => &mut user_pre_phys,
                        "post_phys" => &mut user_post_phys,
                        _ => &mut user_idle,
                    };

                    set_behavior(slot, fn_ident.clone(), attr_ident, &mut behavior_errors);

                    let mut renamed = func.clone();
                    renamed.sig.ident = hook_ident.clone();

                    trait_hook_items.push(ImplItem::Fn(renamed));

                    let takes_delta =
                        matches!(attr_ident.to_string().as_str(), "pre_phys" | "post_phys" | "idle");

                    if takes_delta {
                        quote_spanned! { attr_ident.span()=>
                            const _: fn(&mut #name, f32) = #name::#hook_ident;
                        }
                    } else {
                        quote_spanned! { attr_ident.span()=>
                            const _: fn(&mut #name) = #name::#hook_ident;
                        }
                    }
                }
                _ => unreachable!(),
            };

            behavior_checks.push(check);
        }

        if markers.iter().any(|m| m != "init") {
            moved_to_trait.push(idx);
        }
    }

    for idx in moved_to_trait.into_iter().rev() {
        behaviors_section_items.remove(idx);
    }

    if let Some(errors) = behavior_errors {
        return errors.to_compile_error().into();
    }

    let Some(user_init) = user_init else {
        return error(input_orig, "no function marked #[init] found in behaviors");
    };

    let spawn_info_ty = spawn_info_ty.unwrap_or_else(|| parse_quote! { () });

    let out = quote! {
        mod #mod_name {
            use super::*;

            pub(super) struct #initer_name {
                #(#initer_fields)*
            }

            #vis #struct_token #name {
                #(#fields),*
            }

            impl #name {
                /// (**INTERNAL**) Calls [`destroy`](IComponent::destroy) on the
                /// child Component, then removes it from its Level.
                fn c_destroy_child<I>(world: &#engine_crate::core::world::World, id: &I)
                where
                    I: #comp_mod::ISlotId,
                {
                    use #comp_mod::ISlotId;

                    let mut child = id.get_mut(world).expect("removed by other source");
                    child.destroy(world);
                    drop(child);

                    let (pidx, gidx, tyid) = ISlotId::acquire_parts(id);
                    let lvl = world
                        .get_level(ISlotId::level_id(id))
                        .expect("level destroyed");

                    lvl.remove_component_internal(tyid, pidx, gidx);
                }

                /// (**INTERNAL**) Spawns a new child Component under `parent`, runs its
                /// `post_init` traversal, then converts the returned ID into `I`.
                fn c_spawn_child<C, I>(
                    world: &#engine_crate::core::world::World,
                    parent: #comp_mod::ComponentParent,
                    info: C::SpawnInfo,
                ) -> I
                where
                    I: #comp_mod::ISlotId,
                    C: #comp_mod::ToDyn<<I as #comp_mod::ISlotId>::TraitObject>
                        + #comp_mod::IComponent,
                    #comp_mod::ComponentId<C>: Into<I>,
                {
                    let cid = <C as #comp_mod::IComponent>::spawn(world, parent, info);

                    cid.get_mut(world).expect("just spawned").post_init(world);

                    cid.into()
                }

                #(#gen_gs_fns)*
            }

            unsafe impl #comp_mod::IComponent for #name {
                fn parent_id(&self) -> #comp_mod::ComponentParent {
                    self.c_parent.clone()
                }

                fn children(&self) -> Box<dyn Iterator<Item=DynComponentId>> {
                    Box::new(::std::iter::empty()#(#iter_children)*)
                }

                type SpawnInfo = #spawn_info_ty;

                fn spawn(
                    world: &#engine_crate::core::world::World,
                    parent: #comp_mod::ComponentParent,
                    info: #spawn_info_ty,
                ) -> #comp_mod::ComponentId<Self>
                where
                    Self: Sized
                {
                    let lvl_id = parent.level_id();
                    let lvl = world
                        .get_level(lvl_id)
                        .expect("level destroyed");
                    let self_id = lvl.reserve_slot_internal::<Self>();

                    let init = Self::#user_init(world, parent.clone(), self_id.clone(), info);

                    let self_ = Self {
                        c_self: self_id.clone(),
                        c_parent: parent,
                        #(#init_final),*
                    };

                    let lvl = world
                        .get_level(lvl_id)
                        .expect("level destroyed");
                    let (pidx, gidx, _) = #comp_mod::ISlotId::acquire_parts(&self_id);

                    lvl.fill_slot_internal(pidx, gidx, self_);

                    self_id
                }

                #(#trait_hook_items)*
            }

            const fn _assert_valid_static<T: #comp_mod::StaticValid>() {}
            #(#static_verifications)*

            #(#behavior_checks)*
        }

        #vis2 use #mod_name::#name;

        const _: () = {
            use #mod_name::#initer_name;

            #(#behaviors_uses)*

            impl #name {
                #(#behaviors_section_items)*
            }
        };

    };
    out.into()
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
