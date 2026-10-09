//! The New Game window: pick a landscape preset and a climate, adjust its
//! levers (water, height variation, river branching, islands, seed, the starting highway)
//! and watch a mini-map of the result before starting over on it. A
//! starting town may be picked too: a whole example city generated on the
//! landscape (`nd_procgen::citygen`), drawn on the mini-map.

use bevy_egui::egui;
use nd_procgen::citygen::CityPreset;
use nd_sim::landscape::{Climate, Landscape, LandscapeParams, LandscapePreset, LAND_HALF};
use nd_sim::Vec2 as SVec2;

use super::{hint, section, UiCtx, GOLD, WARN};

const PREVIEW: usize = 168;

fn open_id() -> egui::Id {
    egui::Id::new("new_game_open")
}

pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

pub fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// The top-bar toggle.
pub fn toggle(ui: &mut egui::Ui) {
    let open = is_open(ui.ctx());
    if ui.selectable_label(open, "New game…").on_hover_text("Start a new city on a landscape of your choosing").clicked() {
        set_open(ui.ctx(), !open);
    }
}

/// The levers being edited and the mini-map drawn for them.
#[derive(Clone)]
struct Draft {
    params: LandscapeParams,
    /// The starting town to generate, or an empty map.
    city: Option<CityPreset>,
    drawn: Option<(LandscapeParams, Option<CityPreset>, egui::TextureHandle, String)>,
    confirm: bool,
    /// [identity] The new city's name.
    name: String,
}

/// [identity] A name to offer for a new city, different each time.
pub fn suggest_name() -> String {
    use rand::SeedableRng;
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    nd_sim::names::city_name(&mut rand::rngs::StdRng::seed_from_u64(seed))
}

pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    if !is_open(ctx) {
        return;
    }
    let id = egui::Id::new("new_game_draft");
    let mut draft: Draft = ctx.data(|d| d.get_temp(id)).unwrap_or_else(|| Draft { params: p.sim.world.landscape.params.clone(), city: None, drawn: None, confirm: false, name: suggest_name() });
    let mut open = true;
    egui::Window::new(egui::RichText::new("New game").color(GOLD))
        .open(&mut open)
        .constrain_to(content)
        .default_pos(content.center() - egui::vec2(220.0, 200.0))
        .resizable(false)
        .show(ctx, |ui| body(ui, p, &mut draft));
    if !open {
        set_open(ctx, false);
        draft.confirm = false;
    }
    ctx.data_mut(|d| d.insert_temp(id, draft));
}

