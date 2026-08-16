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
            fn init(
                _world: &World,
                parent: ComponentParent,
                _self_id: ComponentId<Self>
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
        }
        variables {
            trans: Trans3ProviderTop,
        }
        behaviors {
            fn init(
                _world: &World,
                parent: ComponentParent,
                _self_id: ComponentId<Self>
            ) -> CTopInit {
                CTopInit {
                    trans: Trans3ProviderTop::new(&parent)
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
                log!(dbg: "main loop started");
                let main_level = world.main_level().expect("main level");
                main_level.spawn_top_level::<CTop>(world);
            },
            false,
        )
    }
    .expect("main loop failed");
}
