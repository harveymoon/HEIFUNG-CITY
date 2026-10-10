//! Turns simulation state into Bevy entities. Static geometry (terrain,
//! streets, lot ground, project preview) is rebuilt as whole meshes when it
//! changes; buildings and vacant-lot dressing are batched per [`CHUNK`]-metre
//! cell, one mesh per surface per cell, and a cell is rebuilt when anything in
//! it changes; citizens and cars are small shared meshes moved every frame.
//!
//! Every [`Surface`] has a material with a procedurally generated, seamlessly
//! tiling colour texture and normal map. Meshes carry world-scale UVs, so the
//! same brick texture lines up across a whole facade.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{CascadeShadowConfigBuilder, GlobalAmbientLight, NotShadowCaster, NotShadowReceiver};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use nd_procgen::street::{generate_street, signal_posts, StreetInput};
use nd_procgen::{build_terrain, bus_stops, generate_building, generate_texture, generate_vacant, mip_chain, scatter_nature, Extras, Geometry, MeshData, StyleBook, Surface, TerrainConfig};
use nd_sim::roads::{LANE_WIDTH, SHOULDER};
use nd_sim::traffic::LaneId;
use nd_sim::{BuildingId, CarId, CitizenId, LotId, SimEvent, Vec2 as SVec2, World};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::assets::GameAssets;
use crate::sim::{Sim, SimSet};
use crate::tools::Selection;

/// Offscreen renders: Build window thumbnails, save pictures.
mod thumbs;
pub use thumbs::Thumbs;
mod air;
/// [districts] The district layer; [tourism] visitors on foot.
mod districts;
/// Emergency vehicles, fires and rubble.
mod emergency;
// [fire-scene] Firefighters, hoses and water at a fire.
mod fire_scene;
/// [land] Police light bars on convoys, bodyguards on foot.
mod convoys;
mod far;
// [fishing] trawlers at their berths and out on the grounds
mod fishing;
/// [layers] Traffic and Crime focus views: the city dimmed, vehicles lit.
mod focus;
mod haze;
// [dirty-industry] smoke over the dirty plants and the poisoned ground's stain
mod grime;
/// [climate] Rain, snow and hot-spot steam.
pub mod climate;
/// Drone heat vision (render/heat.rs).
mod heat;
/// Planned roads as holograms of the real thing; the depth view.
mod holo;
pub use holo::gap_spot;
/// [plot-hover] The plot under the pointer while placing: outline, light wall, verdict.
mod plot_hover;
pub use plot_hover::{hover_site, site_verdict};
mod junction_view;
mod layers;
/// [pt-ux] Parking, Trails and Land density layers.
mod pt_layers;
pub use pt_layers::{availability_color as parking_color, Mover as TrailMover};
mod lights;
// [review] retaining walls at corner lots over a lower side street
mod retaining;
/// [look] Graphics quality, camera effects (SSAO, TAA, volumetric fog,
/// reflections of the sky) and the frame-rate throttler.
mod look;
pub use look::{Graphics, QualitySetting};
/// The one material swapper heat vision and the focus views share.
mod swap;
/// [layers] Map view: a flat street map with names and pins.
mod mapview;
pub use focus::{POLICE_GLOW, TRAFFIC_BLUE};
pub use layers::danger_color;
pub use nights::road_wear_color;
pub use mapview::{zone_tint as map_zone_tint, PIN_KEY as MAP_PIN_KEY, ROAD_KEY as MAP_ROAD_KEY};
/// The sky dome: light pollution at night, glare round the sun by day.
mod sky;
mod street_look;
/// [looks] Citizens drawn with their own look.
mod people;
pub use people::MESH_CAP as PEOPLE_MESH_CAP;
mod stunts;
/// [events] Rave lights, race markers, road cracks and the road condition layer.
mod nights;
mod survey;
/// [subway] Metro tunnels and trains in the Underground view.
mod subway;
/// [society] Camps, RVs and overflowing bins.
mod society;
/// [social] Murals and community gardens.
mod social;
/// [dirty-industry] Clean-up crews digging out contaminated ground.
mod cleanup;
mod solar;
mod turbines;
/// [rail-crossing] Level crossing lamps and barrier arms.
mod crossings;
/// [taxis] Cabs in their yards, at ranks and kerbs; yard signs.
mod taxis;
/// [waterfront] Private jetties off waterfront lots.
mod shore;

