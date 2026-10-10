//! [plot-hover] The plot under the pointer while placing: a crisp outline
//! on the ground and a wall of light rising from it, fading out with
//! height, green where a click would build and red where it would be
//! refused, with the sim's verdict as a label beside the pointer.
//!
//! Used by the Buildings tool (anything that goes on a lot), the Zone
//! tool's brush and marquee (the parcel being painted) and the Bulldoze
//! tool's ground clean-up (the circle dug out, its cost and days, from
//! `World::cleanup_quote`). The verdict and
//! the outline come from one adapter, [`hover_site`], which asks the sim
//! (`World::placement_site` / `site_problem`, as the click and the Build
//! window's "Here:" line do, and `World::parcel_refusal`) and never applies
//! rules of its own. The mesh is rebuilt only when the site, its verdict or
//! the wall's size step changes; the pulse is the material's alpha.
//!
//! The remote API stands in for the pointer with `tool {cursor: [x, y]}`
//! (also `hover`): it holds until the real pointer moves over the map.

use super::*;
use crate::overlay::mesh::{MeshOverlay, OverlayCx, OverlayMat, Pieces, Wanted};
use crate::overlay::shapes::push_vertex;
use bevy::ecs::system::SystemParamItem;
use crate::tools::{Tool, ToolState, ZoneMode};

#[derive(Component, Default)]
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
    /// [dirty-industry] A warning on a site that would go ahead: homes or
    /// farms painted next to dirty industry (shown even for a zone).
    pub warning: Option<String>,
    /// [dirty-industry] A ground clean-up, not a building: the label says
    /// "Can't clean up" on a refusal.
    pub cleanup: bool,
}

/// [dirty-industry] The Bulldoze tool's clean-up at `p`: a circle of the
/// tool's radius and `World::cleanup_quote`'s verdict (what the click does).
fn cleanup_site(w: &World, tools: &ToolState, p: SVec2) -> HoverSite {
    let r = tools.cleanup_radius.clamp(nd_sim::world::CLEANUP_MIN_RADIUS, nd_sim::world::CLEANUP_MAX_RADIUS);
    let n = 48;
    let outline: Vec<SVec2> = (0..n).map(|k| p + SVec2::from_angle(k as f32 / n as f32 * std::f32::consts::TAU) * r).collect();
    let verdict = w.cleanup_quote(p, r).map(|q| {
        format!(
            "Clean up {:.0} m round: {} stained cell{} ({:.1} cells of full contamination)
costs {} ({} per cell), a crew digs for {} days",
            q.radius,
            q.cells,
            if q.cells == 1 { "" } else { "s" },
            q.amount,
            thousands(q.cost),
            thousands(nd_sim::world::CLEANUP_COST),
            q.days
        )
    });
    HoverSite { outline, verdict, zone: false, warning: None, cleanup: true }
}

/// [dirty-industry] What a homes, shops or farm parcel at `p` would suffer
/// from the dirty plants around, as a warning for the hover label.
fn dirt_warning(w: &World, p: SVec2, lu: nd_sim::LandUse) -> Option<String> {
    use nd_sim::industry::{IndustryKind, Neighbour};
    if w.dirty.is_empty() {
        return None;
    }
    let who = match lu {
        nd_sim::LandUse::Industrial => Neighbour::Industry(IndustryKind::Farming),
        other => Neighbour::Land(other),
    };
    let e = w.dirt_exposure_at(p, who, None);
    if e < 0.1 {
        return None;
    }
    let worst = w.dirty.iter().filter(|s| nd_sim::industry::sensitivity(who, s.as_kind) > 0.0 && s.pos.distance(p) < s.reach).min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p)))?;
    let name = w.buildings.get(worst.building).map_or("dirty industry".to_string(), |b| b.name.clone());
    let d = worst.pos.distance(p);
    Some(match lu {
        nd_sim::LandUse::Industrial => format!("Next to {name} ({d:.0} m): no farm will open here (food output −{:.0}%)", 100.0 * nd_sim::world::ADJACENCY_OUTPUT_HIT * e),
        nd_sim::LandUse::Residential => format!("Next to {name} ({d:.0} m): homes here lose value (−{:.2}) and their people fall ill", nd_sim::world::ADJACENCY_VALUE_HIT * e),
        _ => format!("Next to {name} ({d:.0} m): land value −{:.2}, slow to build", nd_sim::world::ADJACENCY_VALUE_HIT * e),
    })
}

