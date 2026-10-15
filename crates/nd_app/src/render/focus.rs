//! [layers] Focus views: the Traffic load and Crime layers dim the city and
//! light up what the layer is about. With Traffic load on every vehicle on
//! the road turns one flat light blue over a darkened city (and the ones in
//! tunnels are drawn through the ground, `layers::xray`); with Crime on the
//! police cars light up instead, over a city dimmed a little less so the
//! heat map's streets still read.
//!
//! The mechanism is heat vision's, and shared with it (`render/swap.rs`):
//! every mesh's material is swapped for a stand-in derived once per
//! original and cached, the original kept on the entity and put back when
//! the view ends. The layer's own meshes (the shared overlay material, anything
//! tagged [`FocusKeep`]) keep theirs. Heat vision wins: while it is on the
//! focus view stands down, so the two never stack their stand-ins.

use bevy::prelude::*;

use super::{Lighting, Overlays};

/// Which focus view is showing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Focus {
    Traffic,
    Crime,
    /// [metro-ux] The Metro layer: everything dimmed under the lines.
    Metro,
    /// [pt-ux] The Pedestrian network layer: the city dimmed, every person
    /// glowing, the walkways lit.
    Walk,
    /// [pt-ux] The Trails layer: the city nearly black under the streaks.
    Trails,
    /// [pt-ux] The Land density layer: the city dimmed under the plot outlines.
    Density,
    /// [w1] The Construction layer: the city dimmed under the sites.
    Sites,
}

impl Focus {
    /// The view the layers ask for. Crime first: with both on, the police
    /// are what the player turned on last-most likely to look for.
    pub(super) fn wanted(o: &Overlays, l: &Lighting) -> Option<Focus> {
        if l.heat || o.map {
            None
        } else if o.crime {
            Some(Focus::Crime)
        } else if o.traffic {
            Some(Focus::Traffic)
        } else if o.metro {
            Some(Focus::Metro)
        } else if o.trails {
            Some(Focus::Trails)
        } else if o.density || o.park_access {
            Some(Focus::Density)
        } else if o.construction {
            Some(Focus::Sites)
        } else if o.paths || o.walk_networks {
            Some(Focus::Walk)
        } else {
            None
        }
    }

    /// Brightness of the dimmed city: lit surfaces, glow, window lights.
    pub(super) fn dim(self) -> (f32, f32, f32) {
        match self {
            Focus::Traffic => (0.05, 0.035, 0.05),
            Focus::Crime => (0.075, 0.05, 0.08),
            Focus::Metro => (0.07, 0.045, 0.07),
            Focus::Walk => (0.09, 0.06, 0.08),
            Focus::Trails => (0.03, 0.02, 0.03),
            Focus::Density => (0.16, 0.1, 0.12),
            Focus::Sites => (0.16, 0.1, 0.12),
        }
    }
}

/// The light blue every vehicle turns in the Traffic load view (linear,
/// over 1 so it blooms a little against the dark).
pub const TRAFFIC_BLUE: [f32; 3] = [0.55, 1.3, 2.9];
/// Police cars in the Crime view: a hot blue-white.
pub const POLICE_GLOW: [f32; 3] = [0.6, 0.95, 2.6];
/// [pt-ux] People in the Pedestrian network view: a bright cyan.
pub const WALKER_GLOW: [f32; 3] = [0.5, 2.4, 3.2];

/// A police unit's body and light bar (set by `emergency`).
#[derive(Component)]
pub(super) struct PoliceTag;
/// An emergency unit's body or light bar (not its fires or rubble).
#[derive(Component)]
pub(super) struct UnitTag;
/// Keeps its own material in a focus view (a layer's meshes).
#[derive(Component)]
pub(super) struct FocusKeep;

/// How a focus stand-in is derived from its original.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Class {
    Dim(Focus),
    Glow([u32; 3]),
}

pub(super) fn stand_in(o: &StandardMaterial, class: Class) -> StandardMaterial {
    let alpha = o.base_color.alpha();
    let base = StandardMaterial { alpha_mode: o.alpha_mode, double_sided: o.double_sided, cull_mode: o.cull_mode, fog_enabled: o.fog_enabled, ..default() };
    match class {
        Class::Dim(f) => {
            let (lit, glow, _) = f.dim();
            let opaque = matches!(o.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_));
            let k = if o.unlit { glow } else { lit } * if opaque { 1.0 } else { 0.6 };
            StandardMaterial { base_color: Color::linear_rgba(k, k, k, alpha), base_color_texture: o.base_color_texture.clone(), uv_transform: o.uv_transform, unlit: true, ..base }
        }
        Class::Glow(c) => {
            let c = [c[0] as f32 / 1000.0, c[1] as f32 / 1000.0, c[2] as f32 / 1000.0];
            // Emitted, not lit: the paint lives in the vertex colours, which
            // multiply the base colour but not the emission, so a red car and
            // a yellow cab come out the same blue. Emissive alpha 0: not
            // scaled by the camera's exposure (as heat vision does).
            StandardMaterial { base_color: Color::linear_rgba(0.0, 0.0, 0.0, alpha), emissive: LinearRgba::new(c[0], c[1], c[2], 0.0), perceptual_roughness: 1.0, metallic: 0.0, reflectance: 0.0, ..base }
        }
    }
}

fn glow(c: [f32; 3]) -> Class {
    Class::Glow([(c[0] * 1000.0) as u32, (c[1] * 1000.0) as u32, (c[2] * 1000.0) as u32])
}

/// How a mesh is drawn in a focus view: `None` keeps its own material (the
/// layer's own meshes, fires and smoke, a police light bar in the Crime
/// view).
pub(super) fn classify(focus: Focus, tags: (bool, bool, bool, bool, bool), citizen: bool, overlay: bool, o: &StandardMaterial) -> Option<Class> {
    let (car, unit, police, keep, emergency) = tags;
    if overlay || keep {
        return None;
    }
    let class = match focus {
        // [pt-ux] Everyone on foot lights up.
        Focus::Walk if citizen => glow(WALKER_GLOW),
        Focus::Walk if emergency => return None,
        Focus::Traffic if car || unit => glow(TRAFFIC_BLUE),
        // Fires and smoke stay as they are: they are part of the story.
        Focus::Traffic if emergency => return None,
        Focus::Crime if police => glow(POLICE_GLOW),
        Focus::Crime if emergency => return None,
        _ => Class::Dim(focus),
    };
    // A police light bar keeps its red and blue; only the body glows.
    if police && o.unlit && focus == Focus::Crime {
        return None;
    }
    Some(class)
}

/// The fog would lift the dimmed city back up with distance: darken it
/// while a view is on; the daylight pass puts it back after.
pub(super) fn focus_fog(lighting: Res<Lighting>, overlays: Res<Overlays>, mut was_on: Local<bool>, mut fog: Query<&mut bevy::pbr::DistanceFog>, mut render: ResMut<super::RenderState>) {
    let on = Focus::wanted(&overlays, &lighting).is_some();
    if on {
        for mut f in &mut fog {
            f.color = Color::linear_rgb(0.008, 0.008, 0.014);
        }
    } else if *was_on {
        render.daylight_key = None;
        render.glow_key = None;
    }
    *was_on = on;
}