// [refactor/app] render.rs split into these modules (review 3.1).
mod mesh_util;
mod materials;
mod daylight;
mod ground;
mod streets;
mod preview;
mod lots;
mod buildings;
mod reset;
mod citizens;
mod vehicles;
mod parked;
mod overlays;
mod trains;
mod landscape;
mod lift;
pub(crate) use self::mesh_util::*;
use self::materials::*;
pub(crate) use self::daylight::*;
use self::ground::*;
use self::streets::*;
use self::preview::*;
use self::lots::*;
pub(crate) use self::buildings::*;
use self::reset::*;
use self::citizens::*;
pub(crate) use self::vehicles::*;
use self::parked::*;
pub(crate) use self::overlays::*;
use self::trains::*;
use self::landscape::*;
pub(crate) use self::lift::*;

/// Side of the square cells that buildings and vacant-lot dressing are
/// batched into. Wide enough that a few hundred buildings become a few dozen
/// meshes per surface, small enough that one changed building regenerates a
/// street block rather than the whole city.
const CHUNK: f32 = 128.0;

/// Cell index of a world position on the [`CHUNK`] grid.
type ChunkKey = (i32, i32);

fn chunk_of(p: SVec2) -> ChunkKey {
    ((p.x / CHUNK).floor() as i32, (p.y / CHUNK).floor() as i32)
}

#[derive(Component)]
struct SunTag;
#[derive(Component)]
struct TerrainTag;
#[derive(Component, Clone)]
struct ScatterTag;
#[derive(Component)]
struct RoadsTag;
/// Lot ground fills and outline glows: one mesh per surface for the whole city.
#[derive(Component, Clone)]
struct LotsTag;
/// Park layouts, one set of entities per park lot.
#[derive(Component, Clone)]
struct ParkTag;
/// Vacant-lot dressing, batched per chunk.
#[derive(Component, Clone)]
struct VacantTag;
#[derive(Component)]
struct PreviewTag;
/// The Road tool's roundabout under the pointer: what stamping it would do.
#[derive(Component)]
struct RingPreviewTag;
#[derive(Component)]
struct SignalTag(LaneId);
/// [pedestrians] A walk lamp: intersection and approach (it also carries a
/// `SignalTag` with no lane, so it is lifted and despawned with the rest).
#[derive(Component)]
struct PedSignalTag(usize, usize);
/// Lane arrows at junction approaches (follow `LaneNetwork::arrows_version`).
#[derive(Component, Clone)]
struct LaneArrowsTag;
#[derive(Component)]
struct OverlayTag;
#[derive(Component)]
struct PathsTag;
#[derive(Component, Clone)]
struct TransitTag;
/// [tolls] Toll booth meshes, set on the road's height by `lift_spawned`.
#[derive(Component, Clone)]
struct TollTag;
/// Marker only: which train an entity belongs to is tracked in
/// [`RenderState::trains`], so the tag carries no id.
#[derive(Component)]
struct TrainTag;

/// One chunk's worth of building geometry. Which buildings a mesh holds is
/// tracked in [`RenderState::building_chunks`]; selection and inspection go
/// through the sim's lots, never through mesh entities, so no id is needed.
#[derive(Component, Clone)]
pub struct BuildingTag;
/// Movers are looked up through the maps in [`RenderState`], so these tags
/// are plain markers for the per-frame transform queries.
#[derive(Component)]
pub struct CitizenTag;
#[derive(Component)]
pub struct CarTag;
/// [render] A car's body, lights or glow, and whether the car is in a
/// tunnel (`World::car_in_tunnel`): the depth view shows it with the
/// Underground layer and hides it above ground (`holo::apply_depth`).
#[derive(Component)]
pub struct InTunnel(pub bool);

