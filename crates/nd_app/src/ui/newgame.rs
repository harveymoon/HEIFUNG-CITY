//! The New Game window: pick a landscape preset and a climate, adjust its
//! levers (water, height variation, river branching, islands, seed, the starting highway)
//! and watch a mini-map of the result before starting over on it. A
//! starting town may be picked too: a whole example city generated on the
//! landscape (`nd_procgen::citygen`), drawn on the mini-map.
//!
//! [wizard] Step by step, none skipped: 1 landscape, 2 starting city (or the
//! empty map), 3 the city's name (required; the dice suggests one), 4 a
//! review, then Start. Back and Next move between them; Next waits until the
//! step is done ([`step_done`]). The mini-map shows on every step.

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

/// [wizard] The steps of a new game, in order; none can be skipped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    Landscape,
    Town,
    Name,
    Review,
}

impl Step {
    pub const ALL: [Step; 4] = [Step::Landscape, Step::Town, Step::Name, Step::Review];

    pub fn label(self) -> &'static str {
        match self {
            Step::Landscape => "Landscape",
            Step::Town => "Starting city",
            Step::Name => "Name",
            Step::Review => "Review",
        }
    }

    fn index(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    fn next(self) -> Step {
        Step::ALL[(self.index() + 1).min(Step::ALL.len() - 1)]
    }

    fn back(self) -> Step {
        Step::ALL[self.index().saturating_sub(1)]
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
    /// [wizard] The step on screen.
    step: Step,
    /// [wizard] A starting city (or the empty map) was picked on its step.
    town_picked: bool,
}

impl Draft {
    fn new(params: LandscapeParams) -> Self {
        Self { params, city: None, drawn: None, confirm: false, name: String::new(), step: Step::Landscape, town_picked: false }
    }
}

/// [wizard] Whether a step is done, so Next may be pressed: a landscape
/// always is (one is always picked), a starting city once picked, a name
/// once typed (or rolled). The review starts.
fn step_done(step: Step, town_picked: bool, name: &str) -> bool {
    match step {
        Step::Landscape | Step::Review => true,
        Step::Town => town_picked,
        Step::Name => !nd_sim::identity::clean_name(name).is_empty(),
    }
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
    let mut draft: Draft = ctx.data(|d| d.get_temp(id)).unwrap_or_else(|| Draft::new(p.sim.world.landscape.params.clone()));
    let mut open = true;
    egui::Window::new(egui::RichText::new("New game").color(GOLD))
        .open(&mut open)
        .constrain_to(content)
        .default_pos(content.center() - egui::vec2(260.0, 220.0))
        .resizable(false)
        .show(ctx, |ui| body(ui, p, &mut draft));
    if !open {
        set_open(ctx, false);
    }
    if !is_open(ctx) {
        // Next time it opens it starts again at the first step.
        draft.step = Step::Landscape;
        draft.town_picked = false;
        draft.confirm = false;
    }
    ctx.data_mut(|d| d.insert_temp(id, draft));
}

/// [wizard] The steps as a row: done ones in gold, the current one strong.
fn steps_row(ui: &mut egui::Ui, at: Step) {
    ui.horizontal(|ui| {
        for (i, s) in Step::ALL.iter().enumerate() {
            let t = egui::RichText::new(format!("{} {}", i + 1, s.label()));
            let t = if *s == at {
                t.color(GOLD).strong()
            } else if s.index() < at.index() {
                t.color(GOLD)
            } else {
                t.color(egui::Color32::from_gray(120))
            };
            ui.label(t);
            if i + 1 < Step::ALL.len() {
                ui.label(egui::RichText::new("\u{203a}").color(egui::Color32::from_gray(110)));
            }
        }
    });
    ui.separator();
}

fn body(ui: &mut egui::Ui, p: &mut UiCtx, draft: &mut Draft) {
    steps_row(ui, draft.step);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(270.0);
            match draft.step {
                Step::Landscape => landscape_step(ui, draft),
                Step::Town => town_step(ui, draft),
                Step::Name => name_step(ui, draft),
                Step::Review => review_step(ui, draft),
            }
        });
        // The mini-map of what would be made, on every step.
        ui.vertical(|ui| {
            ui.set_max_width(PREVIEW as f32 * 1.4);
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
                *draft = Draft::new(draft.params.clone());
                set_open(ui.ctx(), false);
            }
            if ui.button("Cancel").clicked() {
                draft.confirm = false;
            }
        });
        return;
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(draft.step != Step::Landscape, egui::Button::new("\u{2039} Back")).clicked() {
            draft.step = draft.step.back();
        }
        if draft.step == Step::Review {
            if ui.button(egui::RichText::new("Start").strong()).on_hover_text("Start the new city").clicked() {
                if p.sim.world.buildings.is_empty() && p.sim.world.citizens.is_empty() {
                    start(p, draft.params.clone(), draft.city, &draft.name);
                    *draft = Draft::new(draft.params.clone());
                    set_open(ui.ctx(), false);
                } else {
                    draft.confirm = true;
                }
            }
            return;
        }
        let done = step_done(draft.step, draft.town_picked, &draft.name);
        let r = ui.add_enabled(done, egui::Button::new(egui::RichText::new("Next \u{203a}").strong()));
        if r.clicked() {
            draft.step = draft.step.next();
        }
        if !done {
            r.on_disabled_hover_text(if draft.step == Step::Town { "Pick a starting city (or the empty map) first" } else { "Name the city first (the dice suggests one)" });
        }
    });
}

