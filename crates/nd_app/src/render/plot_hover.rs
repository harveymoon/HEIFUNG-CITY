//! [plot-hover] The plot under the pointer while placing: a crisp outline
//! on the ground and a wall of light rising from it, fading out with
//! height, green where a click would build and red where it would be
//! refused, with the sim's verdict as a label beside the pointer.
//!
//! Used by the Buildings tool (anything that goes on a lot) and the Zone
//! tool's brush and marquee (the parcel being painted). The verdict and
//! the outline come from one adapter, [`hover_site`], which asks the sim
//! (`World::placement_problem`, `World::parcel_refusal`) and never applies
//! rules of its own. The mesh is rebuilt only when the site, its verdict or
//! the wall's size step changes; the pulse is the material's alpha.
//!
//! The remote API stands in for the pointer with `tool {cursor: [x, y]}`
//! (also `hover`): it holds until the real pointer moves over the map.

use super::*;
use crate::tools::{Tool, ToolState, ZoneMode};

#[derive(Component)]
pub struct PlotHoverTag;

/// A site the pointer is over and what a click there would do.
#[derive(Clone, Debug, PartialEq)]
pub struct HoverSite {
    /// The ground the click would take, as a closed polygon (first point not repeated).
    pub outline: Vec<SVec2>,
    /// `Ok(note)` when a click would succeed, `Err(why)` when it would be refused.
    pub verdict: Result<String, String>,
    /// The Zone tool's parcel (a lower wall, and a label only on refusal).
    pub zone: bool,
}