/// A vehicle drawn lower than this (metres) is in a tunnel: the
/// underground view shows it, and only it can be clicked there.
pub const UNDERGROUND_BELOW: f32 = -2.5;

/// A mover that sets its own height (cars, from the road they are on);
/// `lift_movers` leaves it alone.
#[derive(Component)]
pub struct SelfLifted;

/// How far a selected mover (a person, a car, an emergency unit) is lifted
/// off the road to stand out.
pub(crate) const SELECTED_LIFT: f32 = 0.4;

/// [fix/review-app] The movers the selection lifts: a person (and the car
/// they drive), a car or bus, an emergency unit. One exhaustive match, so a
/// new `Selection` kind has to say here whether it lifts anything (a
/// selected ambulance used to be lifted by nobody: its generic car is
/// hidden and the unit body ignored the selection).
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(crate) struct Lifts {
    pub citizen: Option<nd_sim::CitizenId>,
    pub car: Option<CarId>,
    pub unit: Option<nd_sim::UnitId>,
}

pub(crate) fn lifts(sel: Selection) -> Lifts {
    match sel {
        Selection::Citizen(c) => Lifts { citizen: Some(c), ..Default::default() },
        Selection::Car(c) => Lifts { car: Some(c), ..Default::default() },
        Selection::Unit(u) => Lifts { unit: Some(u), ..Default::default() },
        // Outlined by their own gizmos or meshes, not lifted.
        Selection::None | Selection::Building(_) | Selection::Parked(_) | Selection::Toll(_) | Selection::Bot(_) | Selection::Train(_) | Selection::Metro(_) | Selection::Road(_) | Selection::Ship(_) => Lifts::default(),
    }
}

/// Where a mover is spawned before its first pose: far below the map.
/// Spawns go through `Commands`, so the frame's pose systems cannot see the
/// new entity yet; spawned at `Transform::IDENTITY` it would flash for a
/// frame at the world origin (the centre of the main junction).
pub(crate) fn offstage() -> Transform {
    Transform::from_xyz(0.0, -10_000.0, 0.0)
}

/// The meshes of one chunk and what they were built from.
#[derive(Default)]
struct ChunkMeshes {
    entities: Vec<Entity>,
    triangles: usize,
}

#[derive(Resource, Default)]
struct RenderState {
    roads_version: u64,
    lanes_version: u64,
    /// [footbridge] Hash of the crosswalks footbridges replaced, as the
    /// streets (their paint) and the walk lamps were last drawn for.
    closed_roads: u64,
    closed_signals: u64,
    /// What the project holograms were built for: plan version, parcels
    /// version, whether parked projects are shown.
    preview_key: Option<(u64, u64, bool)>,
    arrows_version: u64,
    /// [refactor/app] What the lane arrows were last built from when they
    /// came out empty (a network with no arrows draws nothing, and the
    /// version check alone rebuilt it every frame).
    arrows_empty: Gate<(u64, u64, u64, u64, u64, bool)>,
    /// [refactor/app] Likewise for bus stops: routes with no stop drawn.
    transit_empty: Gate<(u64, u64, u64)>,
    /// What the lot ground meshes were built for: lot and zone counts, roads
    /// version, zoning overlay, and a hash of every lot's built/park/
    /// landlocked state.
    lots_signature: Option<(usize, usize, u64, bool, u64)>,
    /// [refactor/app] The counters `lots_signature` was hashed under.
    lots_gate: Gate<(u64, u64, u64, usize, u64, bool, bool)>,
    /// [refactor/app] The lots version the parks, and the lots and land
    /// value versions the vacant dressing, were last checked against.
    parks_gate: Gate<(u64, u64)>,
    vacant_gate: Gate<(u64, u64, u64, u64)>,
    /// Park lots drawn, with a hash of the layout and polygon they show.
    parks: HashMap<LotId, (u64, Vec<Entity>)>,
    /// Vacant dressing per chunk, with a hash of the lots and land-value
    /// buckets it was generated from.
    vacant_chunks: HashMap<ChunkKey, (u64, Vec<Entity>)>,
    /// Roads version and zone count the open-ground scatter was built for.
    scatter_signature: Option<(u64, usize)>,
    /// Which chunk each drawn building sits in, so a changed or removed
    /// building can mark its chunk dirty.
    building_chunks: HashMap<BuildingId, ChunkKey>,
    /// Solar and wind plants drawn, by lot centre: a plant coming, changing
    /// or going redraws the fields round it (see [`plant_field`]).
    plant_sites: HashMap<BuildingId, SVec2>,
    chunks: HashMap<ChunkKey, ChunkMeshes>,
    citizens: HashMap<CitizenId, Entity>,
    cars: HashMap<CarId, [Entity; 2]>,
    overlay_version: u64,
    overlay_shown: bool,
    paths_version: u64,
    paths_shown: bool,
    transit_version: u64,
    /// [tolls] `Tolls::version` the booths were last drawn for (+1, so 0 is "never").
    toll_version: u64,
    trains: HashMap<nd_sim::TrainId, Vec<Entity>>,
    /// Quantised sun elevation the lighting was last written for.
    daylight_key: Option<i32>,
    /// Quantised glow strength last written to the shared Glow material.
    glow_key: Option<i32>,
}

