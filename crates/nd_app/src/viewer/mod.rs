//! [viewer] The building viewer: one building at a time on a turntable
//! (`render::turntable`), flipped through with arrows or the keyboard,
//! with notes on each building and each style.
//!
//! Two lists: the buildings standing in the city (one row per building a
//! renderer draws; tenants are listed under the building they rent in) and
//! the catalogue of every style the generator can make (`catalogue`), each
//! in a few seeded variants. A third tab lists every note. The viewer is
//! a window over the map (top bar "Viewer", or X); the city runs on behind
//! it. While it is open the keys are the viewer's: left/right flip, up/down
//! change the variant (or flip in the city list), Esc closes.
//!
//! The state is a resource rather than egui memory so the remote API's
//! `viewer` command can drive it (open, select, filter, light, screenshot)
//! without an egui pass.

pub mod catalogue;
pub mod notes;
mod window;
pub use window::{inspirations as window_inspirations, is_open, toggle};

use bevy::prelude::*;
use nd_procgen::Geometry;
use nd_sim::{BuildingId, LandUse, Tier, Vec2 as SVec2};
use serde_json::{json, Value};
use slotmap::Key;

use crate::assets::GameAssets;
use crate::render::Turntable;
use crate::sim::{Sim, SimSet};
use catalogue::Entry;
use notes::{BuildingNotes, Target};

/// Which list the viewer flips through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    City,
    Catalogue,
    /// Every note; picking one shows what it is about.
    Notes,
}

impl Source {
    pub fn key(self) -> &'static str {
        match self {
            Source::City => "city",
            Source::Catalogue => "catalogue",
            Source::Notes => "notes",
        }
    }

    pub fn parse(s: &str) -> Option<Source> {
        match s.trim().to_ascii_lowercase().as_str() {
            "city" | "buildings" => Some(Source::City),
            "catalogue" | "catalog" | "styles" => Some(Source::Catalogue),
            "notes" => Some(Source::Notes),
            _ => None,
        }
    }
}

/// What the lists are narrowed to. `None` or empty is any.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filters {
    /// Words to find in the name, style, owner, business, district.
    pub search: String,
    /// A catalogue section (`catalogue::GROUPS`).
    pub group: Option<String>,
    /// [city] A building form (mall, mansion, tower...).
    pub form: Option<String>,
    pub land_use: Option<LandUse>,
    /// [city] A district's name.
    pub district: Option<String>,
    /// [city] A district's story.
    pub story: Option<String>,
    /// [city] A business kind's label.
    pub business: Option<String>,
    pub tier: Option<Tier>,
    /// Only what has open notes.
    pub noted: bool,
}

/// One building of the city, as the viewer lists it.
#[derive(Clone, Debug)]
pub struct CityRow {
    pub id: BuildingId,
    pub key: String,
    pub name: String,
    pub style: Option<nd_procgen::Style>,
    pub style_label: &'static str,
    pub group: &'static str,
    pub form: Option<&'static str>,
    pub land_use: LandUse,
    pub tier: Tier,
    pub floors: u8,
    pub district: String,
    pub story: &'static str,
    pub business: Option<&'static str>,
    pub footprint: f32,
    pub pos: SVec2,
    /// Name and kind of each business renting a unit in it.
    pub tenants: Vec<String>,
    pub owner: Option<String>,
    /// "standing", "going up" or "coming down".
    pub state: &'static str,
    pub homes: u16,
    pub jobs: u16,
}

/// The viewer's state.
#[derive(Resource, Default)]
pub struct Viewer {
    pub open: bool,
    pub source: Source,
    pub filters: Filters,
    /// The city building shown.
    pub pick: Option<BuildingId>,
    /// The catalogue style (its index in `catalogue`) and variant shown.
    pub style: usize,
    pub variant: usize,
    /// The note picked in the Notes list.
    pub note: Option<u32>,
    pub catalogue: Vec<Entry>,
    pub rows: Vec<CityRow>,
    rows_key: Option<(u64, usize, u64)>,
    /// The note being written in the note box, and the note it edits.
    pub draft: String,
    pub editing: Option<u32>,
    /// The Inspirations picture shown beside the building, if any.
    pub inspiration: Option<std::path::PathBuf>,
    /// "Go to it in the city": the building the camera should fly to.
    pub go_to: Option<BuildingId>,
    /// The lists were narrowed or the pick moved by the API: scroll to it.
    pub scroll_to_pick: bool,
}

