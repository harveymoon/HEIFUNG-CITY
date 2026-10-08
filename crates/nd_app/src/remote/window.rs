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
//! This keeps `remote.rs` free of egui.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_egui::EguiContexts;
use serde_json::{json, Value};

use crate::ui::city_data::{Page, Tab, Target};
use crate::ui::Info;

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
