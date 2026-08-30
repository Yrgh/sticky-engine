use super::*;

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

pub fn comp_def_inner(input: TokenStream1) -> TokenStream1 {
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
                let name_get_at_tr = syn::Ident::new(&format!("{name}_get_at"), Span2::call_site());
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

                    let info_ty = spawn_info_ty.clone().unwrap_or_else(|| parse_quote! { () });

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
                    let hook_ident =
                        syn::Ident::new(&format!("{attr_ident}_hook"), fn_ident.span());

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

                    let takes_delta = matches!(
                        attr_ident.to_string().as_str(),
                        "pre_phys" | "post_phys" | "idle"
                    );

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

                    let (slot, tyid) = ISlotId::acquire_parts(id);
                    let lvl = world
                        .get_level(ISlotId::level_id(id))
                        .expect("level destroyed");

                    lvl.remove_component_internal(tyid, slot);
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
                fn parent(&self) -> #comp_mod::ComponentParent {
                    self.c_parent.clone()
                }

                fn self_id(&self) -> DynComponentId {
                    self.c_self.clone().into()
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
                    let (slot, _) = #comp_mod::ISlotId::acquire_parts(&self_id);

                    lvl.fill_slot_internal(slot, self_);

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