/// What the pointer at `p` is over for the active tool, and the sim's
/// verdict on it. None where the tool has no plot to show (roads, tolls,
/// rail pieces and docks have their own previews; erasing zones).
///
/// The one place the verdict comes from: when `World::placement_site` /
/// `placement_problem_at` land (free placement on unzoned ground), the
/// Buildings arm becomes `placement_site(what, p)` → outline, and
/// `site_problem` → verdict.
pub fn hover_site(w: &World, tools: &ToolState, p: SVec2) -> Option<HoverSite> {
    use nd_sim::Placeable;
    match tools.tool {
        Tool::Buildings => {
            let what = tools.build_item();
            if matches!(what, Placeable::Toll(_) | Placeable::Rail(_) | Placeable::Dock(_)) {
                return None;
            }
            if let Some((id, lot)) = w.lots.iter().find(|(_, l)| l.contains(p)) {
                let verdict = match w.placement_problem(what, id) {
                    Some(why) => Err(why),
                    None => {
                        let cost = match what {
                            Placeable::Plant(k) => w.plant_cost_on(k, id),
                            _ => what.cost(),
                        };
                        Ok(format!("{} fits here: {:.0} to build", capitalised(&what.label()), cost))
                    }
                };
                return Some(HoverSite { outline: lot.polygon.clone(), verdict, zone: false });
            }
            // Ground with no lot: `World::place_building` refuses it.
            let parcel = w.parcels.parcels.get(w.parcels.at(p)?)?;
            Some(HoverSite { outline: parcel.polygon.clone(), verdict: Err("no lot here: city buildings go on a lot".into()), zone: false })
        }
        Tool::Zone if tools.zone_mode != ZoneMode::Polygon => {
            let lu = tools.paint_use(tools.stroke.as_ref().map_or(tools.unzone, |s| s.erase))?;
            let i = w.parcels.at(p)?;
            let parcel = w.parcels.parcels.get(i)?;
            let verdict = match w.parcel_refusal(i, Some(lu)) {
                Some(why) => Err(why.to_string()),
                None => Ok(format!("{} zoning", lu.label())),
            };
            Some(HoverSite { outline: parcel.polygon.clone(), verdict, zone: true })
        }
        _ => None,
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The point standing in for the pointer: the remote API's pin, else the
/// pointer on the map.
pub fn hover_point(tools: &ToolState, cursor: &crate::camera::Cursor) -> Option<SVec2> {
    tools.cursor_pin.or(cursor.ground)
}

/// The site shown this frame, for the label.
#[derive(Resource, Default)]
pub struct PlotHover {
    pub site: Option<HoverSite>,
    pub at: Option<SVec2>,
}

const GREEN: [f32; 3] = [0.5, 6.0, 1.5];
const RED: [f32; 3] = [7.0, 0.45, 0.55];

/// The wall's height and the outline's width for a camera this far out,
/// in steps so a zoom rebuilds the mesh only now and then.
fn sizes(distance: f32, zone: bool) -> (f32, f32) {
    let h = ((distance * 0.02).clamp(6.0, 12.0) * 2.0).round() * 0.5;
    let lw = ((distance * 0.005).clamp(0.8, 3.5) * 4.0).round() * 0.25;
    if zone { (h * 0.6, lw * 0.8) } else { (h, lw) }
}

/// The highlight: a faint fill, a bright outline on the ground and a wall
/// rising from it, fading to nothing at `height` (additive, so the alpha
/// is the brightness).
fn highlight_mesh(w: &World, site: &HoverSite, height: f32, line_w: f32) -> MeshData {
    let mut m = MeshData::default();
    let poly = &site.outline;
    if poly.len() < 3 {
        return m;
    }
    let c = if site.verdict.is_ok() { GREEN } else { RED };
    let col = |a: f32| [c[0], c[1], c[2], a];
    let y = |p: SVec2| w.ground_height(p) + 0.3;
    let centre = poly.iter().copied().sum::<SVec2>() / poly.len() as f32;
    m.ground_polygon(poly, poly.iter().map(|&p| y(p)).fold(y(centre), f32::max) + 0.02, col(0.08));
    let n = poly.len();
    let push = |m: &mut MeshData, p: Vec3, nrm: Vec3, color: [f32; 4]| {
        m.positions.push(p.to_array());
        m.normals.push(nrm.to_array());
        m.colors.push(color);
        m.uvs.push([0.0, 0.0]);
        m.tangents.push([1.0, 0.0, 0.0, 1.0]);
    };
    // Rows up the wall: (fraction of the height, alpha). Bright at the
    // foot, mostly gone a third of the way up, nothing at the top.
    const ROWS: [(f32, f32); 5] = [(0.0, 1.0), (0.1, 0.7), (0.35, 0.32), (0.7, 0.1), (1.0, 0.0)];
    for k in 0..n {
        let (a, b) = (poly[k], poly[(k + 1) % n]);
        let d = (b - a).normalize_or_zero();
        if d == SVec2::ZERO {
            continue;
        }
        let side = SVec2::new(-d.y, d.x);
        let nrm = Vec3::new(side.x, 0.0, side.y);
        let (ya, yb) = (y(a), y(b));
        // The wall.
        let base = m.positions.len() as u32;
        for (t, alpha) in ROWS {
            push(&mut m, Vec3::new(a.x, ya + t * height, a.y), nrm, col(alpha));
            push(&mut m, Vec3::new(b.x, yb + t * height, b.y), nrm, col(alpha));
        }
        for r in 0..ROWS.len() as u32 - 1 {
            let (i0, i1, i2, i3) = (base + r * 2, base + r * 2 + 1, base + r * 2 + 3, base + r * 2 + 2);
            m.indices.extend_from_slice(&[i0, i1, i2, i0, i2, i3]);
        }
        // The outline: a flat bright band, run on past each corner by half
        // its width so the corners close.
        let (a2, b2) = (a - d * line_w * 0.5, b + d * line_w * 0.5);
        let o = side * line_w * 0.5;
        let base = m.positions.len() as u32;
        let hot = [c[0] * 1.5 + 2.0, c[1] * 1.5 + 2.0, c[2] * 1.5 + 2.0, 1.0];
        for p in [a2 + o, b2 + o, b2 - o, a2 - o] {
            let h = if p.distance_squared(a2) < p.distance_squared(b2) { ya } else { yb };
            push(&mut m, Vec3::new(p.x, h + 0.08, p.y), Vec3::Y, hot);
        }
        m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    m
}

/// Keep the highlight in step with the pointer and the verdict.
#[allow(clippy::too_many_arguments)]
pub fn sync_plot_hover(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    mut tools: ResMut<ToolState>,
    cursor: Res<crate::camera::Cursor>,
    cams: Query<&crate::camera::OrbitCamera>,
    time: Res<Time>,
    mut hover: ResMut<PlotHover>,
    mut state: Local<(Option<u64>, Option<Handle<StandardMaterial>>, Option<SVec2>)>,
    existing: Query<Entity, With<PlotHoverTag>>,
) {
    let (last, mat, last_ground) = &mut *state;
    // The real pointer moving over the map takes over from a remote pin.
    if matches!(tools.tool, Tool::Buildings | Tool::Zone) && tools.cursor_pin.is_some() && cursor.ground.is_some() && last_ground.is_some() && cursor.ground != *last_ground {
        tools.cursor_pin = None;
    }
    *last_ground = cursor.ground;
    let w = &sim.world;
    let at = hover_point(&tools, &cursor);
    let site = at.and_then(|p| hover_site(w, &tools, p));
    let distance = cams.iter().next().map_or(400.0, |c| c.distance);
    let key = site.as_ref().map(|s| {
        let (h, lw) = sizes(distance, s.zone);
        let mut hs = DefaultHasher::new();
        hash_f32s(&mut hs, s.outline.iter().copied());
        (format!("{:?}", s.verdict), s.zone, h.to_bits(), lw.to_bits(), w.relief).hash(&mut hs);
        hs.finish()
    });
    let mat = mat.get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, alpha_mode: AlphaMode::Add, cull_mode: None, fog_enabled: false, ..default() })).clone();
    if site.is_some() {
        // A soft pulse, a little quicker on a refusal.
        let speed = if site.as_ref().is_some_and(|s| s.verdict.is_err()) { 5.0 } else { 3.0 };
        if let Some(mut m) = materials.get_mut(&mat) {
            m.base_color = Color::linear_rgba(1.0, 1.0, 1.0, 0.8 + 0.2 * (time.elapsed_secs() * speed).sin());
        }
    }
    hover.at = at;
    if key == *last && !sim.full_rebuild {
        hover.site = site;
        return;
    }
    *last = key;
    despawn_all(&mut commands, &existing);
    if let Some(s) = &site {
        let (h, lw) = sizes(distance, s.zone);
        let data = highlight_mesh(w, s, h, lw);
        if !data.is_empty() {
            commands.spawn((Mesh3d(meshes.add(to_mesh(&data, false))), MeshMaterial3d(mat), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, PlotHoverTag));
        }
    }
    hover.site = site;
}