/// [refactor/app] The sim version counters a hashed signature was last
/// worked out under. While they stay put the signature cannot have moved,
/// so a renderer skips hashing the whole world (lots, parks, the
/// landscape) every frame and hashes only when a counter moves.
#[derive(Default)]
pub(crate) struct Gate<K>(Option<K>);

impl<K: PartialEq> Gate<K> {
    /// True when `key` is the one passed last time; remembers it.
    pub(crate) fn same(&mut self, key: K) -> bool {
        let same = self.0.as_ref() == Some(&key);
        self.0 = Some(key);
        same
    }
}

/// Street-level props drawn only around the walker (`nd_procgen::walk`).
#[derive(Component, Clone)]
struct WalkDetailTag;

/// Renderer statistics shown in the UI.
#[derive(Resource, Default)]
pub struct RenderStats {
    /// Building meshes alive: one per non-empty surface per chunk.
    pub building_entities: usize,
    pub street_triangles: usize,
    pub building_triangles: usize,
    /// [looks] Figure meshes cached, people outdoors, distinct looks among
    /// them, figures built over the run and borrowed this frame.
    pub people_meshes: usize,
    pub people_visible: usize,
    pub people_looks_visible: usize,
    pub people_built: usize,
    pub people_borrowed: usize,
    /// [look] The graphics tier in force, the frame time and the effects.
    pub graphics: Graphics,
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<people::PeopleRender>();
        app.add_plugins(thumbs::ThumbsPlugin);
        app.init_resource::<RenderState>().init_resource::<VehicleRender>().init_resource::<Overlays>().init_resource::<Lighting>().init_resource::<RenderStats>().add_plugins(MaterialPlugin::<lights::WindowMaterial>::default()).init_resource::<sky::SkyLook>().add_systems(Startup, (lights::add_shader, setup)).add_systems(
            Update,
            (sync_terrain, sync_scatter, sync_roads, sync_signals, sync_preview, sync_lots, sync_parks, sync_vacant, sync_overlay, sync_paths, sync_transit, sync_buildings, sync_walk_detail, sync_citizens, sync_cars, sync_bots, sync_trains, update_daylight)
                .chain()
                .in_set(SimSet::Sync),
        );
        app.add_systems(Update, sync_lane_arrows.in_set(SimSet::Sync).after(sync_signals).before(lift_spawned));
        app.add_systems(Update, sync_tolls.in_set(SimSet::Sync).after(sync_roads).before(lift_spawned));
        app.add_systems(Update, sync_ring_preview.in_set(SimSet::Sync).after(sync_preview).before(lift_spawned));
        app.add_systems(Update, (holo::sync_draw_hologram, holo::sync_plan_holograms).in_set(SimSet::Sync).after(sync_preview).before(lift_spawned));
        app.add_systems(Update, holo::apply_depth.in_set(SimSet::Sync).after(lift_movers));
        app.add_systems(Update, holo::sync_tunnel_hints.in_set(SimSet::Sync));
        // [pt-ux] The rest of the surface, after every owner (PostUpdate).
        app.add_systems(PostUpdate, holo::hide_surface_extras.before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate));
        app.add_systems(Update, (subway::sync_metro_tunnels, subway::sync_metro_trains).in_set(SimSet::Sync).after(sync_trains).before(holo::apply_depth));
        app.add_systems(Update, subway::sync_platform_figures.in_set(SimSet::Sync).after(holo::apply_depth));
        app.add_systems(Update, (turbines::sync_turbines, solar::sync_solar, haze::sync_haze, grime::sync_grime, layers::sync_layers, survey::sync_land_layer, lights::update_window_lights).chain().in_set(SimSet::Sync).after(sync_trains).before(update_daylight));
        app.add_systems(Update, (climate::sync_precipitation, climate::sync_steam).in_set(SimSet::Sync).after(sync_landscape));
        app.init_resource::<LandscapeRender>().add_systems(Update, (sync_landscape, sync_ships, lift_spawned, lift_movers).chain().in_set(SimSet::Sync).after(update_daylight));
        // [sweep-visual] The sun's shadow cascades reach as far as the
        // orbit camera looks.
        app.add_systems(Update, sun_cascades_follow_camera);
        app.add_systems(Update, fishing::sync_trawlers.in_set(SimSet::Sync).after(sync_ships));
        // The ground and water out past the map, into the fog.
        app.init_resource::<far::FarRender>().add_systems(Update, far::sync_far.in_set(SimSet::Sync).after(sync_landscape));
        app.add_systems(Update, retaining::sync_retaining.in_set(SimSet::Sync).after(sync_landscape));
        // [carlife] Cars parked at the kerb, after the cars on the road.
        app.init_resource::<ParkedRender>().add_systems(Update, sync_parked_cars.after(sync_cars).before(lift_movers).in_set(SimSet::Sync));
        // [taxis] Cabs standing in yards, at ranks and kerbs.
        app.init_resource::<taxis::TaxiRender>().add_systems(Update, taxis::sync_taxis.after(sync_cars).before(lift_movers).in_set(SimSet::Sync));
        // Emergency vehicles, fires and rubble, after the cars they hide.
        app.add_systems(Update, emergency::sync_emergency.after(sync_cars).before(lift_movers).in_set(SimSet::Sync));
        app.add_systems(Update, emergency::amber_beacons.after(sync_cars).in_set(SimSet::Sync));
        app.add_systems(Update, fire_scene::sync_fire_scene.after(emergency::sync_emergency).in_set(SimSet::Sync));
        // [driving] Tyre smoke, skid marks, wrecks and tows.
        app.add_systems(Update, stunts::sync_stunts.after(sync_cars).before(lift_movers).in_set(SimSet::Sync));
        // [land] billionaires' convoys and bodyguards
        app.init_resource::<convoys::ConvoyRender>().add_systems(Update, convoys::sync_convoys.after(sync_cars).before(lift_movers).in_set(SimSet::Sync));
        // [events]
        app.add_systems(Update, (nights::sync_road_wear, nights::sync_raves, nights::race_gizmos).in_set(SimSet::Sync).after(sync_cars).before(lift_movers));
        // [society] Skid row and the trash.
        app.init_resource::<society::SocietyRender>().add_systems(Update, society::sync_society.after(sync_vacant).before(update_daylight).in_set(SimSet::Sync));
        // [waterfront] jetties, piers and pontoons off the waterfront lots
        app.init_resource::<shore::JettyRender>().add_systems(Update, shore::sync_jetties.after(sync_vacant).before(update_daylight).in_set(SimSet::Sync));
        app.add_systems(Update, shore::sync_coasters.in_set(SimSet::Sync).after(update_daylight));
        // [social] Murals and gardens.
        app.init_resource::<social::SocialRender>().add_systems(Update, social::sync_social.after(sync_vacant).after(sync_buildings).before(update_daylight).in_set(SimSet::Sync));
        // [dirty-industry] Clean-up crews.
        app.init_resource::<cleanup::CleanupRender>().add_systems(Update, cleanup::sync_cleanup.after(sync_vacant).before(update_daylight).in_set(SimSet::Sync));
        // Drone taxis and helicopters, after the buildings whose pads they land on.
        app.init_resource::<air::AirState>().add_systems(Update, air::sync_air.after(sync_buildings).after(sync_cars).before(lift_movers).in_set(SimSet::Sync));
        app.add_systems(Update, check_car_bodies.after(lift_movers).in_set(SimSet::Sync));
        // [rail-crossing] Level crossing gear: flashing lamps, swinging arms.
        app.add_systems(Update, crossings::sync_crossings.after(sync_trains).before(update_daylight).in_set(SimSet::Sync));
        app.add_systems(Update, sky::sync_sky.after(update_daylight).in_set(SimSet::Sync));
        // [look] Street lamps that light the road at night, wet roads in
        // the rain, and the fog's reach by climate and weather.
        app.init_resource::<street_look::StreetLamps>()
            .add_systems(Update, (street_look::sync_lamp_lights, street_look::sync_wet_roads).after(update_daylight).in_set(SimSet::Sync))
            .add_systems(PostUpdate, street_look::sync_fog_reach.before(heat::apply_heat));
        // Tower cranes slewing on building sites.
        app.add_systems(Update, animate_cranes.after(sync_buildings).in_set(SimSet::Sync));
        app.add_plugins(heat::HeatPlugin);
        // [layers] Focus views swap materials before heat vision does (heat wins).
        app.init_resource::<swap::SwapState>().add_systems(PostUpdate, (focus::focus_fog.before(heat::apply_heat), swap::swap_materials));
        app.add_plugins(look::LookPlugin);
        app.insert_gizmo_config(layers::XrayGizmos, bevy::gizmos::config::GizmoConfig { depth_bias: -1.0, line: bevy::gizmos::config::GizmoLineConfig { width: 3.0, ..default() }, ..default() });
        app.add_systems(Update, layers::layer_gizmos.in_set(SimSet::Sync).after(lift_movers));
        // [clarity] Traffic load lines and crash markers over everything.
        app.insert_gizmo_config(layers::LoadGizmos, bevy::gizmos::config::GizmoConfig { depth_bias: -1.0, line: bevy::gizmos::config::GizmoLineConfig { width: 5.0, ..default() }, ..default() });
        app.add_systems(Update, layers::load_gizmos.in_set(SimSet::Sync).after(lift_movers));
        app.add_systems(Update, layers::danger_gizmos.in_set(SimSet::Sync).after(lift_movers));
        app.add_plugins(pt_layers::PtLayersPlugin);
        app.add_plugins(mapview::MapViewPlugin);
        // [districts] The district layer; [tourism] visitors on foot, after
        // the citizens whose figure cache they share.
        app.init_resource::<districts::DistrictRender>().add_systems(Update, (districts::sync_district_layer, districts::sync_tourists.after(sync_citizens).before(lift_movers)).in_set(SimSet::Sync));
        // After every renderer, before the UI (which may load a city and set
        // the flag again for the next frame's renderers).
        app.add_systems(Update, clear_full_rebuild.after(SimSet::Sync));
        // [tools] A label at each spot with no room for a column.
        app.add_systems(bevy_egui::EguiPrimaryContextPass, holo::gap_labels);
        // [plot-hover] The plot under the pointer while placing, and its verdict.
        app.init_resource::<plot_hover::PlotHover>().add_plugins(crate::overlay::mesh::MeshOverlayPlugin::<plot_hover::PlotHoverMesh>::default());
        app.add_systems(bevy_egui::EguiPrimaryContextPass, plot_hover::plot_hover_label);
        // [tools] Footbridges, and the Intersection editor's road view.
        app.init_resource::<junction_view::JunctionCam>()
            .add_systems(Update, (junction_view::sync_footbridges.after(sync_roads).before(lift_spawned), junction_view::sync_junction_camera).in_set(SimSet::Sync))
            .add_systems(PostUpdate, junction_view::tag_road_layers);
    }
}

fn despawn_all<F: bevy::ecs::query::QueryFilter>(commands: &mut Commands, q: &Query<Entity, F>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}
