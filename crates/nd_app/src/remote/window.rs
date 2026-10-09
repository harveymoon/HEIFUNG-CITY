//! `window {name, open?}`: open, close or toggle the UI's tool windows, and
//! [city-data] `city_data {tab?, sub?, open?}` (and `window` with the old
//! names of the report windows) for the full-screen City Data view.
//!
//! Window open state lives in egui's memory (see `ui::windows::Info` and
//! `ui::city_data`), which only exists inside an egui pass. The remote
//! command runs in `Update`, so it queues the request here and
//! `apply_window_requests`, in `EguiPrimaryContextPass`, applies it before
//! the windows are drawn that frame; the same system mirrors which windows
//! are open and what City Data shows, for `status` and the replies.
//! This keeps the command handlers free of egui.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_egui::EguiContexts;
use serde_json::{json, Value};

use crate::ui::city_data::{Page, Tab, Target};
use crate::ui::Info;

use super::ctx::Ctx;
use super::view::*;
use crate::tools::Tool;

/// A change to the City Data view.
#[derive(Clone, Copy, Debug)]
pub enum DataReq {
    Open(Target),
    Close,
}

#[derive(Resource, Default)]
pub struct WindowRequests {
    queue: Vec<(Info, Option<bool>)>,
    /// Titles of the windows open at the end of the last egui pass.
    pub open: Vec<&'static str>,
    /// The New Game window (not an `Info` window).
    pub new_game: Option<bool>,
    /// Open (true) or close the Build window, optionally at a section.
    pub build: Option<(bool, Option<crate::ui::build::Cat>)>,
    /// Whether the Build window was open at the end of the last egui pass,
    /// and the section it showed.
    /// [build-highlight] Open the Build window at a section and sub-section.
    pub build_sub: Option<(crate::ui::build::Cat, Option<&'static str>)>,
    pub build_open: bool,
    pub build_section: Option<&'static str>,
    /// [city-data] A change to the City Data view, applied in the pass.
    pub data: Option<DataReq>,
    /// [city-data] Whether the view was open at the end of the last pass,
    /// the page it showed, and each tab's last page.
    pub data_open: bool,
    pub data_page: Option<Page>,
    pub data_last: HashMap<Tab, Page>,
    /// [city-data] Scroll the page to this many points from its top.
    pub data_scroll: Option<f32>,
    /// [directory] Settings for the Directory tables (`directory {show}`).
    pub directory_biz: Option<crate::ui::directory::TableState<crate::directory::BizCol>>,
    pub directory_people: Option<crate::ui::directory::TableState<crate::directory::PersonCol>>,
}

/// A tool window by name. The View window was the Land window; its old name
/// and a few obvious ones still find it.
pub fn info_of(name: &str) -> Result<Info, String> {
    let n = name.trim().to_ascii_lowercase().replace('_', " ");
    if matches!(n.as_str(), "land" | "layers" | "map" | "map layers") {
        return Ok(Info::Land);
    }
    Info::ALL.into_iter().find(|i| i.title().eq_ignore_ascii_case(name.trim())).ok_or_else(|| {
        format!(
            "unknown window {name:?}; one of {}, build, new_game, history, title; or a City Data name: city, economy, taxes, market, people, power, transit, emergencies, districts, tourism, services",
            Info::ALL.iter().map(|i| i.title().to_ascii_lowercase()).collect::<Vec<_>>().join(", ")
        )
    })
}

impl WindowRequests {
    /// Queue a change and return the open list as it will be once applied
    /// (the egui pass runs later this frame).
    pub fn request(&mut self, info: Info, open: Option<bool>) -> (bool, Vec<String>) {
        let now = self.open.contains(&info.title());
        let will = open.unwrap_or(!now);
        self.queue.push((info, Some(will)));
        let list = Info::ALL
            .into_iter()
            .filter(|i| if *i == info { will } else { self.open.contains(&i.title()) })
            .map(|i| i.title().to_ascii_lowercase())
            .collect();
        (will, list)
    }

    pub fn open_names(&self) -> Vec<String> {
        self.open.iter().map(|t| t.to_ascii_lowercase()).collect()
    }

