//! One material swapper for every view that redraws the city in stand-in
//! materials: drone heat vision (`heat`) and the Traffic and Crime focus
//! views (`focus`).
//!
//! Each mesh keeps wearing its own material handle; while a view is on it
//! wears a stand-in derived from that material instead, and the material it
//! stands in for is kept on the entity ([`SwapOriginal`]). There is only
//! ever one original per entity, whatever the view, and a stand-in is always
//! derived from that original, never from another view's stand-in. Two
//! swappers that each kept their own original could hand each other their
//! stand-ins: turning heat vision off with the Traffic layer on used to make
//! the focus view take heat's stand-ins for originals and put them back
//! when it ended, leaving the city in heat colours.
//!
//! Systems that pick a material for an entity (signals, citizens) compare
//! against [`SwapOriginal`] when it is there, and simply set the material
//! they want: the swapper sees the change and swaps the new one.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::focus::{self, Focus};
use super::{heat, lights::WindowMaterial, BotTag, CarTag, CitizenTag, Lighting, Materials, Overlays, ParkedTag, ShipTag, TrainTag};

/// Which view the city is drawn in.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub(super) enum View {
    #[default]
    Normal,
    Focus(Focus),
    Heat,
    /// [pt-ux] Some things drawn see-through over the rest (`Ghost`).
    Ghost(Ghost),
}

/// [pt-ux] What a ghost view makes see-through.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Ghost {
    /// The Underground view: the surface is hidden (`holo::apply_depth`)
    /// but trains and aircraft, which pass over the tunnels, stay as
    /// ghosts so the city above can still be read.
    Underground,
    /// The Zone tool: buildings see-through so the zoning under them shows
    /// (F29).
    Zoning,
}

/// How see-through a ghost is (alpha).
pub(super) const GHOST_ALPHA: f32 = 0.22;

impl Ghost {
    /// Which ghost view the tools ask for.
    pub(super) fn wanted(tools: &crate::tools::ToolState) -> Option<Ghost> {
        if tools.depth == crate::tools::Depth::Underground {
            Some(Ghost::Underground)
        } else if tools.tool == crate::tools::Tool::Zone {
            Some(Ghost::Zoning)
        } else {
            None
        }
    }

    /// Whether a mesh with these tags (building, aircraft, train) goes
    /// see-through.
    pub(super) fn ghosts(self, (building, air, train): (bool, bool, bool)) -> bool {
        match self {
            Ghost::Underground => air || train,
            Ghost::Zoning => building,
        }
    }

    /// Window lights (only buildings wear them) go see-through too.
    fn ghosts_windows(self) -> bool {
        self == Ghost::Zoning
    }
}

/// [pt-ux] The see-through copy of a material: its own look, blended.
pub(super) fn ghost_of(o: &StandardMaterial) -> StandardMaterial {
    let mut m = o.clone();
    m.base_color.set_alpha(o.base_color.alpha() * GHOST_ALPHA);
    m.alpha_mode = AlphaMode::Blend;
    m
}

impl View {
    /// Heat vision wins over the focus views; the focus views stand down
    /// under it, so the two never stack.
    pub(super) fn wanted(o: &Overlays, l: &Lighting, ghost: Option<Ghost>) -> View {
        if l.heat {
            View::Heat
        } else if let Some(f) = Focus::wanted(o, l) {
            View::Focus(f)
        } else if let Some(g) = ghost {
            View::Ghost(g)
        } else {
            View::Normal
        }
    }
}

/// The material an entity wears outside the stand-in views.
#[derive(Component)]
pub(super) struct SwapOriginal(pub(super) Handle<StandardMaterial>);

#[derive(Component)]
pub(super) struct SwapOriginalWindow(pub(super) Handle<WindowMaterial>);

/// How a stand-in is derived from its original.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Heat(heat::HeatClass),
    Focus(focus::Class),
    Ghost,
}

#[derive(Resource, Default)]
pub(super) struct SwapState {
    active: View,
    /// Stand-ins by original material and derivation.
    derived: HashMap<(AssetId<StandardMaterial>, Key), Handle<StandardMaterial>>,
    /// Every stand-in, to tell them from originals.
    stand_ins: HashSet<AssetId<StandardMaterial>>,
    windows: HashMap<(AssetId<WindowMaterial>, View), Handle<WindowMaterial>>,
    window_stand_ins: HashSet<AssetId<WindowMaterial>>,
}