/// [wizard] Step 1: the landscape preset, the climate, the levers and the seed.
fn landscape_step(ui: &mut egui::Ui, draft: &mut Draft) {
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
    levers(ui, draft);
}

/// The landscape's levers and seed.
fn levers(ui: &mut egui::Ui, draft: &mut Draft) {
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
}

/// [wizard] Step 2: the empty map or a starting city. A town comes with the
/// landscape it was made for; its levers may still be changed here.
fn town_step(ui: &mut egui::Ui, draft: &mut Draft) {
    section(ui, "STARTING CITY");
    if ui.selectable_label(draft.town_picked && draft.city.is_none(), "Empty map").on_hover_text("Nothing but the highway and the trunk line").clicked() {
        draft.city = None;
        draft.town_picked = true;
    }
    for city in CityPreset::ALL {
        let on = draft.town_picked && draft.city == Some(city);
        if ui.selectable_label(on, city.label()).on_hover_text(city.blurb()).clicked() && !on {
            draft.city = Some(city);
            draft.town_picked = true;
            draft.params = if city.furnished() {
                // [perfect-city] The planned city comes with its own site
                // and climate (the seed may still be changed).
                city.landscape(city.default_seed())
            } else {
                LandscapeParams { climate: draft.params.climate, ..city.landscape(draft.params.seed) }
            };
        }
    }
    if !draft.town_picked {
        hint(ui, "Pick one to go on.");
        return;
    }
    match draft.city {
        Some(city) if city.furnished() => hint(ui, &format!("{} Streets, zoning, diamond interchanges, power, police, fire, health, schools, a university, metro, buses, rail, docks, districts and 50 million in the treasury; population 0. Laying it all out takes about half a minute (the window waits). It starts a game of your own, under your name for it.", city.blurb())),
        Some(city) => hint(ui, &format!("{} Streets, zoning, interchanges and a power plant are laid out; it grows from there.", city.blurb())),
        None => hint(ui, "Every map starts with the highway and the trunk line, edge to edge; the rest is yours to build."),
    }
    if draft.city.is_some() {
        hint(ui, &format!("It brings its own landscape: {} {}.", draft.params.climate.label().to_lowercase(), draft.params.preset.label().to_lowercase()));
        levers(ui, draft);
    }
}

/// [wizard] Step 3: the city's name, required; the dice offers one.
fn name_step(ui: &mut egui::Ui, draft: &mut Draft) {
    section(ui, "CITY NAME");
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(200.0).hint_text("name your city"));
        if ui.small_button("🎲").on_hover_text("suggest a name").clicked() {
            draft.name = suggest_name();
        }
    });
    if nd_sim::identity::clean_name(&draft.name).is_empty() {
        hint(ui, "Type a name, or roll the dice for one.");
    }
    hint(ui, "Saves are listed under this name; it can be changed later in the City or Saves window.");
}

/// [wizard] Step 4: what will be made.
fn review_step(ui: &mut egui::Ui, draft: &mut Draft) {
    section(ui, "REVIEW");
    let row = |ui: &mut egui::Ui, k: &str, v: String| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(k).color(egui::Color32::from_gray(150)));
            ui.label(egui::RichText::new(v).color(GOLD));
        });
    };
    row(ui, "City", nd_sim::identity::clean_name(&draft.name));
    row(ui, "Landscape", format!("{} {}", draft.params.climate.label(), draft.params.preset.label().to_lowercase()));
    row(ui, "Seed", draft.params.seed.to_string());
    row(ui, "Starting city", draft.city.map_or_else(|| "empty map".to_string(), |c| c.label().to_string()));
    hint(ui, "Start makes it; Back changes anything.");
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
        Some(c) => format!("New city {named}: {} on {} landscape", c.label(), nd_sim::names::a(label)),
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
fn preview_image(params: &LandscapeParams) -> (egui::ColorImage, String) {
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
    let mut summary = format!("{rivers} river{}, {streams} stream{}, {} lake{}", super::fmt::s(rivers), super::fmt::s(streams), land.lakes.len(), super::fmt::s(land.lakes.len()));
    if land.sea.is_some() {
        summary.push_str(&format!(", sea, {islands} island{}", super::fmt::s(islands)));
    }
    summary.push_str(&format!(", {} hot spot{}", land.hot_spots.len(), super::fmt::s(land.hot_spots.len())));
    if let Some(r) = land.rivers.first() {
        summary.push_str(&format!(". Main river {:.0} m wide, {:.1} m/s.", r.width, r.current));
    }
    (egui::ColorImage { size: [PREVIEW, PREVIEW], source_size: egui::vec2(PREVIEW as f32, PREVIEW as f32), pixels }, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [wizard] Next waits for a starting city and for a name; the steps
    /// run in order and stop at the ends.
    #[test]
    fn wizard_steps_need_their_choice() {
        assert!(step_done(Step::Landscape, false, ""));
        assert!(!step_done(Step::Town, false, "x"));
        assert!(step_done(Step::Town, true, ""));
        assert!(!step_done(Step::Name, true, ""));
        assert!(!step_done(Step::Name, true, "   \t "), "blank is no name");
        assert!(step_done(Step::Name, true, "Neon Bay"));
        assert_eq!(Step::Landscape.next(), Step::Town);
        assert_eq!(Step::Town.next(), Step::Name);
        assert_eq!(Step::Name.next(), Step::Review);
        assert_eq!(Step::Review.next(), Step::Review);
        assert_eq!(Step::Landscape.back(), Step::Landscape);
        assert_eq!(Step::Review.back(), Step::Name);
    }
}
