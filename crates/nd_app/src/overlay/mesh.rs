//! [overlay] Mesh overlays: meshes over the city kept in step with what
//! they show. Each says what it shows as a key ([`MeshOverlay::key`]); one
//! generic system ([`sync`]) rebuilds its pieces when the key changes (or
//! the renderers rebuild everything, `Sim::full_rebuild`), and otherwise
//! leaves them alone or restyles them ([`MeshOverlay::restyle`]). The
//! pieces are unlit, cast and take no shadows, and carry [`OverlayPiece`].

use std::collections::HashMap;
use std::marker::PhantomData;

use bevy::ecs::system::{StaticSystemParam, SystemParam, SystemParamItem};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use nd_procgen::MeshData;

use super::OverlaySet;
use crate::sim::Sim;

/// On every mesh an overlay spawns.
#[derive(Component, Default)]
pub struct OverlayPiece;

/// What an overlay wants shown this frame.
pub enum Wanted<K> {
    /// Leave the pieces as they are (not even a restyle).
    Unchanged,
    /// Nothing: the pieces go.
    Nothing,
    /// What `key` names: built when it differs from what is shown.
    Show(K),
}

/// A mesh to spawn and how.
struct Piece {
    mesh: MeshData,
    material: Handle<StandardMaterial>,
    visibility: Visibility,
}

/// The meshes [`MeshOverlay::build`] makes.
#[derive(Default)]
pub struct Pieces(Vec<Piece>);

impl Pieces {
    /// A mesh, shown; skipped when empty.
    pub fn add(&mut self, mesh: MeshData, material: Handle<StandardMaterial>) {
        if !mesh.is_empty() {
            self.0.push(Piece { mesh, material, visibility: Visibility::Inherited });
        }
    }

    /// A mesh spawned hidden, even when empty, so the pieces stay one to
    /// one with what they stand for (for an overlay that restyles them).
    pub fn add_hidden(&mut self, mesh: MeshData, material: Handle<StandardMaterial>) {
        self.0.push(Piece { mesh, material, visibility: Visibility::Hidden });
    }
}

/// The overlays' materials, unlit and additive, each made on first use.
/// Overlays that animate theirs (a pulse in the alpha) have their own.
#[derive(Resource, Default)]
pub struct OverlayMaterials(HashMap<OverlayMat, Handle<StandardMaterial>>);

/// The materials there are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverlayMat {
    /// The plot under the pointer (`render::plot_hover`): pulsed.
    PlotHover,
    /// Build highlight service areas.
    HighlightArea,
    /// Build highlight outlines and walls: pulsed.
    HighlightWall,
    /// Water routes in focus.
    WaterBright,
    /// The rest of the water network.
    WaterDim,
}

impl OverlayMat {
    fn material(self) -> StandardMaterial {
        let water = |k: f32| Color::linear_rgba(k, k, k, 1.0);
        let (base, depth_bias) = match self {
            OverlayMat::PlotHover => (Color::WHITE, 0.0),
            OverlayMat::HighlightArea => (Color::WHITE, 4.0),
            OverlayMat::HighlightWall => (Color::WHITE, 6.0),
            OverlayMat::WaterBright => (water(1.0), 3.0),
            OverlayMat::WaterDim => (water(0.16), 3.0),
        };
        StandardMaterial { base_color: base, unlit: true, alpha_mode: AlphaMode::Add, cull_mode: None, fog_enabled: false, depth_bias, ..default() }
    }
}

impl OverlayMaterials {
    pub fn get(&mut self, materials: &mut Assets<StandardMaterial>, m: OverlayMat) -> Handle<StandardMaterial> {
        self.0.entry(m).or_insert_with(|| materials.add(m.material())).clone()
    }
}

/// What the overlay hooks get besides their own parameters.
pub struct OverlayCx<'a> {
    pub sim: &'a Sim,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub mats: &'a mut OverlayMaterials,
}

impl OverlayCx<'_> {
    /// One of the overlay materials.
    pub fn mat(&mut self, m: OverlayMat) -> Handle<StandardMaterial> {
        self.mats.get(self.materials, m)
    }
}

/// A mesh overlay (see the module docs).
pub trait MeshOverlay: Send + Sync + 'static {
    /// What is shown: the pieces are rebuilt when it changes.
    type Key: PartialEq + Send + Sync + 'static;
    /// The overlay's own system parameters.
    type Param: SystemParam + 'static;
    /// A marker on its pieces, besides [`OverlayPiece`].
    type Marker: Component + Default;
    /// Every frame: what to show. Per-frame work that does not need a
    /// rebuild (a material's pulse) goes here too.
    fn key(p: &mut SystemParamItem<Self::Param>, cx: &mut OverlayCx) -> Wanted<Self::Key>;
    /// The pieces for `key`.
    fn build(p: &mut SystemParamItem<Self::Param>, cx: &mut OverlayCx, key: &Self::Key, out: &mut Pieces);
    /// A frame the key held (and nothing was rebuilt): restyle the pieces
    /// spawned by the last build, in the order it added them.
    fn restyle(_p: &mut SystemParamItem<Self::Param>, _cx: &mut OverlayCx, _key: &Self::Key, _pieces: &[Entity]) {}
}

/// An overlay's shown key and pieces.
#[derive(Resource)]
pub struct OverlayState<T: MeshOverlay> {
    key: Option<T::Key>,
    pieces: Vec<Entity>,
}

impl<T: MeshOverlay> Default for OverlayState<T> {
    fn default() -> Self {
        Self { key: None, pieces: Vec::new() }
    }
}

/// Registers [`sync`] for `T` in [`OverlaySet::Build`].
pub struct MeshOverlayPlugin<T>(PhantomData<T>);

impl<T> Default for MeshOverlayPlugin<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: MeshOverlay> Plugin for MeshOverlayPlugin<T> {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<OverlayMaterials>() {
            app.init_resource::<OverlayMaterials>();
        }
        app.init_resource::<OverlayState<T>>().add_systems(Update, sync::<T>.in_set(OverlaySet::Build));
    }
}

/// Keep `T`'s pieces in step with its key.
#[allow(clippy::too_many_arguments)]
pub fn sync<T: MeshOverlay>(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mats: ResMut<OverlayMaterials>,
    sim: Res<Sim>,
    mut st: ResMut<OverlayState<T>>,
    mut p: StaticSystemParam<T::Param>,
) {
    let mut cx = OverlayCx { sim: &sim, materials: &mut materials, mats: &mut mats };
    let key = match T::key(&mut p, &mut cx) {
        Wanted::Unchanged => return,
        Wanted::Nothing => None,
        Wanted::Show(k) => Some(k),
    };
    let st = &mut *st;
    if st.key == key && !sim.full_rebuild {
        if let Some(k) = &st.key {
            T::restyle(&mut p, &mut cx, k, &st.pieces);
        }
        return;
    }
    for e in st.pieces.drain(..) {
        commands.entity(e).try_despawn();
    }
    st.key = key;
    let Some(k) = &st.key else { return };
    let mut out = Pieces::default();
    T::build(&mut p, &mut cx, k, &mut out);
    for piece in out.0 {
        let mesh = meshes.add(crate::render::to_mesh(&piece.mesh, false));
        let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(piece.material), Transform::IDENTITY, piece.visibility, NotShadowCaster, NotShadowReceiver, OverlayPiece, T::Marker::default())).id();
        st.pieces.push(e);
    }
}
