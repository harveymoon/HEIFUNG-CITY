//! Neon District: Bevy front end. The simulation lives in `nd_sim`; this
//! crate renders it, drives it, and gives the player tools.

mod assets;
mod camera;
mod render;
mod sim;
mod tools;
mod ui;

use bevy::prelude::*;
use bevy::window::WindowResolution;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::linear_rgb(0.02, 0.018, 0.028)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "District 2040".into(),
                resolution: WindowResolution::new(1600, 900),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(bevy_egui::EguiPlugin::default())
        .add_plugins((
            assets::GameAssetsPlugin,
            sim::SimPlugin,
            camera::CameraPlugin,
            tools::ToolsPlugin,
            render::RenderPlugin,
            ui::UiPlugin,
        ))
        .run();
}
