use std::time::Duration;

use component_engine::prelude::*;

comp_def! {
    (in component_engine)
    pub struct CTest {
        components {

        }
        variables {

        }
        behaviors {
            fn init(
                _world: &World,
                _parent: ComponentParent,
                _self_id: ComponentId<Self>
            ) -> CTestInit {
                spawn(async {
                    tokio::time::sleep(Duration::from_secs_f32(1.0)).await;
                    println!("Tuffness");

                    join_main(|world| {
                        for ex in world.main_level().all_matching_ids::<SSlotExId>() {
                            ex.get(world).expect("in level right now").print();
                        }
                    }).await;

                    queue_quit();
                });

                CTestInit {

                }
            }

            fn pre_phys(&mut self, _world: &World, _delta: f32) {

            }
        }
    }
}

#[slot_def(in component_engine)]
trait SSlotEx {
    fn print(&self);
}

#[slot_impl(in component_engine)]
impl SSlotEx for CTest {
    fn print(&self) {
        println!("Hi");
    }
}

fn main() {
    unsafe {
        run_main_loop(|world| {
            println!("main loop started");
            let main_level = world.main_level();
            main_level.spawn_top_level::<CTest>(world);
        })
    }
    .expect("main loop failed");
}