    /// The page a target lands on, as the pass will resolve it.
    fn resolve(&self, t: Target) -> Page {
        match t {
            Target::Page(p) => p,
            Target::Tab(tab) => self.data_last.get(&tab).copied().unwrap_or(tab.pages()[0]),
        }
    }

    /// [city-data] Open the view at `target` (or where it was), close it
    /// (`open: Some(false)`), or toggle it (`open: None`: closes when it
    /// already shows that target). Returns the view as it will be.
    pub fn request_data(&mut self, target: Option<Target>, open: Option<bool>) -> Value {
        let showing = self.data_open
            && match (target, self.data_page) {
                (Some(Target::Tab(t)), Some(p)) => p.tab() == t,
                (Some(Target::Page(want)), Some(p)) => p == want,
                (None, _) => true,
                _ => false,
            };
        if open.unwrap_or(!showing) {
            let t = target.unwrap_or(Target::Page(self.data_page.unwrap_or(Page::CityOverview)));
            let p = self.resolve(t);
            self.data = Some(DataReq::Open(Target::Page(p)));
            self.data_open = true;
            self.data_page = Some(p);
            self.data_last.insert(p.tab(), p);
        } else {
            self.data = Some(DataReq::Close);
            self.data_open = false;
        }
        self.data_json()
    }