/// What the pointer at `p` is over for the active tool, and the sim's
/// verdict on it. None where the tool has no plot to show (roads, tolls,
/// rail pieces and docks have their own previews; erasing zones).
///
/// The Buildings tool's verdict is `World::placement_site(what, p)` (any
/// lot, or unzoned ground a lot is made of) checked by `site_problem`, the
/// same as the click; the site kept in `ToolState::build_site` while the
/// pointer stays on its parcel is reused rather than worked out again.
pub fn hover_site(w: &World, tools: &ToolState, p: SVec2) -> Option<HoverSite> {
    match tools.tool {
        Tool::Buildings => {
            let what = tools.build_item();
            if !on_a_plot(what) {
                return None;
            }
            let key = w.parcels.at(p).map(|i| (what, i, w.parcels.version, w.lots.len()));
            let site = match (&tools.build_site, key) {
                (Some((a, b, c, d, site)), Some(k)) if (*a, *b, *c, *d) == k => site.clone(),
                _ => w.placement_site(what, p),
            };
            let (ok, text, outline) = match site {
                Ok(site) => {
                    let (ok, text) = site_verdict(w, what, &site);
                    (ok, text, site.outline)
                }
                Err(why) => (false, why, Vec::new()),
            };
            buildings_site(w, tools, p, ok, text, outline)
        }
        Tool::Bulldoze if tools.cleanup => Some(cleanup_site(w, tools, p)),
        Tool::Zone if tools.zone_mode != ZoneMode::Polygon => {
            let lu = tools.paint_use(tools.stroke.as_ref().map_or(tools.unzone, |s| s.erase))?;
            // [island-lots] No parcel under the pointer: say why at the
            // cursor (water, too far from a road, past the buildable land)
            // rather than show nothing, as a click there does nothing.
            let Some(i) = w.parcels.at(p) else {
                let why = w.no_parcel_why(p)?;
                return Some(HoverSite { outline: Vec::new(), verdict: Err(why.to_string()), zone: true, warning: None, cleanup: false });
            };
            let parcel = w.parcels.parcels.get(i)?;
            let verdict = match w.parcel_refusal(i, Some(lu)) {
                Some(why) => Err(why.to_string()),
                None => Ok(format!("{} zoning", lu.label())),
            };
            let warning = if verdict.is_ok() { dirt_warning(w, nd_sim::geom::polygon_centroid(&parcel.polygon), lu) } else { None };
            Some(HoverSite { outline: parcel.polygon.clone(), verdict, zone: true, warning, cleanup: false })
        }
        _ => None,
    }
}

/// [plant-scale] What a click on `site` would do, as the hover label says
/// it: `(true, what it would be)` or `(false, why not)`. A plant shows its
/// nameplate there, what it costs to build and to run and, for solar and
/// wind, the Sunfield or Wind Park it would join and what the joined
/// installation makes (`World::plant_preview_site`: the figures the click
/// charges, on an existing lot or the one made of unzoned ground);
/// anything else its cost and upkeep. The ground note follows.
pub fn site_verdict(w: &World, what: nd_sim::Placeable, site: &nd_sim::PlacementSite) -> (bool, String) {
    if let Some(why) = w.site_problem(what, site) {
        return (false, why);
    }
    let figures = match what {
        nd_sim::Placeable::Plant(k) => match w.plant_preview_site(k, site) {
            Some(p) => plant_figures(&p),
            None => String::new(),
        },
        _ => format!("costs {}, {}/day", thousands(what.cost()), thousands(what.upkeep())),
    };
    (true, if figures.is_empty() { site.note() } else { format!("{figures}\n{}", site.note()) })
}

