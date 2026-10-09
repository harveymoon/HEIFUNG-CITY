//! Neon District: Bevy front end. The simulation lives in `nd_sim`; this
//! crate renders it, drives it, and gives the player tools.

mod assets;
/// [build-highlight] A Build window section's buildings and service areas on the map.
mod build_highlight;
mod camera;
/// [fix/review-app] The one reset of app state when a different city starts.
mod city_reset;
/// [committee] The mayor's AI committee in the running game.
mod committee;
/// [sweep-sim] Crash logs beside the saves: panics and failed allocations.
mod crash;
/// [directory] Every business and person as sortable rows (City Data's Directory).
mod directory;
mod district_tool;
mod fire_scene;
mod flags;
mod map_layers;
/// [camera] Favourites' gold, a person's place posts, a robot's route.
mod marks;
/// [memory] The process's memory counters (the remote `memory` command).
mod memory;
/// [metro-ux] The Metro map layer and drawing lines station to station.
mod metro_ux;
/// [toll-inspect] Picking and outlining toll booths.
mod toll_select;
/// [road-inspect] Picking and outlining street pieces, and the road card's figures.
mod road_select;
mod remote;
mod render;
mod sim;
/// [dock-access] "Connect here" marks on unjoined dock and station street stubs.
mod stub_marks;
mod tools;
mod train_select;
/// [ferry-routes] Ferry routes on the water when a terminal or ship is selected.
mod ferry_select;
mod ui;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use bevy::window::WindowResolution;

/// The starting window size: `ND_WINDOW=WxH`, else 1600 x 900.
fn window_size() -> WindowResolution {
    let parsed = std::env::var("ND_WINDOW").ok().and_then(|v| {
        let (w, h) = v.trim().split_once(['x', 'X'])?;
        Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
    });
    let (w, h) = parsed.filter(|&(w, h)| w >= 320 && h >= 240).unwrap_or((1600, 900));
    WindowResolution::new(w, h)
}

fn main() {
    crash::install();
    App::new()
        .insert_resource(ClearColor(Color::linear_rgb(0.02, 0.018, 0.028)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "District 2040".into(),
                // `ND_WINDOW=1268x900` for another starting size (UI layout checks).
                resolution: window_size(),
                // `ND_NO_VSYNC=1` for measuring frame rates.
                present_mode: if std::env::var("ND_NO_VSYNC").is_ok_and(|v| v == "1") { bevy::window::PresentMode::AutoNoVsync } else { bevy::window::PresentMode::AutoVsync },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .add_systems(Update, window_icon)
        .add_plugins(bevy_egui::EguiPlugin::default())
        .add_plugins((CorePlugins, WorldViewPlugins, PlayerPlugins, remote::RemotePlugin))
        .run();
}

/// [refactor/app] The game's plugins by domain (review 3.5): a new module
/// registers in its group, not in `main`.
///
/// The simulation and what keeps it: assets, the sim, flags, crash logs and
/// the reset when a different city starts.
struct CorePlugins;

impl PluginGroup for CorePlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>().add(assets::GameAssetsPlugin).add(sim::SimPlugin).add(flags::FlagsPlugin).add(crash::CrashPlugin).add(city_reset::CityResetPlugin)
    }
}

/// What is drawn: the camera, the renderer, map layers, marks and the
/// overlays over the city.
struct WorldViewPlugins;

impl PluginGroup for WorldViewPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(camera::CameraPlugin)
            .add(render::RenderPlugin)
            .add(map_layers::MapLayersPlugin)
            .add(marks::MarksPlugin)
            .add(metro_ux::MetroUxPlugin)
            .add(build_highlight::BuildHighlightPlugin)
            .add(stub_marks::StubMarksPlugin)
            .add(ferry_select::FerrySelectPlugin)
    }
}

/// What the player works with: tools, the UI, the committee.
struct PlayerPlugins;

impl PluginGroup for PlayerPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>().add(tools::ToolsPlugin).add(ui::UiPlugin).add(district_tool::DistrictToolPlugin).add(committee::CommitteePlugin)
    }
}

/// The District 2040 icon on the window and its taskbar button, set once the
/// winit window exists (winit windows live on the main thread, hence the
/// marker).
fn window_icon(mut done: Local<bool>, primary: Query<Entity, With<bevy::window::PrimaryWindow>>, _main: bevy::ecs::system::NonSendMarker) {
    if *done {
        return;
    }
    let Ok(entity) = primary.single() else { return };
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let Some(window) = windows.get_window(entity) else { return };
        const RGBA: &[u8] = include_bytes!("../../../assets/brand/icon_256.rgba");
        if let Ok(icon) = winit::window::Icon::from_rgba(RGBA.to_vec(), 256, 256) {
            window.set_window_icon(Some(icon));
        }
        *done = true;
    });
}