/// A key as the remote API writes ids ("12:1").
pub fn key_str<K: Key>(k: K) -> String {
    let d = k.data().as_ffi();
    format!("{}:{}", d & 0xffff_ffff, d >> 32)
}

/// A city building's row.
pub fn city_row(sim: &Sim, assets: &GameAssets, id: BuildingId) -> Option<CityRow> {
    let w = &sim.world;
    let b = w.buildings.get(id)?;
    if b.is_tenant() {
        return None;
    }
    let lot = w.lots.get(b.lot)?;
    let style = nd_procgen::building_style(w, id, &assets.rules, Some(&sim.styles));
    let district = w.district_of_lot(b.lot);
    let tenants = w
        .tenants_of(id)
        .into_iter()
        .filter_map(|t| w.buildings.get(t))
        .map(|t| match t.business.as_ref() {
            Some(z) => format!("{} ({})", t.name, z.kind.label()),
            None => t.name.clone(),
        })
        .collect();
    let owner = w.owner_of(id).map(|(_, o)| format!("{} ({})", o.name, crate::directory::owner_kind_label(&o.kind)));
    let state = if b.demolition.is_some() {
        "coming down"
    } else if b.construction.is_some() {
        "going up"
    } else {
        "standing"
    };
    Some(CityRow {
        id,
        key: key_str(id),
        name: b.name.clone(),
        style,
        style_label: style.map_or("unknown", |s| s.label()),
        group: style.map_or("Other", catalogue::group),
        form: b.plot.form.map(|f| f.label()),
        land_use: w.shown_use(id),
        tier: b.tier,
        floors: b.floors,
        district: district.map_or_else(String::new, |d| d.name.clone()),
        story: district.map_or("", |d| d.story_label()),
        business: b.business.as_ref().map(|z| z.kind.label()),
        footprint: w.footprint(id).map_or(lot.area(), |l| l.area()),
        pos: lot.center,
        tenants,
        owner,
        state,
        homes: b.homes,
        jobs: b.jobs,
    })
}

/// Does `hay` hold every word of `needle` (any case)?
fn finds(needle: &str, hay: &[&str]) -> bool {
    let hay = hay.join(" ").to_lowercase();
    needle.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
}