/// The verdict beside the pointer: what a click would do, or why not.
pub fn plot_hover_label(mut contexts: bevy_egui::EguiContexts, hover: Res<PlotHover>, sim: Res<Sim>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>) -> Result {
    use bevy_egui::egui;
    let (Some(site), Some(at)) = (&hover.site, hover.at) else { return Ok(()) };
    let (text, bg) = match &site.verdict {
        Ok(_) if site.zone => return Ok(()),
        Ok(note) => (note.clone(), egui::Color32::from_rgba_unmultiplied(18, 110, 52, 225)),
        Err(why) => (format!("Can't build: {why}"), egui::Color32::from_rgba_unmultiplied(150, 20, 28, 230)),
    };
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    let Ok(v) = camera.world_to_viewport(gt, Vec3::new(at.x, sim.world.ground_height(at), at.y)) else { return Ok(()) };
    let v = crate::ui::theme::to_points(ctx, v);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("plot_hover_label")));
    let galley = painter.layout(text, crate::ui::theme::small(), egui::Color32::WHITE, 440.0);
    let pos = egui::pos2(v.x + 18.0, v.y + 16.0);
    let r = egui::Rect::from_min_size(pos, galley.size()).expand2(egui::vec2(7.0, 4.0));
    painter.rect_filled(r, 4.0, bg);
    painter.galley(pos, galley, egui::Color32::WHITE);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walls_grow_with_distance_in_steps_and_stay_in_range() {
        let (near, _) = sizes(50.0, false);
        let (far, _) = sizes(5000.0, false);
        assert_eq!(near, 6.0);
        assert_eq!(far, 12.0);
        // Small zooms land on the same step: no rebuild.
        assert_eq!(sizes(400.0, false), sizes(410.0, false));
        assert!(sizes(500.0, true).0 < sizes(500.0, false).0);
    }

    #[test]
    fn the_wall_fades_to_nothing_and_takes_the_verdicts_colour() {
        let w = World::new(nd_sim::WorldConfig { emergencies: false, society: false, landscape: nd_sim::LandscapeParams::flat(), ..nd_sim::WorldConfig::default() });
        let sq = vec![SVec2::new(0.0, 0.0), SVec2::new(20.0, 0.0), SVec2::new(20.0, 20.0), SVec2::new(0.0, 20.0)];
        let ok = HoverSite { outline: sq.clone(), verdict: Ok("fits".into()), zone: false };
        let no = HoverSite { outline: sq, verdict: Err("no".into()), zone: false };
        let m = highlight_mesh(&w, &ok, 8.0, 1.0);
        let top = m.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!(top >= 8.0);
        // Every vertex at the top is fully faded.
        for (p, c) in m.positions.iter().zip(&m.colors) {
            if (p[1] - top).abs() < 1e-3 {
                assert_eq!(c[3], 0.0);
            }
        }
        assert!(m.colors.iter().all(|c| c[1] >= c[0]));
        let r = highlight_mesh(&w, &no, 8.0, 1.0);
        assert!(r.colors.iter().all(|c| c[0] >= c[1]));
    }
}
