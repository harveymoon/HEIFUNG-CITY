//! [photo] Photo mode: every egui pane, bar, label and pin vanishes so the
//! city fills the window, for screenshots (`F10`, or the remote API's
//! `photo_mode {on}`). The UI still runs each pass, so windows keep their
//! places and state; only its drawing is thrown away, between egui's end of
//! pass and the tessellation that would paint it. The map's own marks
//! (selection rings, overlays) are not egui and stay as they are.

use bevy::prelude::*;
use bevy_egui::{EguiFullOutput, EguiPostUpdateSet};

use super::UiFlags;

pub(super) struct PhotoPlugin;

impl Plugin for PhotoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, toggle_key).add_systems(
            PostUpdate,
            drop_ui_shapes.after(EguiPostUpdateSet::EndPass).before(EguiPostUpdateSet::ProcessOutput),
        );
    }
}

/// `F10` flips photo mode. F10 is bound to nothing else (F5..F9, F11 and
/// F12 are camera jumps); hotkeys stand down while a text field has focus.
fn toggle_key(keys: Res<ButtonInput<KeyCode>>, mut flags: ResMut<UiFlags>) {
    if keys.just_pressed(KeyCode::F10) && !flags.keyboard_captured {
        flags.photo_mode = !flags.photo_mode;
    }
}

/// Empties the pass's shapes while photo mode is on: nothing egui laid out
/// this frame is painted. Textures still update, so fonts and thumbnails
/// are ready the moment the UI comes back.
fn drop_ui_shapes(flags: Res<UiFlags>, mut outputs: Query<&mut EguiFullOutput>) {
    if !flags.photo_mode {
        return;
    }
    for mut out in &mut outputs {
        if let Some(full) = out.0.as_mut() {
            full.shapes.clear();
        }
    }
}
