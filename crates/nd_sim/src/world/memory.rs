//! [memory] Heap probe: a hook for `examples/memory_probe` (nd_procgen) to
//! weigh each of the world's fields by cloning it under a counting
//! allocator. Not used by the game.

use super::World;

impl World {
    /// Call `f(name, clone)` for every field of the world, where `clone`
    /// makes and drops a deep copy of that field: the bytes it allocates
    /// are the field's heap size (lengths, not spare capacity). A new field
    /// belongs in the list too (the destructuring below fails to compile
    /// until it is).
    #[allow(clippy::clone_on_copy)]
    pub fn heap_probe(&self, f: &mut dyn FnMut(&'static str, &dyn Fn())) {
        macro_rules! weigh {
            ($($field:ident)*) => {{
                let World { $($field,)* } = self;
                $(f(stringify!($field), &|| {
                    let _ = std::hint::black_box($field.clone());
                });)*
            }};
        }
        weigh!(
            version config clock rng master project parked_projects next_project plan_warnings
            implement_refused parcels planned parcel_lots next_gesture plan_version plan_cache roads zones
            lots buildings citizens households traffic walk weather transit rail facilities economy
            services land_value land_value_work events next_seed road_component deferred_intents stations
            yards halts seller_index seller_index_hour available groceries_for_sale meals_for_sale
            landscape ports fishing road_z road_inner relief ground bridge_metres bots driving journeys
            peds waiting_cache crowds_cache seats society vehicles enforcement air contamination cleanups
            industry_bias places concerns districts tourism subway tolls taxis cargo_hubs rides streets
            pillars pillar_gaps crosswalks estate skyline pressure scene identity street_events road_wear
            dirty road_log social social_concern committee path_links corps
        );
    }
}