impl Viewer {
    /// The city rows again when the city changed (another city, buildings
    /// built or gone, or a new day).
    pub fn refresh_rows(&mut self, sim: &Sim, assets: &GameAssets) {
        let w = &sim.world;
        let key = (sim.generation, w.buildings.len(), w.clock.day());
        if self.rows_key == Some(key) {
            return;
        }
        self.rows_key = Some(key);
        self.rows = w.buildings.keys().filter_map(|id| city_row(sim, assets, id)).collect();
        self.rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.key.cmp(&b.key)));
        if self.pick.is_some_and(|p| !w.buildings.contains_key(p)) {
            self.pick = None;
        }
    }

    /// Force the next `refresh_rows` to rebuild (a city was replaced).
    pub fn dirty_rows(&mut self) {
        self.rows_key = None;
    }

    pub fn row(&self, id: BuildingId) -> Option<&CityRow> {
        self.rows.iter().find(|r| r.id == id)
    }

    /// The city rows the filters let through, as indices into `rows`.
    pub fn city_list(&self, notes: &BuildingNotes, game: &str) -> Vec<usize> {
        let f = &self.filters;
        (0..self.rows.len())
            .filter(|&i| {
                let r = &self.rows[i];
                f.group.as_deref().is_none_or(|g| g == r.group)
                    && f.form.as_deref().is_none_or(|x| r.form == Some(x))
                    && f.land_use.is_none_or(|l| l == r.land_use)
                    && f.district.as_deref().is_none_or(|d| d == r.district)
                    && f.story.as_deref().is_none_or(|s| s == r.story)
                    && f.business.as_deref().is_none_or(|b| r.business == Some(b))
                    && f.tier.is_none_or(|t| t == r.tier)
                    && (f.search.trim().is_empty() || finds(&f.search, &[&r.name, r.style_label, r.group, r.form.unwrap_or(""), r.business.unwrap_or(""), &r.district, r.story, r.owner.as_deref().unwrap_or(""), &r.tenants.join(" "), &r.key]))
                    && (!f.noted || notes.open_count(&building_target(game, "", r)) > 0)
            })
            .collect()
    }

    /// The catalogue entries the filters let through, as indices, by
    /// section (in `catalogue::GROUPS` order) and then in style order.
    pub fn catalogue_list(&self, notes: &BuildingNotes) -> Vec<usize> {
        let f = &self.filters;
        let mut order: Vec<usize> = (0..self.catalogue.len()).collect();
        order.sort_by_key(|&i| (catalogue::GROUPS.iter().position(|g| *g == self.catalogue[i].group), i));
        order
            .into_iter()
            .filter(|&i| {
                let e = &self.catalogue[i];
                f.group.as_deref().is_none_or(|g| g == e.group)
                    && f.land_use.is_none_or(|l| l == e.land_use)
                    && f.tier.is_none_or(|t| e.tiers.contains(&t))
                    && (f.search.trim().is_empty() || finds(&f.search, &[&e.key, e.label, e.group]))
                    && (!f.noted || notes.open_count(&Target::Style { style: e.key.clone() }) > 0)
            })
            .collect()
    }

    /// The notes listed: open first, newest first within each.
    pub fn notes_list(&self, notes: &BuildingNotes) -> Vec<u32> {
        let f = &self.filters;
        let mut v: Vec<&notes::Note> = notes.book.notes.iter().filter(|n| !f.noted || n.is_open()).filter(|n| f.search.trim().is_empty() || finds(&f.search, &[&n.text, &n.target.title()])).collect();
        v.sort_by(|a, b| b.is_open().cmp(&a.is_open()).then(b.updated.cmp(&a.updated)).then(b.id.cmp(&a.id)));
        v.into_iter().map(|n| n.id).collect()
    }

    /// Step through the list shown by `d` (wrapping); in the catalogue a
    /// new style starts at its first variant.
    pub fn step(&mut self, d: i32, notes: &BuildingNotes, game: &str) {
        match self.source {
            Source::City => {
                let list = self.city_list(notes, game);
                if list.is_empty() {
                    return;
                }
                let at = self.pick.and_then(|p| list.iter().position(|&i| self.rows[i].id == p));
                let next = match at {
                    Some(k) => (k as i64 + d as i64).rem_euclid(list.len() as i64) as usize,
                    None => 0,
                };
                self.pick = Some(self.rows[list[next]].id);
            }
            Source::Catalogue => {
                let list = self.catalogue_list(notes);
                if list.is_empty() {
                    return;
                }
                let at = list.iter().position(|&i| i == self.style);
                let next = match at {
                    Some(k) => (k as i64 + d as i64).rem_euclid(list.len() as i64) as usize,
                    None => 0,
                };
                self.style = list[next];
                self.variant = 0;
            }
            Source::Notes => {
                let list = self.notes_list(notes);
                if list.is_empty() {
                    return;
                }
                let at = self.note.and_then(|n| list.iter().position(|&x| x == n));
                let next = match at {
                    Some(k) => (k as i64 + d as i64).rem_euclid(list.len() as i64) as usize,
                    None => 0,
                };
                self.note = Some(list[next]);
            }
        }
        self.scroll_to_pick = true;
    }

    /// The next or previous variant of the catalogue style.
    pub fn step_variant(&mut self, d: i32) {
        if let Some(e) = self.catalogue.get(self.style) {
            let n = e.variants.len().max(1) as i64;
            self.variant = (self.variant as i64 + d as i64).rem_euclid(n) as usize;
        }
    }

    /// What is on show: a city building or a catalogue variant. In the
    /// Notes list, what the picked note is about (a building only while it
    /// stands in this city).
    pub fn subject(&self, sim: &Sim, notes: &BuildingNotes) -> Subject {
        match self.source {
            Source::City => self.pick.filter(|p| sim.world.buildings.contains_key(*p)).map_or(Subject::None, Subject::City),
            Source::Catalogue => self.catalogue.get(self.style).map_or(Subject::None, |e| Subject::Style(e.style, self.variant.min(e.variants.len().saturating_sub(1)))),
            Source::Notes => match self.note.and_then(|id| notes.get(id)).map(|n| &n.target) {
                Some(Target::Building { game, building, .. }) if *game == sim.world.identity.game_id => {
                    let id = crate::viewer::building_of(&sim.world, building);
                    id.map_or(Subject::None, Subject::City)
                }
                Some(Target::Style { style }) => catalogue::style_of(style).map_or(Subject::None, |s| Subject::Style(s, 0)),
                _ => Subject::None,
            },
        }
    }

    /// The note target of what is on show.
    pub fn target(&self, sim: &Sim, notes: &BuildingNotes) -> Option<Target> {
        match self.subject(sim, notes) {
            Subject::City(id) => {
                let r = self.row(id)?;
                Some(building_target(&sim.world.identity.game_id, sim.world.identity.display_name(), r))
            }
            Subject::Style(s, _) => Some(Target::Style { style: catalogue::style_key(s) }),
            Subject::None => None,
        }
    }

    /// The subject as JSON, for the API.
    pub fn subject_json(&self, sim: &Sim, notes: &BuildingNotes) -> Value {
        match self.subject(sim, notes) {
            Subject::City(id) => self.row(id).map_or(Value::Null, |r| city_row_json(r)),
            Subject::Style(s, v) => {
                let e = &self.catalogue[s.index()];
                let var = e.variants.get(v).copied();
                json!({
                    "kind": "style", "style": e.key, "label": e.label, "group": e.group, "family": format!("{:?}", e.family),
                    "land_use": e.land_use.label(), "tiers": e.tiers.iter().map(|t| t.label()).collect::<Vec<_>>(), "on_zone_menu": e.on_menu,
                    "variant": v, "variants": e.variants.len(),
                    "tier": var.map(|x| x.tier.label()), "floors": var.map(|x| x.floors), "seed": var.map(|x| x.seed),
                })
            }
            Subject::None => Value::Null,
        }
    }
}

