//! The bundle of resources every command runs against.

use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::events::EventLog;
use super::history::History;
use super::window::WindowRequests;
use crate::assets::GameAssets;
use crate::camera::{CameraMode, OrbitCamera, WalkState};
use crate::render::{Lighting, Overlays, RenderStats};
use crate::sim::Sim;
use crate::tools::{EffectivePlan, Selection, ToolState};
use crate::ui::{Saves, UiFlags};

/// Everything a command may read or write, in one bundle so the dispatcher
/// has a single argument.
#[derive(SystemParam)]
pub(super) struct Ctx<'w, 's> {
    pub(super) sim: ResMut<'w, Sim>,
    pub(super) tools: ResMut<'w, ToolState>,
    pub(super) selection: ResMut<'w, Selection>,
    pub(super) eplan: ResMut<'w, EffectivePlan>,
    pub(super) flags: ResMut<'w, UiFlags>,
    pub(super) saves: ResMut<'w, Saves>,
    pub(super) overlays: ResMut<'w, Overlays>,
    pub(super) lighting: ResMut<'w, Lighting>,
    pub(super) mode: ResMut<'w, CameraMode>,
    pub(super) walk: ResMut<'w, WalkState>,
    /// [camera] The camera's follow (shared with the inspector's button)
    /// and its smooth moves.
    pub(super) follow: ResMut<'w, crate::camera::CameraFollow>,
    pub(super) fly: ResMut<'w, crate::camera::CameraFly>,
    pub(super) log: ResMut<'w, EventLog>,
    pub(super) history: ResMut<'w, History>,
    pub(super) windows: ResMut<'w, WindowRequests>,
    pub(super) cams: Query<'w, 's, &'static mut OrbitCamera>,
    pub(super) stats: Res<'w, RenderStats>,
    pub(super) assets: Res<'w, GameAssets>,
    pub(super) diagnostics: Res<'w, DiagnosticsStore>,
    pub(super) commands: Commands<'w, 's>,
    pub(super) claude: ResMut<'w, crate::flags::ClaudeFlags>,
    /// [committee] The committee's backend and meeting driver.
    pub(super) committee: ResMut<'w, crate::committee::CommitteeRt>,
    /// [build-highlight] What the Build window's section shows on the map.
    pub(super) highlight: Res<'w, crate::build_highlight::BuildHighlight>,
    /// [memory] For the `memory` command: what the renderer holds.
    pub(super) meshes: Res<'w, Assets<Mesh>>,
    pub(super) materials: Res<'w, Assets<StandardMaterial>>,
    pub(super) images: Res<'w, Assets<Image>>,
    pub(super) entities: Query<'w, 's, Entity>,
    /// [fix/review-app] Which city the tools and selection belong to.
    pub(super) epoch: ResMut<'w, crate::city_reset::CityEpoch>,
    /// [viewer] The building viewer, its turntable and its notes.
    pub(super) viewer: ResMut<'w, crate::viewer::Viewer>,
    pub(super) turntable: ResMut<'w, crate::render::Turntable>,
    pub(super) notes: ResMut<'w, crate::viewer::notes::BuildingNotes>,
    /// [photo] The game window, for `photo_mode {size}`.
    pub(super) window: Query<'w, 's, &'static mut Window, With<bevy::window::PrimaryWindow>>,
    /// [sound] The sound engine's handle and what it last heard.
    pub(super) sound: Res<'w, crate::audio::Sound>,
    /// [r4-tiles] The tile picked in the Land for sale window.
    pub(super) picked_tile: ResMut<'w, crate::render::land_tiles::PickedTile>,
    /// [laptop-nav] Navigation settings, this frame's input, the pad and
    /// the synthetic input queue.
    pub(super) nav_settings: ResMut<'w, crate::nav::NavSettings>,
    pub(super) nav_input: Res<'w, crate::nav::NavInput>,
    pub(super) nav_pad: ResMut<'w, crate::nav::NavPad>,
    pub(super) nav_remote: ResMut<'w, crate::nav::NavRemote>,
}
