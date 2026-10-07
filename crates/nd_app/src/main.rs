//! Neon District: Bevy front end. The simulation lives in `nd_sim`; this
//! crate renders it, drives it, and gives the player tools.

mod assets;
mod camera;
mod district_tool;
mod flags;
mod map_layers;
/// [camera] Favourites' gold, a person's place posts, a robot's route.
mod marks;
/// [metro-ux] The Metro map layer and drawing lines station to station.
mod metro_ux;
/// [toll-inspect] Picking and outlining toll booths.
mod toll_select;
mod remote;
mod render;
mod sim;
mod tools;
mod train_select;
mod ui;

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
        .add_plugins((
            assets::GameAssetsPlugin,
            sim::SimPlugin,
            camera::CameraPlugin,
            tools::ToolsPlugin,
            render::RenderPlugin,
            ui::UiPlugin,
            flags::FlagsPlugin,
            remote::RemotePlugin,
            district_tool::DistrictToolPlugin,
            map_layers::MapLayersPlugin,
            marks::MarksPlugin,
            metro_ux::MetroUxPlugin,
        ))
        .run();
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