/// What the turntable shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Subject {
    None,
    City(BuildingId),
    /// A catalogue style and its variant.
    Style(nd_procgen::Style, usize),
}

impl Subject {
    /// The turntable's key: a building redraws when it is rebuilt.
    pub fn key(self, sim: &Sim) -> Option<String> {
        match self {
            Subject::None => None,
            Subject::City(id) => {
                let b = sim.world.buildings.get(id)?;
                Some(format!("b {} {} {} {} {}", key_str(id), sim.generation, b.floors, b.seed, b.construction.is_some() as u8 + 2 * b.demolition.is_some() as u8))
            }
            Subject::Style(s, v) => Some(format!("s {s:?} {v}")),
        }
    }

    /// Draw it: a city building as the city draws it (its lot, its
    /// neighbours' style picks, its brand, snow in a cold climate); a
    /// catalogue variant on its made-up lot.
    pub fn geometry(self, sim: &Sim, assets: &GameAssets, catalogue: &[Entry]) -> Option<Geometry> {
        match self {
            Subject::None => None,
            Subject::City(id) => {
                let mut g = crate::render::building_geometry(&sim.world, assets, &sim.styles, id)?;
                nd_procgen::climate::snow_cover(&mut g, assets.palette.snow);
                Some(g)
            }
            Subject::Style(s, v) => {
                let e = catalogue.get(s.index())?;
                let var = *e.variants.get(v)?;
                Some(catalogue::geometry(e, var, &assets.rules, &assets.palette))
            }
        }
    }
}

/// A building by its API id in this world.
pub fn building_of(w: &nd_sim::World, key: &str) -> Option<BuildingId> {
    let (idx, ver) = key.split_once(':')?;
    let idx: u32 = idx.trim().parse().ok()?;
    let ver: u32 = ver.trim().parse().ok()?;
    let id = BuildingId::from(slotmap::KeyData::from_ffi((u64::from(ver) << 32) | u64::from(idx)));
    w.buildings.contains_key(id).then_some(id)
}

/// The note target for a city row.
pub fn building_target(game: &str, city: &str, r: &CityRow) -> Target {
    Target::Building { game: game.to_string(), city: city.to_string(), building: r.key.clone(), name: r.name.clone(), pos: [r.pos.x, r.pos.y], style: r.style_label.to_string() }
}

