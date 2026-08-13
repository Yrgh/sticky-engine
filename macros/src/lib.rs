extern crate proc_macro;

use proc_macro::TokenStream as TokenStream1;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span as Span2, TokenStream as TokenStream2};
use quote::{ToTokens, quote, quote_spanned};
use syn::{
    FieldModifiers, Ident, ImplItem, ItemImpl, ItemTrait, Path, Token, Type, VisRestricted,
    Visibility, braced, parenthesized, parse::Parse, parse_macro_input, parse_quote,
    punctuated::Punctuated, spanned::Spanned, token,
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
/// If you get messages saying the `component-engine` crate cannot be found, you
/// can add `(in path)` to the macro arguments, where `path` is the path to the
/// `component-engine` crate.
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

    let engine_crate =
        if let Some((_, path)) = parse_macro_input!(attr as ParentCrateSpecifier).0 {
            path.into_token_stream()
        } else {
            let Ok(parent_crate_name) = crate_name("component-engine") else {
                return error(input_orig, "could not find component-engine crate");
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
        .push(parse_quote! { #engine_crate::engine::component::IComponent });

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
            use #engine_crate::engine::component::*;

            #[doc = #id_doc]
            #visibility1 struct #slot_id_name {
                pidx: u32,
                gidx: u32,
                lidx: #engine_crate::engine::level::LevelIndex,
                tyid: ::std::any::TypeId,
                conv: &'static #engine_crate::engine::relations::Convert<dyn super::#slot_name>,
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
                        conv: #engine_crate::engine::relations::RELATIONS
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
                    lidx: #engine_crate::engine::level::LevelIndex,
                    tyid: ::std::any::TypeId,
                ) -> Self where Self: Sized {
                    Self {
                        pidx,
                        gidx,
                        lidx,
                        tyid,
                        conv: #engine_crate::engine::relations::RELATIONS
                            .get_convert::<#slot_id_name>(tyid)
                            .expect("bad reflection data"),
                    }
                }

                fn level_id(&self) -> #engine_crate::engine::level::LevelIndex {
                    self.lidx
                }

                fn acquire_parts(&self) -> (u32, u32, ::std::any::TypeId) {
                    (self.pidx, self.gidx, self.tyid)
                }

                fn get<'a>(&'a self, world: &'a #engine_crate::engine::world::World)
                    -> Option<::std::cell::Ref<'a, Self::TraitObject>>
                {
                    Some((self.conv.comp_to_t_ref)(
                        world
                            .get_level(self.lidx)?
                            .acquire_component_internal_dyn(self.tyid, self.pidx, self.gidx)?
                    ))
                }

                fn get_mut<'a>(&'a self, world: &'a #engine_crate::engine::world::World)
                    -> Option<::std::cell::RefMut<'a, Self::TraitObject>>
                {
                    Some((self.conv.comp_to_t_mut)(
                        world
                            .get_level(self.lidx)?
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
/// If you get errors saying the `component-engine` crate cannot be found, you
/// can add `(in path)` to the macro arguments, where `path` is the path to the
/// `component-engine` crate.
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

    let engine_crate =
        if let Some((_, path)) = parse_macro_input!(attr as ParentCrateSpecifier).0 {
            path.into_token_stream()
        } else {
            let Ok(parent_crate_name) = crate_name("component-engine") else {
                return error(input_orig, "could not find component-engine crate");
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
            use #engine_crate::engine::relations::*;

            impl #engine_crate::engine::component::ToDyn<dyn #slot_tr> for #comp_ty {
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

        while !behaviors_section.is_empty() {
            behaviors_section_items.push(behaviors_section.parse()?);
        }

        Ok(ComponentDef {
            engine_specifier,
            vis,
            struct_token,
            name,
            components_section_items,
            variables_section_items,
            behaviors_section_items,
        })
    }
}

#[proc_macro]
/// Defines a Component.
///
/// The basic structure is
///
/// ```rust
/// #[comp_def]
/// struct CComponent {
///     // Child components, stored as hidden fields with getters and setters.
///     components {
///         static component1: ComponentId<CExample>, // MUST be ComponentId<X>, cannot change.
///         dyn component2: SExampleId, // Any ID type works, can change.
///         dyn? component3: SExampleId, // Same as component2, but behaves like an Option.
///         dyn* component4: SExampleId, // Same as component2, but behaves like a Vec.
///     }
///     // Fields added to your structure
///     variables {
///         var1: Type1,
///         pub var2: Type2,
///     }
///     // Special impl block
///     behaviors {
///         // Called when CComponent is spawned. CComponentInit holds initial values for all
///         // variables and specifically dyn components. static, dyn?, and dyn* components all
///         // initialize automatically.
///         //
///         // Note: self_id can be passed around but will return None on all accesses until after
///         // init completes
///         fn init(
///             world: &world::World,
///             parent: ComponentParent,
///             self_id: ComponentId<Self>
///         ) -> CComponentInit {unsafe
///             ...
///         }
///     }
/// }
/// ```
///
/// If you get messages saying the `component-engine` crate cannot be found, you
/// can add `(in path)` to the beginning of the macro, where `path` is the path
/// to the `component-engine` crate.
///
/// # Behaviors
///
/// While `init` is mandatory, you can also implement the following, if you
/// wish: (TODO)
///
/// - `destroy(&world::World)` - Called before anything happens when the component is
///   about to be removed.
///
/// - `pre_phys(&world::World, f32)` - Called at a stable interval, depth-first, before
///   the physics engine runs. Children are processed after parents.
///
/// - `post_phys(&world::World, f32)` - Runs *after* `pre_phys` and the physics engine,
///   with children being processed *before* parents.
///
/// - `idle(&world::World, f32)` - Runs before the draw queue is created, even on
///   non-visual objects. Not suitable for game logic.
///
/// # Components
///
/// The macro generates getters and setters on the struct for each component,
/// depending on how it was written.
///
/// `static`:
///
/// - `get_name_id` - Returns the ID of `name`
///
/// `dyn`:
///
/// - `get_name_id` - Returns the ID of `name`
///
/// - `spawn_in_name` - Spawns a new component of the given type and replaces it
///   in `name`.
///
/// `dyn?`:
///
/// - `get_name_id` - Returns the ID of `name`, if it is present.
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
/// - `name_get_at` - Returns the component at the given index in `name`.
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
        behaviors_section_items,
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
        let Ok(parent_crate_name) = crate_name("component-engine") else {
            return error(input_orig, "could not find component-engine crate");
        };

        match parent_crate_name {
            FoundCrate::Itself => quote! { crate },
            FoundCrate::Name(name) => quote! { ::#name },
        }
    };

    let comp_mod = quote! { #engine_crate::engine::component };

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
        let formatted = syn::Ident::new(&format!("c_{name}"), Span2::call_site());

        let stored = match modifier {
            ChildComponentModifier::Static | ChildComponentModifier::DynOne => id_ty.clone(),
            ChildComponentModifier::DynOpt => parse_quote! { Option<#id_ty> },
            ChildComponentModifier::DynPlural => parse_quote! { Vec<#id_ty> },
        };

        fields.push(syn::Field {
            attrs: Vec::new(),
            vis: Visibility::Inherited,
            modifiers: FieldModifiers::default(),
            ident: Some(formatted.clone()),
            colon_token: Some(colon),
            ty: stored.clone(),
            default: None,
        });

        match modifier {
            ChildComponentModifier::Static => {
                let getter_name = syn::Ident::new(&format!("get_{name}_id"), Span2::call_site());

                gen_gs_fns.push(quote! {
                    pub fn #getter_name(&self) -> #stored {
                        self.#formatted.clone()
                    }
                });

                static_verifications.push(quote_spanned! {
                    id_ty.span() =>
                    const _: () = _assert_valid_static::<#id_ty>();
                });

                init_final.push(quote! {
                    #formatted: <
                        <#id_ty as #comp_mod::StaticValid>::Component
                        as #comp_mod::IComponent
                    >::spawn(world, self_id.clone().into())
                });

                iter_children.push(quote! {
                    .chain(::std::iter::once(self.#formatted.clone().into()))
                });
            }
            ChildComponentModifier::DynOne => {
                let get_name_id = syn::Ident::new(&format!("get_{name}_id"), Span2::call_site());
                let spawn_in_name = syn::Ident::new(&format!("spawn_in{name}"), Span2::call_site());

                gen_gs_fns.push(quote! {
                    pub fn #get_name_id(&self) -> #stored {
                        self.#formatted.clone()
                    }

                    pub fn #spawn_in_name<C>(
                        &mut self,
                        world: &#engine_crate::engine::world::World
                    )
                    where
                        C: #comp_mod::ToDyn<<#stored as #comp_mod::ISlotId>::TraitObject>
                            + #comp_mod::IComponent,
                        #comp_mod::ComponentId<C>: Into<#id_ty>
                    {
                        use #comp_mod::{ISlotId, IComponent};

                        let (old_pidx, old_gidx, old_tyid) = ISlotId::acquire_parts(&self.#formatted);
                        let old_level = ISlotId::level_id(&self.#formatted);
                        let mut old = self.#formatted.get_mut(world).expect("removed by other source");
                        old.destroy_hook(world);
                        drop(old);

                        let lvl = world
                            .get_level(old_level)
                            .expect("level destroyed");

                        lvl.remove_component_internal(old_tyid, old_pidx, old_gidx);

                        let cid = <C as IComponent>::spawn(world, self.c_self.clone().into());

                        self.#formatted = cid.into();
                    }
                });

                initer_fields.push(quote! {
                    pub #name: #id_ty,
                });

                init_final.push(quote! {
                    #formatted: init.#name
                });

                iter_children.push(quote! {
                    .chain(::std::iter::once(self.#formatted.clone().into()))
                });
            }
            ChildComponentModifier::DynOpt => {
                let get_name_id = syn::Ident::new(&format!("get_{name}_id"), Span2::call_site());
                let spawn_in_name =
                    syn::Ident::new(&format!("spawn_in_{name}"), Span2::call_site());
                let clear_name = syn::Ident::new(&format!("clear_{name}"), Span2::call_site());
                let has_name = syn::Ident::new(&format!("has_{name}"), Span2::call_site());

                gen_gs_fns.push(quote! {
                    pub fn #get_name_id(&self) -> #stored {
                        self.#formatted.clone()
                    }

                    pub fn #spawn_in_name<C>(
                        &mut self,
                        world: &#engine_crate::engine::world::World
                    )
                    where
                        C: #comp_mod::ToDyn<<#id_ty as #comp_mod::ISlotId>::TraitObject>
                            + #comp_mod::IComponent,
                        #comp_mod::ComponentId<C>: Into<#id_ty>
                    {
                        use #comp_mod::{ISlotId, IComponent};

                        if let Some(old_id) = &self.#formatted {
                            let (old_pidx, old_gidx, old_tyid) = ISlotId::acquire_parts(old_id);
                            let old_level = ISlotId::level_id(old_id);
                            let mut old = old_id.get_mut(world).expect("removed by other source");
                            old.destroy_hook(world);
                            drop(old);

                            let lvl = world
                                .get_level(old_level)
                                .expect("level destroyed");

                            lvl.remove_component_internal(old_tyid, old_pidx, old_gidx);
                        }

                        let cid = <C as IComponent>::spawn(world, self.c_self.clone().into());

                        self.#formatted = Some(cid.into());
                    }

                    pub fn #clear_name(
                        &mut self,
                        world: &#engine_crate::engine::world::World
                    ) -> bool {
                        use #comp_mod::ISlotId;

                        if let Some(old_id) = &self.#formatted {
                            let (old_pidx, old_gidx, old_tyid) = ISlotId::acquire_parts(old_id);
                            let old_level = ISlotId::level_id(old_id);
                            let mut old = old_id.get_mut(world).expect("removed by other source");
                            old.destroy_hook(world);
                            drop(old);

                            let lvl = world
                                .get_level(old_level)
                                .expect("level destroyed");

                            lvl.remove_component_internal(old_tyid, old_pidx, old_gidx);
                            self.#formatted = None;
                            true
                        } else {
                            false
                        }
                    }

                    pub fn #has_name(&self) -> bool {
                        self.#formatted.is_some()
                    }
                });

                init_final.push(quote! {
                    #formatted: None
                });

                iter_children.push(quote! {
                    .chain(self.#formatted.clone().into_iter().map(Into::into))
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
                let name_get_at = syn::Ident::new(&format!("{name}_get_at"), Span2::call_site());
                let name_len = syn::Ident::new(&format!("{name}_len"), Span2::call_site());

                gen_gs_fns.push(quote! {
                    pub fn #name_iter_ids(&self) -> impl Iterator<Item=&#id_ty> {
                        self.#formatted.iter()
                    }

                    pub fn #name_len(&self) -> usize {
                        self.#formatted.len()
                    }

                    pub fn #name_get_at(&self, index: usize) -> Option<&#id_ty> {
                        self.#formatted.get(index)
                    }

                    pub fn #name_find_id(&self, id: &#id_ty) -> Option<usize> {
                        self.#formatted.iter().position(|id2| id2 == id)
                    }

                    pub fn #name_remove_at(
                        &mut self,
                        index: usize,
                        world: &#engine_crate::engine::world::World
                    ) -> bool {
                        use #comp_mod::ISlotId;

                        if index > self.#formatted.len() {
                            return false;
                        }

                        let old_id = self.#formatted.remove(index);
                        let (old_pidx, old_gidx, old_tyid) = ISlotId::acquire_parts(&old_id);
                        let old_level = ISlotId::level_id(&old_id);
                        let mut old = old_id.get_mut(world).expect("removed by other source");
                        old.destroy_hook(world);
                        drop(old);

                        let lvl = world
                            .get_level(old_level)
                            .expect("level destroyed");

                        lvl.remove_component_internal(old_tyid, old_pidx, old_gidx);
                        true
                    }

                    pub fn #name_remove_id(
                        &mut self,
                        id: &#id_ty,
                        world: &#engine_crate::engine::world::World
                    ) -> bool {
                        if let Some(index) = self.#name_find_id(id) {
                            self.#name_remove_at(index, world)
                        } else {
                            false
                        }
                    }

                    pub fn #name_spawn_at<C>(
                        &mut self,
                        index: usize,
                        world: &#engine_crate::engine::world::World
                    )
                    where
                        C: #comp_mod::ToDyn<<#id_ty as #comp_mod::ISlotId>::TraitObject>
                            + #comp_mod::IComponent,
                        #comp_mod::ComponentId<C>: Into<#id_ty>
                    {
                        let new_id: #id_ty = <C as #comp_mod::IComponent>::spawn(
                            world,
                            self.c_self.clone().into()
                        ).into();

                        self.#formatted.insert(index, new_id);
                    }

                    pub fn #name_move_from(&mut self, src: usize, dst: usize) {
                        let old = self.#formatted.remove(src);
                        self.#formatted.insert(dst, old);
                    }
                });

                init_final.push(quote! {
                    #formatted: Default::default()
                });

                iter_children.push(quote! {
                    .chain(self.#formatted.clone().into_iter().map(Into::into))
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

    let mut user_destroy = None;
    let mut user_pre_phys = None;
    let mut user_post_phys = None;
    let mut user_idle = None;

    for item in &behaviors_section_items {
        if let ImplItem::Fn(func) = item {
            match func.sig.ident.to_string().as_str() {
                "destroy" => user_destroy = Some(quote! { self.destroy_hook(world); }),
                "pre_phys" => user_pre_phys = Some(quote! { self.pre_phys(world, delta); }),
                "post_phys" => user_post_phys = Some(quote! { self.post_phys(world, delta); }),
                "idle" => user_idle = Some(quote! { self.idle(world, delta); }),
                _ => {}
            }
        }
    }

    let initer_name = syn::Ident::new(&format!("{name}Init"), Span2::call_site());

    quote! {
        mod #mod_name {
            use super::*;

            pub(super) struct #initer_name {
                #(#initer_fields)*
            }

            #vis #struct_token #name {
                #(#fields),*
            }

            impl #name {
                #(#gen_gs_fns)*
            }

            unsafe impl #comp_mod::IComponent for #name {
                fn parent_id(&self) -> #comp_mod::ComponentParent {
                    self.c_parent.clone()
                }

                fn children(&self) -> Box<dyn Iterator<Item=DynComponentId>> {
                    Box::new(::std::iter::empty()#(#iter_children)*)
                }

                fn spawn(
                    world: &#engine_crate::engine::world::World,
                    parent: #comp_mod::ComponentParent,
                ) -> #comp_mod::ComponentId<Self>
                where
                    Self: Sized
                {
                    let lvl_id = parent.level_id();
                    let lvl = world.get_level(lvl_id).expect("level destroyed");
                    let self_id = lvl.reserve_slot_internal::<Self>();

                    let init = Self::init(world, parent.clone(), self_id.clone());

                    let self_ = Self {
                        c_self: self_id.clone(),
                        c_parent: parent,
                        #(#init_final),*
                    };

                    let lvl = world.get_level(lvl_id).expect("level destroyed");
                    let (pidx, gidx, _) = #comp_mod::ISlotId::acquire_parts(&self_id);

                    lvl.fill_slot_internal(pidx, gidx, self_);

                    self_id
                }

                fn destroy_hook(&mut self, world: &#engine_crate::engine::world::World) {
                    #user_destroy

                    use #comp_mod::ISlotId;

                    for id in self.children() {
                        let (child_pidx, child_gidx, child_tyid) = ISlotId::acquire_parts(&id);
                        let child_level = ISlotId::level_id(&id);
                        let mut child = id.get_mut(world).expect("removed by other source");
                        child.destroy_hook(world);
                        drop(child);

                        let lvl = world
                            .get_level(child_level)
                            .expect("level destroyed");

                        lvl.remove_component_internal(child_tyid, child_pidx, child_gidx);
                    }
                }

                fn pre_phys_hook(
                    &mut self,
                    world: &#engine_crate::engine::world::World,
                    delta: f32
                ) {
                    use #comp_mod::ISlotId;

                    #user_pre_phys

                    for id in self.children() {
                        let mut child = id.get_mut(world).expect("child removed");
                        child.pre_phys_hook(world, delta);
                    }
                }

                fn post_phys_hook(
                    &mut self,
                    world: &#engine_crate::engine::world::World,
                    delta: f32
                ) {
                    use #comp_mod::ISlotId;

                    for id in self.children() {
                        let mut child = id.get_mut(world).expect("child removed");
                        child.post_phys_hook(world, delta);
                    }

                    #user_post_phys
                }

                fn idle_hook(
                    &mut self,
                    world: &#engine_crate::engine::world::World,
                    delta: f32
                ) {
                    use #comp_mod::ISlotId;

                    #user_idle

                    for id in self.children() {
                        let mut child = id.get_mut(world).expect("child removed");
                        child.idle_hook(world, delta);
                    }
                }
            }

            const fn _assert_valid_static<T: #comp_mod::StaticValid>() {}
            #(#static_verifications)*
        }

        #vis2 use #mod_name::#name;
        use #mod_name::#initer_name;

        impl #name {
            #(#behaviors_section_items)*
        }

    }
    .into()
}

fn random_name() -> String {
    Uuid::now_v7().simple().to_string()
}
