use sticky_engine::prelude::*;

comp_def! {
    (in sticky_engine)
    struct CRel {
        components {

        }
        variables {
            trans: Trans3ProviderRelative,
        }
        behaviors {
            #[init]
            fn init(
                _world: &World,
                parent: ComponentParent,
                _self_id: ComponentId<Self>,
                _: u32
            ) -> CRelInit {
                CRelInit {
                    trans: Trans3ProviderRelative::new(&parent)
                }
            }
        }
    }
}

#[slot_impl(in sticky_engine)]
impl STrans3 for CRel {
    fn get_provider(&self) -> &dyn ITrans3Provider {
        &self.trans
    }

    fn get_provider_mut(&mut self) -> &mut dyn ITrans3Provider {
        &mut self.trans
    }
}

comp_def! {
    (in sticky_engine)
    struct CTop {
        components {
            static rel: CRel,
            dyn rel2: CRel,
            dyn? opt_rel: dyn STrans3,
            dyn* many: dyn STrans3,
        }
        variables {
            trans: Trans3ProviderTop,
        }
        behaviors {
            #[init]
            fn init(
                world: &World,
                parent: ComponentParent,
                self_id: ComponentId<Self>,
                _: ()
            ) -> CTopInit {
                let kid = || CRel::spawn(world, self_id.clone().into(), 0);
                CTopInit {
                    trans: Trans3ProviderTop::new(&parent),
                    rel: kid(),
                    rel2: kid(),
                    opt_rel: Some(kid().into()),
                    many: vec![kid().into(), kid().into()],
                }
            }
        }
    }
}

#[slot_impl(in sticky_engine)]
impl STrans3 for CTop {
    fn get_provider(&self) -> &dyn ITrans3Provider {
        &self.trans
    }

    fn get_provider_mut(&mut self) -> &mut dyn ITrans3Provider {
        &mut self.trans
    }
}

fn main() {
    unsafe {
        run_main_loop(
            |world| {
                let main_level = world.main_level().expect("main level");
                main_level.spawn_top_level::<CTop>(world, ());
            },
            false,
        )
    }
    .expect("main loop failed");
}