pub fn city_row_json(r: &CityRow) -> Value {
    json!({
        "kind": "building", "id": r.key, "name": r.name, "style": r.style_label, "style_key": r.style.map(catalogue::style_key), "group": r.group,
        "form": r.form, "land_use": r.land_use.label(), "tier": r.tier.label(), "floors": r.floors,
        "district": r.district, "story": r.story, "business": r.business, "footprint_m2": r.footprint.round(),
        "pos": [r.pos.x.round(), r.pos.y.round()], "tenants": r.tenants, "owner": r.owner, "state": r.state, "homes": r.homes, "jobs": r.jobs,
    })
}

pub struct ViewerPlugin;

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Viewer>()
            .add_plugins(notes::NotesPlugin)
            .add_systems(PreUpdate, keys_to_viewer.after(crate::ui::read_input_flags))
            .add_systems(Update, sync.after(SimSet::Sync))
            .add_systems(bevy_egui::EguiPrimaryContextPass, window::window.after(crate::ui::side_panel));
    }
}

/// While the viewer is open the keys are its own: the map's camera and
/// tools do not move under it.
fn keys_to_viewer(viewer: Res<Viewer>, mut flags: ResMut<crate::ui::UiFlags>) {
    if viewer.open {
        flags.keyboard_captured = true;
    }
}

/// Keep the turntable on what the viewer shows; up while open, down when
/// closed.
fn sync(mut viewer: ResMut<Viewer>, mut tt: ResMut<Turntable>, sim: Res<Sim>, assets: Res<GameAssets>, notes: Res<BuildingNotes>) {
    tt.active = viewer.open;
    if !viewer.open {
        return;
    }
    if viewer.catalogue.is_empty() || assets.rules_changed {
        viewer.catalogue = catalogue::entries(&assets.rules);
    }
    if sim.full_rebuild {
        viewer.dirty_rows();
    }
    viewer.refresh_rows(&sim, &assets);
    if viewer.source == Source::City && viewer.pick.is_none() {
        viewer.step(0, &notes, &sim.world.identity.game_id);
    }
    let subject = viewer.subject(&sim, &notes);
    let Some(key) = subject.key(&sim) else { return };
    if !tt.showing(&key) {
        if let Some(g) = subject.geometry(&sim, &assets, &viewer.catalogue) {
            tt.show(&key, g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [viewer] Flipping wraps both ways and keeps to the filters; the
    /// catalogue's variants wrap too.
    #[test]
    fn flipping_follows_the_filters() {
        let mut sim = Sim::new();
        sim.seed_demo_district();
        // A day for the zones to fill in.
        sim.step_ticks(1440);
        let assets = GameAssets::for_tests();
        let notes = BuildingNotes::default();
        let mut v = Viewer { catalogue: catalogue::entries(&assets.rules), ..Default::default() };
        v.refresh_rows(&sim, &assets);
        assert!(v.rows.len() > 3, "the demo city has buildings");
        let all = v.city_list(&notes, "");
        v.step(0, &notes, "");
        assert_eq!(v.pick, Some(v.rows[all[0]].id));
        v.step(-1, &notes, "");
        assert_eq!(v.pick, Some(v.rows[*all.last().unwrap()].id), "back from the first is the last");
        v.step(1, &notes, "");
        assert_eq!(v.pick, Some(v.rows[all[0]].id));
        // A land use only.
        let lu = v.rows[all[0]].land_use;
        v.filters.land_use = Some(lu);
        let some = v.city_list(&notes, "");
        assert!(!some.is_empty() && some.iter().all(|&i| v.rows[i].land_use == lu));
        // The catalogue by section; variants wrap.
        v.source = Source::Catalogue;
        v.filters = Filters { group: Some("Malls".into()), ..Default::default() };
        let malls = v.catalogue_list(&notes);
        assert_eq!(malls.len(), 3);
        v.step(1, &notes, "");
        assert!(malls.contains(&v.style));
        v.step_variant(-1);
        assert_eq!(v.variant, v.catalogue[v.style].variants.len() - 1);
        assert!(matches!(v.subject(&sim, &notes), Subject::Style(s, _) if catalogue::group(s) == "Malls"));
    }
}