fn body(ui: &mut egui::Ui, p: &mut UiCtx, draft: &mut Draft) {
    section(ui, "CITY NAME");
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(220.0).hint_text("name your city"));
        if ui.small_button("🎲").on_hover_text("another name").clicked() {
            draft.name = suggest_name();
        }
    });
    hint(ui, "Saves are listed under this name; it can be changed later in the City or Saves window.");
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(250.0);
            section(ui, "LANDSCAPE");
            for preset in LandscapePreset::ALL {
                let on = draft.params.preset == preset;
                if ui.selectable_label(on, preset.label()).on_hover_text(preset.blurb()).clicked() && !on {
                    // A preset sets its own levers; the seed and the climate stay.
                    draft.params = LandscapeParams { climate: draft.params.climate, ..LandscapeParams::preset(preset, draft.params.seed) };
                }
            }
            hint(ui, draft.params.preset.blurb());
            section(ui, "CLIMATE");
            ui.horizontal_wrapped(|ui| {
                for climate in Climate::ALL {
                    let on = draft.params.climate == climate;
                    if ui.selectable_label(on, climate.label()).on_hover_text(climate.blurb()).clicked() {
                        draft.params.climate = climate;
                    }
                }
            });
            hint(ui, draft.params.climate.blurb());
            if draft.params.preset != LandscapePreset::Flat {
                section(ui, "LEVERS");
                ui.add(egui::Slider::new(&mut draft.params.water, 0.0..=1.0).text("water"));
                ui.add(egui::Slider::new(&mut draft.params.relief, 0.0..=1.0).text("height variation"));
                ui.add(egui::Slider::new(&mut draft.params.branching, 0.0..=1.0).text("river branching"));
                if matches!(draft.params.preset, LandscapePreset::Islands | LandscapePreset::Coast | LandscapePreset::Delta) {
                    ui.add(egui::Slider::new(&mut draft.params.islands, 0..=8).text("islands"));
                }
            }
            ui.horizontal(|ui| {
                ui.label("seed");
                let mut s = draft.params.seed as i64;
                if ui.add(egui::DragValue::new(&mut s).range(0..=999_999)).changed() {
                    draft.params.seed = s.max(0) as u64;
                }
                if ui.small_button("🎲").on_hover_text("another seed").clicked() {
                    draft.params.seed = (draft.params.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) >> 20) % 1_000_000;
                }
            });
            section(ui, "STARTING TOWN");
            if ui.selectable_label(draft.city.is_none(), "Empty map").on_hover_text("Nothing but the highway and the trunk line").clicked() {
                draft.city = None;
            }
            for city in CityPreset::ALL {
                let on = draft.city == Some(city);
                if ui.selectable_label(on, city.label()).on_hover_text(city.blurb()).clicked() && !on {
                    // A town comes with the landscape it was made for; the
                    // levers may still be changed after.
                    draft.city = Some(city);
                    draft.params = if city.furnished() {
                        // [perfect-city] The planned city comes with its own
                        // site and climate (the seed may still be changed).
                        city.landscape(city.default_seed())
                    } else {
                        LandscapeParams { climate: draft.params.climate, ..city.landscape(draft.params.seed) }
                    };
                }
            }
            match draft.city {
                Some(city) if city.furnished() => hint(ui, &format!("{} Streets, zoning, diamond interchanges, power, police, fire, health, schools, a university, metro, buses, rail, docks, districts and 50 million in the treasury; population 0. Laying it all out takes about half a minute (the window waits).", city.blurb())),
                Some(city) => hint(ui, &format!("{} Streets, zoning, interchanges and a power plant are laid out; it grows from there.", city.blurb())),
                None => hint(ui, "Every map starts with the highway and the trunk line, edge to edge; the rest is yours to build."),
            }
        });
        ui.vertical(|ui| {
            refresh_preview(ui.ctx(), draft);
            if let Some((_, _, tex, summary)) = &draft.drawn {
                ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(PREVIEW as f32 * 1.4, PREVIEW as f32 * 1.4)));
                hint(ui, summary);
            }
        });
    });
    ui.separator();
    if draft.confirm {
        ui.colored_label(WARN, "This throws the current city away.");
        ui.horizontal(|ui| {
            if ui.button("Start the new city").clicked() {
                start(p, draft.params.clone(), draft.city, &draft.name);
                draft.name = suggest_name();
                draft.confirm = false;
                set_open(ui.ctx(), false);
            }
            if ui.button("Cancel").clicked() {
                draft.confirm = false;
            }
        });
    } else if ui.button(egui::RichText::new("Start").strong()).clicked() {
        if p.sim.world.buildings.is_empty() && p.sim.world.citizens.is_empty() {
            start(p, draft.params.clone(), draft.city, &draft.name);
            draft.name = suggest_name();
            set_open(ui.ctx(), false);
        } else {
            draft.confirm = true;
        }
    }
}

fn start(p: &mut UiCtx, params: LandscapeParams, city: Option<CityPreset>, name: &str) {
    let label = format!("{} {}", params.climate.label().to_lowercase(), params.preset.label().to_lowercase());
    match city {
        Some(c) => {
            p.sim.generate_city(c, params);
        }
        None => p.sim.new_game(params),
    }
    p.sim.set_city_name(name);
    let named = p.sim.world.identity.display_name().to_string();
    p.flags.message = match city {
        Some(c) => format!("New city {named}: {} on a {label} landscape", c.label()),
        None => format!("New city {named}: {label} landscape"),
    };
}

/// Draw the mini-map for the current levers when they changed: water blue,
/// land shaded by height, the highway and the railway, hot spots in yellow.
fn refresh_preview(ctx: &egui::Context, draft: &mut Draft) {
    if draft.drawn.as_ref().is_some_and(|(p, c, ..)| *p == draft.params && *c == draft.city) {
        return;
    }
    let (mut image, summary) = preview_image(&draft.params);
    if let Some(city) = draft.city {
        draw_city(&mut image, city, &draft.params);
    }
    let tex = ctx.load_texture("new_game_preview", image, egui::TextureOptions::LINEAR);
    draft.drawn = Some((draft.params.clone(), draft.city, tex, summary));
}

/// The generated town's streets over the mini-map.
fn draw_city(image: &mut egui::ColorImage, city: CityPreset, params: &LandscapeParams) {
    let land = Landscape::new(params);
    let plan = nd_procgen::citygen::generate(city, params.seed, &land);
    let span = LAND_HALF * 2.0;
    for r in &plan.roads {
        let c = match r.class {
            nd_procgen::citygen::RoadClass::Arterial => egui::Color32::from_rgb(250, 225, 150),
            _ => egui::Color32::from_rgb(215, 215, 205),
        };
        for w in r.points.windows(2) {
            let steps = (w[0].distance(w[1]) / 6.0).ceil().max(1.0) as usize;
            for k in 0..=steps {
                let q = w[0].lerp(w[1], k as f32 / steps as f32);
                let i = ((q.x + LAND_HALF) / span * PREVIEW as f32) as usize;
                let j = ((LAND_HALF - q.y) / span * PREVIEW as f32) as usize;
                if i < PREVIEW && j < PREVIEW {
                    image.pixels[j * PREVIEW + i] = c;
                }
            }
        }
    }
}