/// [plant-scale] A plant preview in a line or two: "130 MW, costs 21,600,
/// 1,170/day" and "joins Saffron Sunfield: 3 lots, 124 MW together (+38 MW)".
pub fn plant_figures(p: &nd_sim::PlantPreview) -> String {
    let site = if p.site > 0.0 && (p.site - 1.0).abs() > 0.01 { p.site } else { 1.0 };
    let mut s = format!("{} MW, costs {}, {}/day", mw(p.capacity_mw * site), thousands(p.cost), thousands(p.upkeep_per_day));
    if site != 1.0 {
        s.push_str(&format!(" (site {:.0}%)", site * 100.0));
    }
    if let Some(j) = &p.join {
        let name = j.joins.clone().unwrap_or_default();
        s.push_str(&format!("\njoins {name}: {} lots, {} MW together (+{} MW)", j.members.len() + 1, mw(j.capacity_mw), mw(j.capacity_mw - j.before_mw)));
    }
    s
}

/// Megawatts to a tenth below ten, whole above.
fn mw(x: f32) -> String {
    if x.abs() < 10.0 { format!("{x:.1}") } else { format!("{x:.0}") }
}

/// A whole number with thousands separators: 21,600.
pub fn thousands(x: f32) -> String {
    let n = x.round() as i64;
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Whether the Buildings tool's item goes on a plot (tolls, rail pieces
/// and docks have previews of their own).
fn on_a_plot(what: nd_sim::Placeable) -> bool {
    use nd_sim::Placeable;
    !matches!(what, Placeable::Toll(_) | Placeable::Rail(_) | Placeable::Dock(_))
}

/// The Buildings tool's site from its verdict (`ToolState::build_here`'s
/// shape): a refusal with no site outlines the parcel under the point, if
/// any, so the red still shows where the pointer is.
fn buildings_site(w: &World, tools: &ToolState, p: SVec2, ok: bool, text: String, outline: Vec<SVec2>) -> Option<HoverSite> {
    let outline = if outline.len() >= 3 { outline } else { w.parcels.parcels.get(w.parcels.at(p)?)?.polygon.clone() };
    let label = capitalised(&tools.build_item().label());
    let verdict = if ok { Ok(format!("{label} can go here: {text}")) } else { Err(text) };
    Some(HoverSite { outline, verdict, zone: false, warning: None, cleanup: false })
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
            push_vertex(&mut m, Vec3::new(a.x, ya + t * height, a.y), nrm, col(alpha));
            push_vertex(&mut m, Vec3::new(b.x, yb + t * height, b.y), nrm, col(alpha));
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
            push_vertex(&mut m, Vec3::new(p.x, h + 0.08, p.y), Vec3::Y, hot);
        }
        m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    m
}

/// [overlay] The highlight as a mesh overlay, kept in step with the
/// pointer and the verdict: rebuilt only when the site, its verdict or the
/// wall's size step changes; the pulse is the material's alpha.
pub struct PlotHoverMesh;

impl MeshOverlay for PlotHoverMesh {
    type Key = u64;
    type Param = (
        ResMut<'static, ToolState>,
        Res<'static, crate::camera::Cursor>,
        Query<'static, 'static, &'static crate::camera::OrbitCamera>,
        Res<'static, Time>,
        Res<'static, Lighting>,
        ResMut<'static, PlotHover>,
        Local<'static, Option<SVec2>>,
    );
    type Marker = PlotHoverTag;