/// What an entity is, for heat (how hot) and the focus views (what lights up).
type HeatTags = (Has<CarTag>, Has<CitizenTag>, Has<BotTag>, Has<TrainTag>, Has<ParkedTag>, Has<ShipTag>, Has<super::emergency::EmergencyTag>, Has<super::stunts::StuntTag>, Has<super::air::AirTag>, Has<super::haze::Puff>);
type GhostTags = (Has<super::BuildingTag>, Has<super::air::AirTag>, Has<TrainTag>);
type FocusTags = (Has<CarTag>, Has<focus::UnitTag>, Has<focus::PoliceTag>, Has<focus::FocusKeep>, Has<super::emergency::EmergencyTag>);

/// Put every mesh in the material the current view wants: a stand-in of its
/// original, or the original itself. A view change revisits every mesh; in
/// between, only meshes whose material was just set (spawned, or changed by
/// another system) are looked at.
#[allow(clippy::type_complexity)]
pub(super) fn swap_materials(
    mut commands: Commands,
    lighting: Res<Lighting>,
    overlays: Res<Overlays>,
    mats: Res<Materials>,
    tools: Option<Res<crate::tools::ToolState>>,
    mut state: ResMut<SwapState>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut window_materials: ResMut<Assets<WindowMaterial>>,
    mut q: Query<(Entity, &mut MeshMaterial3d<StandardMaterial>, Option<&SwapOriginal>, HeatTags, FocusTags, GhostTags)>,
    mut windows: Query<(Entity, &mut MeshMaterial3d<WindowMaterial>, Option<&SwapOriginalWindow>)>,
) {
    let want = View::wanted(&overlays, &lighting, tools.as_deref().and_then(Ghost::wanted));
    let changed = want != state.active;
    if want == View::Normal && !changed {
        return;
    }
    state.active = want;
    let overlay = mats.overlay.id();
    for (e, mut mat, orig, heat_tags, focus_tags, ghost_tags) in &mut q {
        if !changed && !mat.is_changed() {
            continue;
        }
        let showing_stand_in = state.stand_ins.contains(&mat.0.id());
        // What the entity really wears: the original behind a stand-in, or
        // the material it was just given.
        let original = if showing_stand_in {
            match orig {
                Some(o) => o.0.clone(),
                None => continue,
            }
        } else {
            mat.0.clone()
        };
        let id = original.id();
        let key = match (want, materials.get(id)) {
            (View::Normal, _) | (_, None) => None,
            (View::Heat, Some(o)) => Some(Key::Heat(heat::class_of(o, heat::temperature(heat_tags)))),
            (View::Focus(f), Some(o)) => focus::classify(f, focus_tags, heat_tags.1, id == overlay, o).map(Key::Focus),
            (View::Ghost(g), Some(_)) => (g.ghosts(ghost_tags) && id != overlay).then_some(Key::Ghost),
        };
        let Some(key) = key else {
            if showing_stand_in {
                mat.0 = original;
            }
            if orig.is_some() {
                commands.entity(e).remove::<SwapOriginal>();
            }
            continue;
        };
        let handle = match state.derived.get(&(id, key)) {
            Some(h) => h.clone(),
            None => {
                let Some(o) = materials.get(id) else { continue };
                let m = match key {
                    Key::Heat(c) => heat::stand_in(o, c),
                    Key::Focus(c) => focus::stand_in(o, c),
                    Key::Ghost => ghost_of(o),
                };
                let h = materials.add(m);
                state.stand_ins.insert(h.id());
                state.derived.insert((id, key), h.clone());
                h
            }
        };
        if mat.0 != handle {
            mat.0 = handle;
        }
        if orig.is_none_or(|o| o.0 != original) {
            commands.entity(e).insert(SwapOriginal(original));
        }
    }
    let window_k = match want {
        View::Normal => None,
        View::Ghost(g) if !g.ghosts_windows() => None,
        // A ghost window keeps its colour (the alpha is what changes).
        View::Ghost(_) => Some(-1.0),
        View::Heat => Some(heat::COOL_WINDOW),
        View::Focus(f) => Some(f.dim().2),
    };
    for (e, mut mat, orig) in &mut windows {
        if !changed && !mat.is_changed() {
            continue;
        }
        let showing_stand_in = state.window_stand_ins.contains(&mat.0.id());
        let original = if showing_stand_in {
            match orig {
                Some(o) => o.0.clone(),
                None => continue,
            }
        } else {
            mat.0.clone()
        };
        let Some(k) = window_k else {
            if showing_stand_in {
                mat.0 = original;
            }
            if orig.is_some() {
                commands.entity(e).remove::<SwapOriginalWindow>();
            }
            continue;
        };
        let id = original.id();
        let handle = match state.windows.get(&(id, want)) {
            Some(h) => h.clone(),
            None => {
                let Some(o) = window_materials.get(id) else { continue };
                let mut m = o.clone();
                // [chase-cam] Nothing is cut in these views.
                m.extension.cut = Default::default();
                if k < 0.0 {
                    m.base = ghost_of(&o.base);
                } else {
                    m.base.base_color = Color::linear_rgb(k, k, k);
                }
                let h = window_materials.add(m);
                state.window_stand_ins.insert(h.id());
                state.windows.insert((id, want), h.clone());
                h
            }
        };
        if mat.0 != handle {
            mat.0 = handle;
        }
        if orig.is_none_or(|o| o.0 != original) {
            commands.entity(e).insert(SwapOriginalWindow(original));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [pt-ux] What each ghost view makes see-through, and that heat
    /// vision and the focus layers win over it.
    #[test]
    fn ghosts_by_view() {
        // (building, aircraft, train)
        assert!(Ghost::Underground.ghosts((false, true, false)));
        assert!(Ghost::Underground.ghosts((false, false, true)));
        assert!(!Ghost::Underground.ghosts((true, false, false)));
        assert!(Ghost::Zoning.ghosts((true, false, false)));
        assert!(!Ghost::Zoning.ghosts((false, true, true)));
        let o = Overlays::default();
        let mut l = Lighting::default();
        assert_eq!(View::wanted(&o, &l, Some(Ghost::Zoning)), View::Ghost(Ghost::Zoning));
        assert_eq!(View::wanted(&o, &l, None), View::Normal);
        l.heat = true;
        assert_eq!(View::wanted(&o, &l, Some(Ghost::Zoning)), View::Heat);
        let m = ghost_of(&StandardMaterial::default());
        assert!(matches!(m.alpha_mode, AlphaMode::Blend));
        assert!((m.base_color.alpha() - GHOST_ALPHA).abs() < 1e-6);
    }

    /// Heat vision on and off with the Traffic layer on, in every order:
    /// every mesh ends up in its own material, never a stand-in.
    #[test]
    fn views_restore_the_originals() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(AssetPlugin::default());
        app.init_asset::<StandardMaterial>().init_asset::<WindowMaterial>().init_asset::<Image>();
        app.init_resource::<Lighting>().init_resource::<Overlays>().init_resource::<SwapState>();
        let (red, car_paint, overlay) = {
            let mut m = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            (m.add(StandardMaterial { base_color: Color::linear_rgb(1.0, 0.0, 0.0), ..default() }), m.add(StandardMaterial::default()), m.add(StandardMaterial { unlit: true, ..default() }))
        };
        let windows = {
            let lit = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
            app.world_mut().resource_mut::<Assets<WindowMaterial>>().add(WindowMaterial { base: StandardMaterial::default(), extension: super::super::lights::WindowLights { lit, clock: Vec4::ZERO, cut: Default::default() } })
        };
        app.insert_resource(Materials::for_test(overlay.clone(), windows.clone()));
        app.add_systems(Update, swap_materials);
        let wall = app.world_mut().spawn(MeshMaterial3d(red.clone())).id();
        let car = app.world_mut().spawn((MeshMaterial3d(car_paint.clone()), CarTag)).id();
        let glow = app.world_mut().spawn(MeshMaterial3d(windows.clone())).id();
        let check = |app: &mut App, stand_in: bool| {
            let w = app.world();
            let wall_m = &w.get::<MeshMaterial3d<StandardMaterial>>(wall).unwrap().0;
            let car_m = &w.get::<MeshMaterial3d<StandardMaterial>>(car).unwrap().0;
            let glow_m = &w.get::<MeshMaterial3d<WindowMaterial>>(glow).unwrap().0;
            assert_eq!(*wall_m != red, stand_in);
            assert_eq!(*car_m != car_paint, stand_in);
            assert_eq!(*glow_m != windows, stand_in);
        };
        // Traffic on, heat on, traffic off, heat off; and the other orders.
        let steps: [(bool, bool); 8] = [(true, false), (true, true), (false, true), (false, false), (false, true), (true, true), (true, false), (false, false)];
        for (traffic, heat) in steps {
            app.world_mut().resource_mut::<Overlays>().traffic = traffic;
            app.world_mut().resource_mut::<Lighting>().heat = heat;
            app.update();
            app.update();
            check(&mut app, traffic || heat);
        }
        // The originals are untouched.
        let m = app.world().resource::<Assets<StandardMaterial>>();
        assert_eq!(m.get(&red).unwrap().base_color, Color::linear_rgb(1.0, 0.0, 0.0));
        assert!(app.world().get::<SwapOriginal>(wall).is_none());
    }
}