    /// [city-data] The view's state and its map of tabs and sub-tabs.
    pub fn data_json(&self) -> Value {
        let p = self.data_page.unwrap_or(Page::CityOverview);
        json!({
            "open": self.data_open,
            "tab": p.tab().key(),
            "sub": p.key(),
            "title": p.title(),
            "tabs": Tab::ALL.iter().map(|t| json!({ "tab": t.key(), "label": t.label(), "subs": t.pages().iter().map(|p| p.key()).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        })
    }

    /// [city-data] The short form for `status`.
    pub fn data_status(&self) -> Value {
        let p = self.data_page.unwrap_or(Page::CityOverview);
        json!({ "open": self.data_open, "tab": p.tab().key(), "sub": p.key() })
    }
}

pub fn apply_window_requests(mut contexts: EguiContexts, mut req: ResMut<WindowRequests>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if let Some(open) = req.new_game.take() {
        crate::ui::newgame::set_open(ctx, open);
    }
    for (info, open) in std::mem::take(&mut req.queue) {
        let open = open.unwrap_or(!info.is_open(ctx));
        info.set_open(ctx, open);
    }
    if let Some((open, section)) = req.build.take() {
        match (open, section) {
            (true, Some(cat)) => crate::ui::build::show_section(ctx, cat),
            (open, _) => crate::ui::build::set_open(ctx, open),
        }
    }
    if let Some((cat, sub)) = req.build_sub.take() {
        crate::ui::build::show_sub(ctx, cat, sub);
    }
    match req.data.take() {
        Some(DataReq::Open(t)) => crate::ui::city_data::open_at(ctx, t.resolve(ctx)),
        Some(DataReq::Close) => crate::ui::city_data::set_open(ctx, false),
        None => {}
    }
    if let Some(s) = req.directory_biz.take() {
        crate::ui::directory::set_businesses(ctx, s);
    }
    if let Some(s) = req.directory_people.take() {
        crate::ui::directory::set_people(ctx, s);
    }
    if let Some(y) = req.data_scroll.take() {
        crate::ui::city_data::scroll_to(ctx, y);
    }
    req.open = Info::ALL.into_iter().filter(|i| i.is_open(ctx)).map(|i| i.title()).collect();
    req.build_open = crate::ui::build::is_open(ctx);
    req.build_section = crate::ui::build::section_of(ctx).map(|c| c.key());
    req.data_open = crate::ui::city_data::is_open(ctx);
    req.data_page = Some(crate::ui::city_data::page(ctx));
    for t in Tab::ALL {
        let p = crate::ui::city_data::last_of(ctx, t);
        req.data_last.insert(t, p);
    }
}

/// What a `window` command's `name` names, parsed once.
enum Named {
    NewGame,
    Build,
    Committee,
    GameHistory,
    Title,
    /// [city-data] An old report window's name: its City Data tab or page.
    CityData(Target),
    /// A tool window, or an unknown name (`info_of` says which).
    Tool,
}

impl Named {
    fn parse(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "new_game" | "new game" => Self::NewGame,
            "build" => Self::Build,
            "committee" => Self::Committee,
            "history" | "game_history" | "game history" => Self::GameHistory,
            "title" | "title_screen" | "title screen" => Self::Title,
            _ => Page::from_window(name).map_or(Self::Tool, Self::CityData),
        }
    }
}

/// `window {name, open?}`.
pub(super) fn window(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let named = req_str(cmd, "name").map(Named::parse);
    match named {
        Ok(Named::NewGame) => {
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            ctx.windows.new_game = Some(open);
            Ok(json!({ "window": "new game", "open": open }))
        }
        Ok(Named::Build) => {
            // The Build window, optionally at a section.
            let section = match opt_str(cmd, "section")? {
                Some(k) => Some(crate::ui::build::Cat::parse(k).ok_or_else(|| format!("unknown section {k:?}; one of {}", crate::ui::build::Cat::ALL.iter().map(|c| c.key()).collect::<Vec<_>>().join(", ")))?),
                None => None,
            };
            let open = opt_bool(cmd, "open")?.unwrap_or(if section.is_some() { true } else { !ctx.windows.build_open });
            ctx.windows.build = Some((open, section));
            Ok(json!({ "window": "build", "open": open, "section": section.map(|c| c.key()).or(ctx.windows.build_section) }))
        }
        Ok(Named::Committee) => {
            // [committee] The Committee window, optionally at a tab.
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            let tab = match opt_str(cmd, "tab")? {
                Some(t) => Some(crate::ui::committee::Tab::parse(t).ok_or_else(|| format!("unknown tab {t:?}; meeting, members, alerts, projects, settings"))?),
                None => None,
            };
            ctx.committee.open_request = Some(open);
            ctx.committee.tab_request = tab;
            Ok(json!({ "window": "committee", "open": open, "tab": opt_str(cmd, "tab")? }))
        }
        Ok(Named::GameHistory) => {
            // [identity] The Game history window.
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            ctx.saves.history_requested = Some(open);
            Ok(json!({ "window": "game history", "open": open }))
        }
        Ok(Named::Title) => {
            let open = opt_bool(cmd, "open")?.unwrap_or(!ctx.saves.title);
            ctx.saves.title = open;
            if !open {
                // As "Play this map": the city behind runs again.
                ctx.sim.paused = false;
            }
            Ok(json!({ "window": "title", "open": open }))
        }
        Ok(Named::CityData(mut target)) => {
            // [city-data] The report windows are City Data pages now: the old
            // name opens its tab (at its last sub-tab) or its page; People's
            // old `tab` and a `sub` pick a sub-tab. `expanded` is accepted and
            // means open: every page is full size.
            if let Some(sub) = opt_str(cmd, "sub")?.or(opt_str(cmd, "tab")?) {
                let tab = match target {
                    Target::Tab(t) => t,
                    Target::Page(p) => p.tab(),
                };
                let sub = if sub.trim().eq_ignore_ascii_case("overview") && tab == Tab::People { "population" } else { sub };
                target = Target::Page(Page::parse(None, tab.key(), Some(sub))?);
            }
            let open = if opt_bool(cmd, "expanded")? == Some(true) { Some(true) } else { opt_bool(cmd, "open")? };
            let mut v = ctx.windows.request_data(Some(target), open);
            v["window"] = json!("city data");
            v["note"] = json!("the report windows are tabs of the City Data view now; see city_data");
            Ok(v)
        }
        Ok(Named::Tool) | Err(_) => {
            let info = info_of(req_str(cmd, "name")?)?;
            let (open, list) = ctx.windows.request(info, opt_bool(cmd, "open")?);
            Ok(json!({ "window": info.title().to_ascii_lowercase(), "open": open, "windows_open": list }))
        }
    }
}

/// [city-data] `city_data {tab?, sub?, open?, scroll?}`: the full-screen
/// City Data view.
pub(super) fn city_data(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let tab = opt_str(cmd, "tab")?;
    let sub = opt_str(cmd, "sub")?;
    let open = opt_bool(cmd, "open")?;
    if let Some(y) = opt_f32(cmd, "scroll")? {
        ctx.windows.data_scroll = Some(y.max(0.0));
    }
    if tab.is_none() && sub.is_some() {
        return Err("sub needs a tab".into());
    }
    if tab.is_none() && open.is_none() {
        return Ok(ctx.windows.data_json());
    }
    let target = match (tab, sub) {
        (Some(t), Some(s)) => Some(Target::Page(Page::parse(None, t, Some(s))?)),
        (Some(t), None) => Some(Target::Tab(Tab::parse(t).ok_or_else(|| format!("unknown tab {t:?}; one of city, economy, market, people, services, directory"))?)),
        _ => None,
    };
    // Naming a page opens it unless told otherwise.
    let open = open.or(target.map(|_| true));
    Ok(ctx.windows.request_data(target, open))
}

/// [build-highlight] `build_highlight {category, off}`: open the Build
/// window on a section, sub-section or kind so its buildings and service
/// areas show on the map; with no fields, what shows now.
pub(super) fn build_highlight(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    use crate::ui::build;
    let now = |ctx: &Ctx| {
        let sel = ctx.highlight.sel;
        let view = ctx.highlight.view.as_ref();
        json!({
            "on": sel.is_some(),
            "section": sel.map(|s| s.cat.key()),
            "sub": sel.and_then(|s| s.sub),
            "focus": sel.and_then(|s| s.focus).map(|p| p.label()),
            "buildings": view.map_or(Vec::new(), |v| v.marked.iter().map(|m| json!({ "building": key_str(m.id), "name": m.name, "kind": format!("{:?}", m.family).to_lowercase(), "level": format!("{:?}", m.level).to_lowercase(), "label": m.label })).collect()),
            "summary": view.map(|v| v.summary.headline.clone()),
            "detail": view.map(|v| v.summary.detail.clone()),
            "homes_covered": view.and_then(|v| v.homes_covered),
            "uncovered": view.map(|v| v.summary.spots.iter().map(|s| json!({ "name": s.name, "at": [s.at.x, s.at.y], "homes": s.homes })).collect::<Vec<_>>()),
        })
    };
    if opt_bool(cmd, "off")? == Some(true) {
        ctx.windows.build = Some((false, None));
        if build::is_build_tool(ctx.tools.tool) {
            ctx.tools.tool = Tool::Inspect;
            ctx.tools.points.clear();
            ctx.tools.drag = None;
        }
        return Ok(json!({ "on": false }));
    }
    let Some(name) = opt_str(cmd, "category")?.or(opt_str(cmd, "item")?) else { return Ok(now(ctx)) };
    let (cat, sub, item) = build::resolve_highlight(name).ok_or_else(|| format!("nothing to highlight for {name:?}: a section (power, services, transit, rail, water, attractions), a sub-section (safety, sanitation, parking, metro...) or a kind (health, police, fire, school, metro_station...)"))?;
    if build::highlight_kinds(cat, sub).is_empty() {
        return Err(format!("{} has no buildings to highlight (roads, zones, districts, lines and tolls have none)", sub.unwrap_or(cat.label())));
    }
    if let Some(item) = item {
        build::activate(item, &mut ctx.tools);
    } else if build::is_build_tool(ctx.tools.tool) && build::current_item(&ctx.tools).and_then(|i| i.placeable()).is_none_or(|p| !build::highlight_kinds(cat, sub).contains(&build::norm(p))) {
        // A tool for something else would keep the window elsewhere.
        ctx.tools.tool = Tool::Inspect;
    }
    ctx.windows.build_sub = Some((cat, sub));
    Ok(json!({ "on": true, "section": cat.key(), "sub": sub, "item": item.map(|i| i.label()), "note": "the highlight and its summary are worked out next frame: build_highlight {} reads them" }))
}
