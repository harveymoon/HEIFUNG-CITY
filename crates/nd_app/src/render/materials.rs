//! The shared surface materials and their tiling textures, and the scene set-up.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

#[derive(Resource)]
pub(super) struct Materials {
    /// One material per [`Surface`], indexed by `Surface::index()`.
    pub(super) surfaces: Vec<Handle<StandardMaterial>>,
    /// The plain citizen tint from before looks (`ND_PLAIN_PEOPLE=1`).
    pub(super) citizen: Handle<StandardMaterial>,
    pub(super) citizen_spotlight: Handle<StandardMaterial>,
    pub(super) citizen_selected: Handle<StandardMaterial>,
    pub(super) citizen_mesh: Handle<Mesh>,
    pub(super) bike_mesh: Handle<Mesh>,
    pub(super) car_bodies: Vec<Handle<StandardMaterial>>,
    pub(super) loco_mesh: Handle<Mesh>,
    pub(super) coach_mesh: Handle<Mesh>,
    pub(super) wagon_mesh: Handle<Mesh>,
    pub(super) signal_green: Handle<StandardMaterial>,
    pub(super) signal_red: Handle<StandardMaterial>,
    pub(super) signal_mesh: Handle<Mesh>,
    pub(super) ped_walk: Handle<StandardMaterial>,
    pub(super) ped_stop: Handle<StandardMaterial>,
    pub(super) overlay: Handle<StandardMaterial>,
    /// The surface streets seen from the underground view: faint, see-through.
    pub(super) ghost_road: Handle<StandardMaterial>,
    /// Building glow: the glow material with window lights switched by
    /// occupancy (see `lights`).
    pub(super) windows: Handle<lights::WindowMaterial>,
    /// The lit share of every building, one texel each.
    pub(super) lit: Handle<Image>,
}

impl Materials {
    pub(super) fn surface(&self, s: Surface) -> Handle<StandardMaterial> {
        self.surfaces[s.index()].clone()
    }
    pub(super) fn glow(&self) -> Handle<StandardMaterial> {
        self.surface(Surface::Glow)
    }

    /// Placeholder handles except the two the material swapper reads.
    #[cfg(test)]
    pub(super) fn for_test(overlay: Handle<StandardMaterial>, windows: Handle<lights::WindowMaterial>) -> Self {
        Self {
            surfaces: Vec::new(),
            citizen: default(),
            citizen_spotlight: default(),
            citizen_selected: default(),
            citizen_mesh: default(),
            bike_mesh: default(),
            car_bodies: Vec::new(),
            loco_mesh: default(),
            coach_mesh: default(),
            wagon_mesh: default(),
            signal_green: default(),
            signal_red: default(),
            signal_mesh: default(),
            ped_walk: default(),
            ped_stop: default(),
            overlay,
            ghost_road: default(),
            windows,
            lit: default(),
        }
    }
}

