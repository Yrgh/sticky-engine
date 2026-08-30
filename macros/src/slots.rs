use super::*;

pub fn slot_def_inner(attr: TokenStream1, input: TokenStream1) -> TokenStream1 {
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
            use #engine_crate::core::util::gen_slot_vec::SlotIndex;

            #[doc = #id_doc]
            #visibility1 struct #slot_id_name {
                slot: SlotIndex,
                lidx: #engine_crate::core::level::LevelIndex,
                tyid: ::std::any::TypeId,
                conv: &'static #engine_crate::core::relations::Convert<dyn super::#slot_name>,
            }

            impl<C: ToDyn<dyn super::#slot_name> + IComponent>
                From<ComponentId<C>> for #slot_id_name
            {
                fn from(value: ComponentId<C>) -> Self {
                    let lidx = value.level_id();
                    let (slot, tyid) = value.acquire_parts();

                    Self {
                        slot,
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
                        slot: self.slot,
                        lidx: self.lidx,
                        tyid: self.tyid,
                        conv: self.conv,
                    }
                }
            }

            unsafe impl ISlotId for #slot_id_name {
                type TraitObject = dyn super::#slot_name;

                unsafe fn from_parts(
                    slot: SlotIndex,
                    lidx: #engine_crate::core::level::LevelIndex,
                    tyid: ::std::any::TypeId,
                ) -> Self where Self: Sized {
                    Self {
                        slot,
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

                fn acquire_parts(&self) -> (SlotIndex, ::std::any::TypeId) {
                    (self.slot, self.tyid)
                }

                fn get<'w>(&self, world: &'w #engine_crate::core::world::World)
                    -> Result<::std::cell::Ref<'w, Self::TraitObject>, #engine_crate::core::ComponentGetError>
                {
                    Ok((self.conv.comp_to_t_ref)(
                        world
                            .get_level(self.lidx)
                            .ok_or(#engine_crate::core::ComponentGetError::NotFound)?
                            .acquire_component_internal_dyn(self.tyid, self.slot)?
                    ))
                }

                fn get_mut<'w>(&self, world: &'w #engine_crate::core::world::World)
                    -> Result<::std::cell::RefMut<'w, Self::TraitObject>, #engine_crate::core::ComponentGetMutError>
                {
                    Ok((self.conv.comp_to_t_mut)(
                        world
                            .get_level(self.lidx)
                            .ok_or(#engine_crate::core::ComponentGetMutError::NotFound)?
                            .acquire_component_internal_dyn_mut(self.tyid, self.slot)?
                    ))
                }
            }

            impl ISlotTr for dyn super::#slot_name {
                type Id = #slot_id_name;
            }

            impl From<#slot_id_name> for DynComponentId {
                fn from(value: #slot_id_name) -> Self {
                    unsafe {
                        DynComponentId::from_parts(value.slot, value.lidx, value.tyid)
                    }
                }
            }

            impl ::std::cmp::PartialEq for #slot_id_name {
                fn eq(&self, other: &Self) -> bool {
                    self.slot == other.slot && self.lidx == other.lidx
                }
            }

            impl ::std::cmp::Eq for #slot_id_name {}

            impl ::std::hash::Hash for #slot_id_name {
                fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) {
                    self.slot.hash(state);
                    self.lidx.hash(state);
                }
            }
        }

        #visibility2 use #module_name::#slot_id_name;
    }
    .into()
}

pub fn slot_impl_inner(attr: TokenStream1, input: TokenStream1) -> TokenStream1 {
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