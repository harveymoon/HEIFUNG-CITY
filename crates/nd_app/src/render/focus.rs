//! [layers] Focus views: the Traffic load and Crime layers dim the city and
//! light up what the layer is about. With Traffic load on every vehicle on
//! the road turns one flat light blue over a darkened city (and the ones in
//! tunnels are drawn through the ground, `layers::xray`); with Crime on the
//! police cars light up instead, over a city dimmed a little less so the
//! heat map's streets still read.
//!
//! The mechanism is heat vision's (`render/heat.rs`): every mesh's material
//! is swapped for a stand-in derived once per original and cached, the
//! original kept on the entity ([`FocusOriginal`]) and put back when the
//! view ends. The layer's own meshes (the shared overlay material, anything
//! tagged [`FocusKeep`]) keep theirs. Heat vision wins: while it is on the
//! focus view stands down, so the two never stack their stand-ins.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::{lights::WindowMaterial, CarTag, Lighting, Materials, Overlays};

/// Which focus view is showing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Focus {
    Traffic,
    Crime,
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
        } else {
            None
        }
    }

    /// Brightness of the dimmed city: lit surfaces, glow, window lights.
    fn dim(self) -> (f32, f32, f32) {
        match self {
            Focus::Traffic => (0.05, 0.035, 0.05),
            Focus::Crime => (0.075, 0.05, 0.08),
        }
    }
}

/// The light blue every vehicle turns in the Traffic load view (linear,
/// over 1 so it blooms a little against the dark).
pub const TRAFFIC_BLUE: [f32; 3] = [0.55, 1.3, 2.9];
/// Police cars in the Crime view: a hot blue-white.
pub const POLICE_GLOW: [f32; 3] = [0.6, 0.95, 2.6];

/// A police unit's body and light bar (set by `emergency`).
#[derive(Component)]
pub(super) struct PoliceTag;
/// An emergency unit's body or light bar (not its fires or rubble).
#[derive(Component)]
pub(super) struct UnitTag;
/// Keeps its own material in a focus view (a layer's meshes).
#[derive(Component)]
pub(super) struct FocusKeep;

#[derive(Component)]
pub(super) struct FocusOriginal(Handle<StandardMaterial>);
#[derive(Component)]
pub(super) struct FocusOriginalWindow(Handle<WindowMaterial>);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Class {
    Dim(Focus),
    Glow([u32; 3]),
    Keep,
}

#[derive(Resource, Default)]
pub(super) struct FocusState {
    active: Option<Focus>,
    derived: HashMap<(AssetId<StandardMaterial>, Class), Handle<StandardMaterial>>,
    stand_ins: HashSet<AssetId<StandardMaterial>>,
    windows: HashMap<(AssetId<WindowMaterial>, Focus), Handle<WindowMaterial>>,
    window_stand_ins: HashSet<AssetId<WindowMaterial>>,
}

fn stand_in(o: &StandardMaterial, class: Class) -> StandardMaterial {
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
        Class::Keep => o.clone(),
    }
}

fn glow(c: [f32; 3]) -> Class {
    Class::Glow([(c[0] * 1000.0) as u32, (c[1] * 1000.0) as u32, (c[2] * 1000.0) as u32])
}

/// Swap every mesh's material for its focus stand-in while a focus view is
/// on, and back after. Runs before heat vision's swap (see the module doc).
#[allow(clippy::type_complexity)]
pub(super) fn swap_focus(
    mut commands: Commands,
    lighting: Res<Lighting>,
    overlays: Res<Overlays>,
    mats: Res<Materials>,
    mut state: ResMut<FocusState>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut window_materials: ResMut<Assets<WindowMaterial>>,
    mut q: Query<(Entity, &mut MeshMaterial3d<StandardMaterial>, Option<&FocusOriginal>, (Has<CarTag>, Has<UnitTag>, Has<PoliceTag>, Has<FocusKeep>, Has<super::emergency::EmergencyTag>))>,
    mut windows: Query<(Entity, &mut MeshMaterial3d<WindowMaterial>, Option<&FocusOriginalWindow>)>,
    mut view: (Query<&mut bevy::pbr::DistanceFog>, ResMut<super::RenderState>),
) {
    let want = Focus::wanted(&overlays, &lighting);
    // The fog would lift the dimmed city back up with distance: darken it
    // while a view is on; the daylight pass puts it back after.
    if want.is_some() {
        for mut f in &mut view.0 {
            f.color = Color::linear_rgb(0.008, 0.008, 0.014);
        }
    } else if state.active.is_some() {
        view.1.daylight_key = None;
        view.1.glow_key = None;
    }
    if state.active != want && state.active.is_some() {
        // Put every original back; a new view swaps again from scratch.
        for (e, mut mat, orig, _) in &mut q {
            if let Some(o) = orig {
                if state.stand_ins.contains(&mat.0.id()) {
                    mat.0 = o.0.clone();
                }
                commands.entity(e).remove::<FocusOriginal>();
            }
        }
        for (e, mut mat, orig) in &mut windows {
            if let Some(o) = orig {
                if state.window_stand_ins.contains(&mat.0.id()) {
                    mat.0 = o.0.clone();
                }
                commands.entity(e).remove::<FocusOriginalWindow>();
            }
        }
        state.active = None;
    }
    let Some(focus) = want else { return };
    let first = state.active.is_none();
    state.active = Some(focus);
    let overlay = mats.overlay.id();
    for (e, mut mat, orig, (car, unit, police, keep, emergency)) in &mut q {
        if !first && orig.is_some() && !mat.is_changed() {
            continue;
        }
        let id = mat.0.id();
        if state.stand_ins.contains(&id) || id == overlay || keep {
            continue;
        }
        let class = match focus {
            Focus::Traffic if car || unit => glow(TRAFFIC_BLUE),
            // Fires and smoke stay as they are: they are part of the story.
            Focus::Traffic if emergency => Class::Keep,
            Focus::Crime if police => glow(POLICE_GLOW),
            Focus::Crime if emergency => Class::Keep,
            _ => Class::Dim(focus),
        };
        if class == Class::Keep {
            continue;
        }
        let Some(o) = materials.get(id) else { continue };
        // A police light bar keeps its red and blue; only the body glows.
        let class = if police && o.unlit && focus == Focus::Crime { Class::Keep } else { class };
        if class == Class::Keep {
            continue;
        }
        let handle = match state.derived.get(&(id, class)) {
            Some(h) => h.clone(),
            None => {
                let m = stand_in(o, class);
                let h = materials.add(m);
                state.stand_ins.insert(h.id());
                state.derived.insert((id, class), h.clone());
                h
            }
        };
        commands.entity(e).insert(FocusOriginal(mat.0.clone()));
        mat.0 = handle;
    }
    let (_, _, window_k) = focus.dim();
    for (e, mut mat, orig) in &mut windows {
        if !first && orig.is_some() && !mat.is_changed() {
            continue;
        }
        let id = mat.0.id();
        if state.window_stand_ins.contains(&id) {
            continue;
        }
        let handle = match state.windows.get(&(id, focus)) {
            Some(h) => h.clone(),
            None => {
                let Some(o) = window_materials.get(id) else { continue };
                let mut m = o.clone();
                m.base.base_color = Color::linear_rgb(window_k, window_k, window_k);
                let h = window_materials.add(m);
                state.window_stand_ins.insert(h.id());
                state.windows.insert((id, focus), h.clone());
                h
            }
        };
        commands.entity(e).insert(FocusOriginalWindow(mat.0.clone()));
        mat.0 = handle;
    }
}