pub(super) fn tiling_image(size: usize, rgba: &[u8], srgb: bool) -> Image {
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let mut img = Image::new(
        Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        rgba.to_vec(),
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    let (data, levels) = mip_chain(size, rgba);
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = levels;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

pub(super) fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut window_materials: ResMut<Assets<lights::WindowMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    assets: Res<GameAssets>,
) {
    let pal = &assets.palette;
    let unlit = |c: [f32; 3]| StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), unlit: true, ..default() };

    // Surface materials with generated textures.
    const TEX: usize = 256;
    let mut surfaces = Vec::with_capacity(Surface::ALL.len());
    for s in Surface::ALL {
        let size = s.texels(TEX);
        let mat = match generate_texture(s, size) {
            Some(tex) => {
                let color = images.add(tiling_image(size, &tex.color, true));
                let normal = images.add(tiling_image(size, &tex.normal, false));
                // [look] Roughness (and on metal, metalness) varies over the
                // texture; the map holds the absolute roughness.
                let rough = images.add(tiling_image(size, &nd_procgen::roughness_map(s, &tex), false));
                StandardMaterial {
                    base_color: Color::WHITE,
                    base_color_texture: Some(color),
                    normal_map_texture: Some(normal),
                    metallic_roughness_texture: Some(rough),
                    perceptual_roughness: 1.0,
                    metallic: s.metallic(),
                    reflectance: if s == Surface::Glass { 0.6 } else { 0.4 },
                    uv_transform: Affine2::from_scale(Vec2::splat(1.0 / s.tile())),
                    ..default()
                }
            }
            None => StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() },
        };
        surfaces.push(materials.add(mat));
    }

    let citizen = materials.add(unlit(pal.citizen));
    let citizen_spotlight = materials.add(unlit(pal.citizen_spotlight));
    let citizen_selected = materials.add(unlit([8.0, 8.0, 8.0]));
    let citizen_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 0.42, 0.0), Vec3::new(0.34, 0.84, 0.28), 0.0, [0.45, 0.45, 0.5, 1.0]);
        m.boxed(Vec3::new(0.0, 1.14, 0.0), Vec3::new(0.5, 0.62, 0.3), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(0.0, 1.62, 0.0), Vec3::new(0.25, 0.26, 0.25), 0.0, [1.3, 1.2, 1.1, 1.0]);
        meshes.add(shared_mesh(&m, false))
    };
    let bike_mesh = {
        let mut m = MeshData::default();
        // wheels, frame, seated rider
        for x in [-0.55f32, 0.55] {
            m.boxed(Vec3::new(x, 0.34, 0.0), Vec3::new(0.68, 0.68, 0.06), 0.0, [0.1, 0.1, 0.1, 1.0]);
        }
        m.boxed(Vec3::new(0.0, 0.6, 0.0), Vec3::new(1.1, 0.06, 0.06), 0.0, [0.6, 0.6, 0.65, 1.0]);
        m.boxed(Vec3::new(0.35, 0.95, 0.0), Vec3::new(0.06, 0.6, 0.4), 0.0, [0.6, 0.6, 0.65, 1.0]);
        m.boxed(Vec3::new(-0.15, 0.95, 0.0), Vec3::new(0.4, 0.5, 0.26), 0.0, [0.45, 0.45, 0.5, 1.0]);
        m.boxed(Vec3::new(-0.2, 1.4, 0.0), Vec3::new(0.45, 0.55, 0.3), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(-0.15, 1.82, 0.0), Vec3::new(0.25, 0.26, 0.25), 0.0, [1.3, 1.2, 1.1, 1.0]);
        meshes.add(shared_mesh(&m, false))
    };
    let car_bodies = pal
        .car_colors
        .iter()
        .map(|&c| materials.add(StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), perceptual_roughness: 0.35, metallic: 0.6, ..default() }))
        .collect();
    let rail_wheels = |m: &mut MeshData, len: f32| {
        for x in [-len * 0.35, len * 0.35] {
            for s in [-1.0f32, 1.0] {
                m.boxed(Vec3::new(x, 0.5, s * 0.75), Vec3::new(2.6, 0.7, 0.3), 0.0, [0.1, 0.1, 0.1, 1.0]);
            }
        }
    };
    let loco_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 2.1, 0.0), Vec3::new(17.4, 2.6, 2.8), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(5.5, 3.7, 0.0), Vec3::new(5.0, 0.8, 2.6), 0.0, [0.2, 0.22, 0.25, 1.0]);
        m.boxed(Vec3::new(7.5, 2.6, 0.0), Vec3::new(1.2, 1.4, 2.5), 0.0, [0.14, 0.17, 0.2, 1.0]);
        m.boxed(Vec3::new(8.8, 1.1, 0.0), Vec3::new(0.6, 0.6, 2.4), 0.0, [0.9, 0.2, 0.1, 1.0]);
        m.boxed(Vec3::new(-2.0, 3.6, 0.0), Vec3::new(9.0, 0.5, 2.0), 0.0, [0.3, 0.3, 0.32, 1.0]);
        rail_wheels(&mut m, 17.0);
        meshes.add(shared_mesh(&m, true))
    };
    let coach_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 2.1, 0.0), Vec3::new(18.4, 2.6, 2.8), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(0.0, 2.6, 0.0), Vec3::new(17.8, 0.9, 2.86), 0.0, [0.14, 0.17, 0.2, 1.0]);
        m.boxed(Vec3::new(0.0, 3.5, 0.0), Vec3::new(18.0, 0.3, 2.4), 0.0, [0.5, 0.5, 0.52, 1.0]);
        rail_wheels(&mut m, 18.0);
        meshes.add(shared_mesh(&m, true))
    };
    let wagon_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 1.0, 0.0), Vec3::new(13.6, 0.5, 2.7), 0.0, [0.2, 0.2, 0.22, 1.0]);
        m.boxed(Vec3::new(0.0, 2.55, 0.0), Vec3::new(12.2, 2.6, 2.44), 0.0, [1.0, 1.0, 1.0, 1.0]);
        rail_wheels(&mut m, 13.0);
        meshes.add(shared_mesh(&m, true))
    };
    let overlay = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
    let ghost_road = materials.add(StandardMaterial { base_color: Color::linear_rgba(0.55, 0.75, 1.3, 0.2), unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
    let lit = images.add(lights::lit_image());
    let windows = window_materials.add(lights::WindowMaterial {
        base: StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() },
        extension: lights::WindowLights { lit: lit.clone(), clock: Vec4::ZERO, cut: default() },
    });
    let signal_green = materials.add(unlit(pal.signal_green));
    let signal_red = materials.add(unlit(pal.signal_red));
    let ped_walk = materials.add(unlit([0.85, 0.95, 1.0]));
    let ped_stop = materials.add(unlit([1.0, 0.42, 0.06]));
    let signal_mesh = {
        let mut m = MeshData::default();
        m.boxed(Vec3::ZERO, Vec3::new(0.08, 0.26, 0.26), 0.0, [1.0, 1.0, 1.0, 1.0]);
        meshes.add(shared_mesh(&m, false))
    };
    commands.insert_resource(Materials {
        surfaces,
        citizen,
        citizen_spotlight,
        citizen_selected,
        citizen_mesh,
        bike_mesh,
        car_bodies,
        loco_mesh,
        coach_mesh,
        wagon_mesh,
        signal_green,
        signal_red,
        signal_mesh,
        ped_walk,
        ped_stop,
        overlay,
        ghost_road,
        windows,
        lit,
    });

    commands.insert_resource(GlobalAmbientLight { color: Color::linear_rgb(0.35, 0.45, 0.9), brightness: 70.0, ..default() });
    commands.spawn((
        DirectionalLight { color: Color::linear_rgb(0.45, 0.55, 1.0), illuminance: 700.0, shadow_maps_enabled: true, ..default() },
        CascadeShadowConfigBuilder { num_cascades: 3, minimum_distance: 0.5, maximum_distance: SHADOW_FAR, first_cascade_far_bound: SHADOW_FIRST, ..default() }.build(),
        Transform::from_xyz(300.0, 500.0, -200.0).looking_at(Vec3::ZERO, Vec3::Y),
        SunTag,
    ));
}
