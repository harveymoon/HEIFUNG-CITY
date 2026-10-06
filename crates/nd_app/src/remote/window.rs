//! `window {name, open?}`: open, close or toggle the UI's info windows.
//!
//! Window open state lives in egui's memory (see `ui::windows::Info`), which
//! only exists inside an egui pass. The remote command runs in `Update`, so
//! it queues the request here and `apply_window_requests`, in
//! `EguiPrimaryContextPass`, applies it before the windows are drawn that
//! frame; the same system mirrors which windows are open for `status`.
//! This keeps `remote.rs` free of egui.

use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::ui::Info;

#[derive(Resource, Default)]
pub struct WindowRequests {
    queue: Vec<(Info, Option<bool>)>,
    /// Titles of the windows open at the end of the last egui pass.
    pub open: Vec<&'static str>,
    /// The New Game window (not an `Info` window).
    pub new_game: Option<bool>,
    /// [concerns] A People window tab to show.
    pub people_tab: Option<crate::ui::people::Tab>,
}

pub fn info_of(name: &str) -> Result<Info, String> {
    Info::ALL
        .into_iter()
        .find(|i| i.title().eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| format!("unknown window {name:?}; one of {}", Info::ALL.iter().map(|i| i.title().to_ascii_lowercase()).collect::<Vec<_>>().join(", ")))
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
}

pub fn apply_window_requests(mut contexts: EguiContexts, mut req: ResMut<WindowRequests>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if let Some(open) = req.new_game.take() {
        crate::ui::newgame::set_open(ctx, open);
    }
    if let Some(tab) = req.people_tab.take() {
        crate::ui::people::set_tab(ctx, tab);
    }
    for (info, open) in std::mem::take(&mut req.queue) {
        let open = open.unwrap_or(!info.is_open(ctx));
        info.set_open(ctx, open);
    }
    req.open = Info::ALL.into_iter().filter(|i| i.is_open(ctx)).map(|i| i.title()).collect();
}