/// The mini-map for a set of levers, and a one-line summary of the map.
pub fn preview_image(params: &LandscapeParams) -> (egui::ColorImage, String) {
    let land = Landscape::new(params);
    let mut pixels = Vec::with_capacity(PREVIEW * PREVIEW);
    let span = LAND_HALF * 2.0;
    for j in 0..PREVIEW {
        for i in 0..PREVIEW {
            // North (+y) at the top.
            let p = SVec2::new(-LAND_HALF + (i as f32 + 0.5) / PREVIEW as f32 * span, LAND_HALF - (j as f32 + 0.5) / PREVIEW as f32 * span);
            let c = if let Some(hit) = land.water_at(p) {
                match hit.kind {
                    // Frozen lakes and the sea's edge in the Arctic.
                    nd_sim::WaterKind::Lake if params.climate == Climate::Arctic => [190, 210, 225],
                    nd_sim::WaterKind::Sea if params.climate == Climate::Arctic && land.water_distance(p) > -70.0 => [190, 210, 225],
                    nd_sim::WaterKind::Sea | nd_sim::WaterKind::Lake if params.climate == Climate::Tropical && land.water_distance(p) > -60.0 => [40, 170, 170],
                    nd_sim::WaterKind::Stream => [70, 140, 190],
                    nd_sim::WaterKind::River => [40, 105, 170],
                    nd_sim::WaterKind::Lake => [45, 110, 160],
                    nd_sim::WaterKind::Sea => [25, 70, 130],
                }
            } else if land.hot_spot_at(p).is_some_and(|h| h.pos.distance(p) < h.radius) {
                [220, 190, 60]
            } else if params.highway && ((p.y - nd_sim::landscape::HIGHWAY_Y).abs() < 14.0) {
                [235, 180, 90]
            } else if (p.y - nd_sim::rail::TRUNK_Y).abs() < 8.0 {
                [150, 150, 160]
            } else {
                let h = land.base_height(p);
                let t = (h / 60.0).clamp(0.0, 1.0);
                let basin = if p.length() < 900.0 { 1.0 } else { 0.85 };
                // The climate's ground: from dark rock to snow.
                let (lo, hi) = match params.climate {
                    Climate::Desert => ([120.0, 105.0, 70.0], [175.0, 160.0, 120.0]),
                    Climate::Temperate => ([70.0, 95.0, 55.0], [160.0, 165.0, 115.0]),
                    Climate::Tropical => ([40.0, 120.0, 45.0], [110.0, 175.0, 90.0]),
                    Climate::Arctic => ([200.0, 208.0, 220.0], [240.0, 244.0, 250.0]),
                    Climate::Volcanic => ([45.0, 50.0, 48.0], [95.0, 110.0, 85.0]),
                };
                let g = [(lo[0] + (hi[0] - lo[0]) * t) * basin, (lo[1] + (hi[1] - lo[1]) * t) * basin, (lo[2] + (hi[2] - lo[2]) * t) * basin];
                [g[0] as u8, g[1] as u8, g[2] as u8]
            };
            pixels.push(egui::Color32::from_rgb(c[0], c[1], c[2]));
        }
    }
    let streams = land.rivers.iter().filter(|r| r.stream).count();
    let rivers = land.rivers.len() - streams;
    let islands = land.sea.as_ref().map_or(0, |s| s.islands.len());
    let mut summary = format!("{rivers} river{}, {streams} stream{}, {} lake{}", if rivers == 1 { "" } else { "s" }, if streams == 1 { "" } else { "s" }, land.lakes.len(), if land.lakes.len() == 1 { "" } else { "s" });
    if land.sea.is_some() {
        summary.push_str(&format!(", sea, {islands} island{}", if islands == 1 { "" } else { "s" }));
    }
    summary.push_str(&format!(", {} hot spot{}", land.hot_spots.len(), if land.hot_spots.len() == 1 { "" } else { "s" }));
    if let Some(r) = land.rivers.first() {
        summary.push_str(&format!(". Main river {:.0} m wide, {:.1} m/s.", r.width, r.current));
    }
    (egui::ColorImage { size: [PREVIEW, PREVIEW], source_size: egui::vec2(PREVIEW as f32, PREVIEW as f32), pixels }, summary)
}