    fn key((tools, cursor, cams, time, lighting, hover, last_ground): &mut SystemParamItem<Self::Param>, cx: &mut OverlayCx) -> Wanted<u64> {
        // The real pointer moving over the map takes over from a remote pin.
        if matches!(tools.tool, Tool::Buildings | Tool::Zone | Tool::Bulldoze | Tool::Edit) && tools.cursor_pin.is_some() && cursor.ground.is_some() && last_ground.is_some() && cursor.ground != **last_ground {
            tools.cursor_pin = None;
        }
        **last_ground = cursor.ground;
        let w = &cx.sim.world;
        let at = hover_point(tools, cursor);
        // The Buildings tool's verdict was worked out this frame in
        // `tool_input` (`build_here`, from the same point): not again here.
        let site = at.and_then(|p| match (&tools.build_here, tools.tool) {
            (Some((ok, text, outline)), Tool::Buildings) if on_a_plot(tools.build_item()) => buildings_site(w, tools, p, *ok, text.clone(), outline.clone()),
            _ => hover_site(w, tools, p),
        });
        let distance = cams.iter().next().map_or(400.0, |c| c.distance);
        let key = site.as_ref().map(|s| {
            let (h, lw) = sizes(distance, s.zone);
            let mut hs = DefaultHasher::new();
            hash_f32s(&mut hs, s.outline.iter().copied());
            (format!("{:?}", s.verdict), s.zone, h.to_bits(), lw.to_bits(), w.relief).hash(&mut hs);
            hs.finish()
        });
        let mat = cx.mat(OverlayMat::PlotHover);
        if site.is_some() {
            // A soft pulse, a little quicker on a refusal.
            let speed = if site.as_ref().is_some_and(|s| s.verdict.is_err()) { 5.0 } else { 3.0 };
            if let Some(mut m) = cx.materials.get_mut(&mat) {
                // Dimmer at night, when the glow has nothing to compete with.
                let t = if lighting.follow_clock { w.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
                let daylight = smoothstep(-0.12, 0.30, ((t - 0.25) * std::f32::consts::TAU).sin());
                let k = 0.45 + 0.55 * daylight;
                m.base_color = Color::linear_rgba(k, k, k, 0.8 + 0.2 * (time.elapsed_secs() * speed).sin());
            }
        }
        hover.at = at;
        hover.site = site;
        match key {
            Some(k) => Wanted::Show(k),
            None => Wanted::Nothing,
        }
    }

    fn build((_, _, cams, _, _, hover, _): &mut SystemParamItem<Self::Param>, cx: &mut OverlayCx, _: &u64, out: &mut Pieces) {
        let Some(s) = &hover.site else { return };
        let distance = cams.iter().next().map_or(400.0, |c| c.distance);
        let (h, lw) = sizes(distance, s.zone);
        out.add(highlight_mesh(&cx.sim.world, s, h, lw), cx.mat(OverlayMat::PlotHover));
    }
}

/// The verdict beside the pointer: what a click would do, or why not.
pub fn plot_hover_label(mut contexts: bevy_egui::EguiContexts, hover: Res<PlotHover>, sim: Res<Sim>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>) -> Result {
    use bevy_egui::egui;
    let (Some(site), Some(at)) = (&hover.site, hover.at) else { return Ok(()) };
    let (text, bg) = match &site.verdict {
        Ok(_) if site.warning.is_some() => (site.warning.clone().unwrap_or_default(), egui::Color32::from_rgba_unmultiplied(150, 100, 10, 230)),
        Ok(_) if site.zone => return Ok(()),
        Ok(note) => (note.clone(), egui::Color32::from_rgba_unmultiplied(18, 110, 52, 225)),
        Err(why) if site.cleanup => (format!("Can't clean up: {why}"), egui::Color32::from_rgba_unmultiplied(150, 20, 28, 230)),
        Err(why) if site.zone => (format!("Can't zone here: {why}"), egui::Color32::from_rgba_unmultiplied(150, 20, 28, 230)),
        Err(why) => (format!("Can't build: {why}"), egui::Color32::from_rgba_unmultiplied(150, 20, 28, 230)),
    };
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    // [city-data] The map is covered: nothing to label.
    if crate::ui::city_data::is_open(ctx) {
        return Ok(());
    }
    let Ok(v) = camera.world_to_viewport(gt, Vec3::new(at.x, sim.world.ground_height(at), at.y)) else { return Ok(()) };
    let v = crate::ui::theme::to_points(ctx, v);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("plot_hover_label")));
    let galley = painter.layout(text, crate::ui::theme::small(), egui::Color32::WHITE, 440.0);
    // Beside the pointer, flipped to its left (or above it) where it would
    // run off the screen, so a long reason is never cut off.
    let screen = ctx.content_rect();
    let size = galley.size();
    let x = if v.x + 18.0 + size.x + 8.0 > screen.right() { v.x - 18.0 - size.x } else { v.x + 18.0 };
    let y = if v.y + 16.0 + size.y + 6.0 > screen.bottom() { v.y - 16.0 - size.y } else { v.y + 16.0 };
    let pos = egui::pos2(x.max(screen.left() + 8.0), y.max(screen.top() + 6.0));
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
        let ok = HoverSite { outline: sq.clone(), verdict: Ok("fits".into()), zone: false, warning: None, cleanup: false };
        let no = HoverSite { outline: sq, verdict: Err("no".into()), zone: false, warning: None, cleanup: false };
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
