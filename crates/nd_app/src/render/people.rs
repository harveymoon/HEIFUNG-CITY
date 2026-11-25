//! [looks] Citizens drawn with their own look.
//!
//! Every figure is a small vertex-coloured mesh (`nd_procgen::people`),
//! built once per quantised look (`PersonKey`) and shared by everyone who
//! looks alike; size is the entity's scale. All of them use one lit
//! material whose emissive map is the trim palette, so the whole crowd is a
//! single material (batched across meshes) however varied it is.
//!
//! The mesh cache is bounded (`FigureCache`): only people outdoors hold a
//! figure (indoors they drop back to the plain stand-in, releasing it), and
//! when the cache is full the look least recently on the street goes. The
//! people nearest the camera claim their figures first, so when more looks
//! are outdoors than the cache holds it is the distant ones that borrow a
//! cached figure of the same silhouette.

use super::*;
use nd_procgen::people::{emissive_palette, person_key, person_mesh, FigureCache, PersonKey, EMISSIVE_CELLS};

/// Figures kept at most.
pub const MESH_CAP: usize = 1024;

#[derive(Resource)]
pub(super) struct PeopleRender {
    material: Option<Handle<StandardMaterial>>,
    pub(super) cache: FigureCache<Handle<Mesh>>,
    night_key: Option<i32>,
    /// [r11-adopt-frames] The crowd as `sync_citizens` last worked it out.
    pub(super) crowd: Crowd,
}

/// [r11-adopt-frames] Who is out and how they look, kept while the city on
/// screen stays the same (`SimCore::changes`).
#[derive(Default)]
pub(super) struct Crowd {
    /// What the crowd was worked out for: the city's change count, its
    /// generation, the Underground view, plain figures.
    pub(super) key: Option<(u64, u64, bool, bool)>,
    /// Everyone shown: id, entity, look, stature, where they stand.
    pub(super) outdoors: Vec<(CitizenId, Entity, PersonKey, f32, SVec2)>,
    /// [r11-adopt-frames] Looks drawn from seeds, by citizen ([`kept_look`]).
    pub(super) looks: HashMap<CitizenId, KeptLook>,
    /// Everyone's entity and looks pass flags: shown (1), in the spotlight (2).
    pub(super) flags: Vec<(CitizenId, Entity, u8)>,
    /// What the materials were last chosen for (generation, Underground
    /// view, plain figures, the selected citizen, the figures' material):
    /// when it moves, everyone is set again.
    pub(super) looks_key: Option<(u64, bool, bool, Option<CitizenId>, AssetId<StandardMaterial>)>,
    /// The city change count the looks pass last ran for, the flags each
    /// figure was last set by (only those whose flags moved are set
    /// again), and whether some figure was not reachable yet (spawned that
    /// frame).
    pub(super) looks_seen: Option<u64>,
    pub(super) applied: HashMap<CitizenId, u8>,
    pub(super) looks_missed: bool,
}

impl Default for PeopleRender {
    fn default() -> Self {
        Self { material: None, cache: FigureCache::new(MESH_CAP), night_key: None, crowd: Crowd::default() }
    }
}

impl PeopleRender {
    /// The one material every figure uses.
    pub(super) fn material(&mut self, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) -> Handle<StandardMaterial> {
        if let Some(m) = &self.material {
            return m.clone();
        }
        let srgb = |v: f32| {
            let v = v.clamp(0.0, 1.0);
            let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0).round() as u8
        };
        let data: Vec<u8> = emissive_palette().iter().flat_map(|c| [srgb(c[0]), srgb(c[1]), srgb(c[2]), 255]).collect();
        let mut img = Image::new(
            Extent3d { width: EMISSIVE_CELLS as u32, height: 1, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = ImageSampler::nearest();
        let palette = images.add(img);
        let m = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.85,
            reflectance: 0.3,
            emissive: LinearRgba::BLACK,
            emissive_texture: Some(palette),
            ..default()
        });
        self.material = Some(m.clone());
        m
    }

    /// [r17-people] The figures' material, once made.
    pub(super) fn material_handle(&self) -> Option<Handle<StandardMaterial>> {
        self.material.clone()
    }

    /// The trim and the clothes glow with the night (`night` 0 day to 1).
    pub(super) fn set_night(&mut self, night: f32, materials: &mut Assets<StandardMaterial>) {
        let key = (night * 32.0).round() as i32;
        if self.night_key == Some(key) {
            return;
        }
        self.night_key = Some(key);
        if let Some(mut m) = self.material.as_ref().and_then(|h| materials.get_mut(h)) {
            // A touch by day (the trim catches the eye), more at night.
            let k = 0.15 + 2.2 * key as f32 / 32.0;
            m.emissive = LinearRgba::rgb(k, k, k);
        }
    }

    pub(super) fn mesh_for(&mut self, key: PersonKey, meshes: &mut Assets<Mesh>) -> Option<Handle<Mesh>> {
        self.cache.get_or_build(key, |k| {
            let _b = crate::frame_stats::span("fig_build");
            meshes.add(shared_mesh(&person_mesh(k), false))
        })
    }
}

/// `ND_PLAIN_PEOPLE=1`: draw everyone as the old plain figure, for
/// comparing frame rates.
pub(super) fn plain() -> bool {
    static PLAIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PLAIN.get_or_init(|| std::env::var("ND_PLAIN_PEOPLE").is_ok_and(|v| v == "1"))
}

/// The key and size of a citizen's figure as they are now.
pub(super) fn figure_of(c: &nd_sim::Citizen) -> (PersonKey, f32) {
    figure_with(c, c.appearance())
}

/// [r11-adopt-frames] [`figure_of`] with the citizen's look given (as
/// `Citizen::appearance` has it, perhaps kept from before: [`kept_look`]).
pub(super) fn figure_with(c: &nd_sim::Citizen, look: nd_sim::looks::Appearance) -> (PersonKey, f32) {
    let riding = c.mode == nd_sim::citizens::TravelMode::Bike && c.activity.is_traveling();
    (person_key(&look, c.age, c.class(), riding), look.stature(c.age))
}

/// [r11-adopt-frames] A look drawn from a citizen's seed, with what it was
/// drawn from (seed, age, class).
pub(super) type KeptLook = (u64, u8, nd_sim::Tier, nd_sim::looks::Appearance);

/// [r11-adopt-frames] A citizen's look as `Citizen::appearance` gives it:
/// their own, or the one kept for them while their seed, age and class
/// stay the same, or drawn afresh (several seeded generators, ~1 µs; a
/// 38,000-person city's crowd was drawn afresh with every batch). The
/// second value is a look drawn afresh, to keep.
pub(super) fn kept_look(c: &nd_sim::Citizen, kept: Option<&KeptLook>) -> (nd_sim::looks::Appearance, Option<KeptLook>) {
    if let Some(l) = c.look {
        return (l, None);
    }
    let class = c.class();
    match kept {
        Some(k) if (k.0, k.1, k.2) == (c.seed, c.age, class) => (k.3, None),
        _ => {
            let l = c.appearance();
            (l, Some((c.seed, c.age, class, l)))
        }
    }
}
