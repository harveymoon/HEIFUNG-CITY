//! [city-data] The City Data view: every report on the city in one place,
//! filling the map area between the top bar and the toolbar. Not a window:
//! it does not move or resize, and the map's labels, pins and legend stand
//! down while it is up. The game keeps running behind it.
//!
//! A row of tabs (City, Economy, Market, People, Services, Directory), a row of
//! sub-tabs under it, then the page. Every page has the same furniture
//! (`ui/pane.rs`): a title and a line on what it is for, headline figures
//! as cards, then sections under one kind of heading with full-width charts,
//! tables and labelled figures. The range picker (72 h to all) sits at the
//! right of the sub-tab row on pages with charts over time, one setting for
//! them all.
//!
//! [pt-ux] It sits above every other pane while it is open (the tool
//! windows, the inspector, the Build window, the Committee: all below it,
//! where they stay until it closes), on the foreground layer. A name
//! clicked in a page selects it and closes the view, so the inspector shows
//! beside the map; a "go" button that flies the camera somewhere also
//! closes the view, so the player sees where it went.
//!
//! Whether it is open and which page shows (and each tab's last sub-tab)
//! live in egui memory, so they last the session and the UI keeps no state
//! of its own. Keys: J opens and closes it, Esc closes it.

use bevy_egui::egui;

use super::{pane, UiCtx, GOLD, IVORY};

/// The top-level tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    City,
    Economy,
    Market,
    People,
    Services,
    /// [directory] Every business and every person, in sortable tables.
    Directory,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::City, Tab::Economy, Tab::Market, Tab::People, Tab::Services, Tab::Directory];

    pub fn label(self) -> &'static str {
        match self {
            Tab::City => "City",
            Tab::Economy => "Economy",
            Tab::Market => "Market",
            Tab::People => "People",
            Tab::Services => "Services",
            Tab::Directory => "Directory",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Tab::City => "city",
            Tab::Economy => "economy",
            Tab::Market => "market",
            Tab::People => "people",
            Tab::Services => "services",
            Tab::Directory => "directory",
        }
    }

    /// The tab's pages, in sub-tab order.
    pub fn pages(self) -> &'static [Page] {
        use Page::*;
        match self {
            Tab::City => &[CityOverview, CityLand, CityDistricts, CitySkyline, CityTourism, CityParks],
            Tab::Economy => &[EconomyOverview, EconomyCashFlow, EconomyLedger, EconomyMoneyFlow, EconomyTaxes, EconomyCorporations],
            Tab::Market => &[MarketGoods, MarketBusinesses, MarketTrade],
            Tab::People => &[PeoplePopulation, PeopleInequality, PeopleConcerns, PeopleSocial, PeopleFavourites],
            Tab::Services => &[ServicesPower, ServicesTransit, ServicesEmergencies],
            Tab::Directory => &[DirectoryBusinesses, DirectoryPeople],
        }
    }

    pub fn parse(s: &str) -> Option<Tab> {
        let s = s.trim().to_ascii_lowercase();
        Tab::ALL.into_iter().find(|t| t.key() == s)
    }
}

/// One page: a sub-tab of a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    CityOverview,
    CityLand,
    CityDistricts,
    CitySkyline,
    CityTourism,
    /// [parks] (D5) Every park, how kept, clean-ups and preserves.
    CityParks,
    EconomyOverview,
    EconomyCashFlow,
    EconomyLedger,
    /// [ledger] Where rent, wages, profits and taxes go.
    EconomyMoneyFlow,
    EconomyTaxes,
    /// [megacorp] Mega-corps, universities and the rules on them.
    EconomyCorporations,
    MarketGoods,
    MarketBusinesses,
    MarketTrade,
    PeoplePopulation,
    PeopleInequality,
    PeopleConcerns,
    PeopleSocial,
    PeopleFavourites,
    ServicesPower,
    ServicesTransit,
    ServicesEmergencies,
    DirectoryBusinesses,
    DirectoryPeople,
}

impl Page {
    pub fn tab(self) -> Tab {
        Tab::ALL.into_iter().find(|t| t.pages().contains(&self)).unwrap_or(Tab::City)
    }

    /// The sub-tab's label.
    pub fn label(self) -> &'static str {
        use Page::*;
        match self {
            CityOverview | EconomyOverview => "Overview",
            CityLand => "Land and rent",
            CityDistricts => "Districts",
            CitySkyline => "Skyline",
            CityTourism => "Tourism",
            CityParks => "Parks",
            EconomyCashFlow => "Cash flow",
            EconomyLedger => "Ledger",
            EconomyMoneyFlow => "Money flow",
            EconomyTaxes => "Taxes",
            EconomyCorporations => "Corporations",
            MarketGoods => "Goods and prices",
            MarketBusinesses => "Businesses",
            MarketTrade => "Trade",
            PeoplePopulation => "Population",
            PeopleInequality => "Income and inequality",
            PeopleConcerns => "Complaints and concerns",
            PeopleSocial => "Social",
            PeopleFavourites => "Favourites",
            ServicesPower => "Power",
            ServicesTransit => "Transit",
            ServicesEmergencies => "Emergencies",
            DirectoryBusinesses => "Businesses",
            DirectoryPeople => "People",
        }
    }

    /// The sub-tab's name in the API (`city_data {tab, sub}`).
    pub fn key(self) -> &'static str {
        use Page::*;
        match self {
            CityOverview | EconomyOverview => "overview",
            CityLand => "land",
            CityDistricts => "districts",
            CitySkyline => "skyline",
            CityTourism => "tourism",
            CityParks => "parks",
            EconomyCashFlow => "cash_flow",
            EconomyLedger => "ledger",
            EconomyMoneyFlow => "money_flow",
            EconomyTaxes => "taxes",
            EconomyCorporations => "corporations",
            MarketGoods => "goods",
            MarketBusinesses => "businesses",
            MarketTrade => "trade",
            PeoplePopulation => "population",
            PeopleInequality => "inequality",
            PeopleConcerns => "concerns",
            PeopleSocial => "social",
            PeopleFavourites => "favourites",
            ServicesPower => "power",
            ServicesTransit => "transit",
            ServicesEmergencies => "emergencies",
            DirectoryBusinesses => "businesses",
            DirectoryPeople => "people",
        }
    }

    /// The page's title.
    pub fn title(self) -> &'static str {
        use Page::*;
        match self {
            CityOverview => "The city at a glance",
            CityLand => "Land and rent",
            CityDistricts => "Districts",
            CitySkyline => "The skyline",
            CityTourism => "Tourism",
            CityParks => "The parks",
            EconomyOverview => "The city's money",
            EconomyCashFlow => "Cash flow",
            EconomyLedger => "The ledger",
            EconomyMoneyFlow => "Where the money goes",
            EconomyTaxes => "Taxes",
            EconomyCorporations => "Mega-corporations and universities",
            MarketGoods => "Goods and prices",
            MarketBusinesses => "Businesses",
            MarketTrade => "Trade",
            PeoplePopulation => "Population",
            PeopleInequality => "Income and inequality",
            PeopleConcerns => "Complaints and concerns",
            PeopleSocial => "Social life",
            PeopleFavourites => "Favourites",
            ServicesPower => "Power",
            ServicesTransit => "Transit",
            ServicesEmergencies => "Emergencies",
            DirectoryBusinesses => "Every business",
            DirectoryPeople => "Everyone in town",
        }
    }

    /// One line under the title: what the page is for.
    pub fn blurb(self) -> &'static str {
        use Page::*;
        match self {
            CityOverview => "Homes, jobs, buildings and traffic over time, and the city's standing figures.",
            CityLand => "What land is worth, what homes rent for by tier, and how the lots are used.",
            CityDistricts => "Every district with its figures and policies. Pick one to edit it; the District tool (9) paints them.",
            CitySkyline => "The tallest buildings, the towers rising, the race for the city's tallest, and the height limit that holds it.",
            CityTourism => "Who visits, what draws them, where they sleep and what they spend.",
            CityParks => "Every park, how well kept it is and what wears it down: litter, dealers, people sleeping rough. Clean one up, or leave it wild.",
            EconomyOverview => "The treasury, where it is heading, and what the city's businesses want.",
            EconomyCashFlow => "Money in and out of the treasury day by day, by flow.",
            EconomyLedger => "Every line of the books, opened onto the buildings, lines and classes behind it.",
            EconomyMoneyFlow => "Every dollar in town moves from one account to another: rent to owners, wages, profits, taxes and trade, and what crosses the city limits.",
            EconomyTaxes => "What each tax brings in, and the rates: set them here.",
            EconomyCorporations => "Mega-corps with their logos, HQs, holdings and market share; universities; the cap and the levy.",
            MarketGoods => "Every good's price against its base and the stock in town.",
            MarketBusinesses => "How many businesses trade, how each kind pays, and the demand for new buildings.",
            MarketTrade => "Goods bought from and sold out of town by the city's businesses.",
            PeoplePopulation => "Who lives here: people, households, jobs and housing over time, and who was pushed out.",
            PeopleInequality => "How income is spread: the classes over time, the Gini index, the histogram and the Lorenz curve.",
            PeopleConcerns => "How happy people are and what they complain about, with where and what they said.",
            PeopleSocial => "Friendships and feuds, crews and clubs, petitions to you and the news.",
            PeopleFavourites => "The people you starred: fly to them, follow them, or send a gift.",
            ServicesPower => "Supply and demand hour by hour, the batteries, the grid's price, and every plant.",
            ServicesTransit => "Riders by mode and line, fares against upkeep; buses, rail, metro, taxis and cargo.",
            ServicesEmergencies => "Every 911 call, live and on record: response times, units, outcomes.",
            DirectoryBusinesses => "Every business in town: click a heading to sort, search or pick to narrow it, click a row to inspect it, go to fly there.",
            DirectoryPeople => "Every citizen: click a heading to sort, search or pick to narrow it, click a row to inspect them, go to fly there.",
        }
    }

    /// Whether the page charts history (and so shows the range picker).
    fn charts(self) -> bool {
        use Page::*;
        matches!(self, CityOverview | CityLand | EconomyOverview | EconomyCashFlow | EconomyLedger | EconomyTaxes | MarketBusinesses | MarketTrade | PeoplePopulation | PeopleInequality | ServicesPower | ServicesTransit | ServicesEmergencies)
    }

    /// A page by its tab and sub-tab keys; no sub-tab is the tab's last one.
    pub fn parse(ctx: Option<&egui::Context>, tab: &str, sub: Option<&str>) -> Result<Page, String> {
        let t = Tab::parse(tab).ok_or_else(|| format!("unknown tab {tab:?}; one of {}", Tab::ALL.map(|t| t.key()).join(", ")))?;
        match sub {
            None => Ok(ctx.map_or(t.pages()[0], |c| last_of(c, t))),
            Some(s) => {
                let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
                let s = match s.as_str() {
                    "cashflow" | "flows" => "cash_flow",
                    "budget" | "tax" => "taxes",
                    "money" | "sankey" | "flow" | "where_the_money_goes" => "money_flow",
                    "corps" | "corp" | "megacorps" | "mega_corps" | "universities" => "corporations",
                    "prices" | "goods_and_prices" => "goods",
                    "industry" => "businesses",
                    "imports" | "exports" => "trade",
                    "income" | "gini" => "inequality",
                    "complaints" | "happiness" => "concerns",
                    "favorites" => "favourites",
                    "land_and_rent" | "rent" => "land",
                    "911" => "emergencies",
                    "citizens" | "residents" | "population" if t == Tab::Directory => "people",
                    "shops" | "firms" | "companies" if t == Tab::Directory => "businesses",
                    other => other,
                };
                t.pages().iter().copied().find(|p| p.key() == s).ok_or_else(|| format!("{} has no sub-tab {s:?}; one of {}", t.key(), t.pages().iter().map(|p| p.key()).collect::<Vec<_>>().join(", ")))
            }
        }
    }

    /// What an old info window's name opens now: a whole tab (at its last
    /// sub-tab) or one page. `None` for names that are not data windows.
    pub fn from_window(name: &str) -> Option<Target> {
        let n = name.trim().to_ascii_lowercase().replace('_', " ");
        Some(match n.as_str() {
            "city" | "city data" | "data" => Target::Tab(Tab::City),
            "economy" => Target::Tab(Tab::Economy),
            "market" => Target::Tab(Tab::Market),
            "people" => Target::Tab(Tab::People),
            "services" => Target::Tab(Tab::Services),
            "directory" => Target::Tab(Tab::Directory),
            "taxes" => Target::Page(Page::EconomyTaxes),
            "power" => Target::Page(Page::ServicesPower),
            "transit" => Target::Page(Page::ServicesTransit),
            "emergencies" | "911" => Target::Page(Page::ServicesEmergencies),
            "districts" => Target::Page(Page::CityDistricts),
            "tourism" => Target::Page(Page::CityTourism),
            "parks" => Target::Page(Page::CityParks),
            "skyline" | "towers" => Target::Page(Page::CitySkyline),
            _ => return None,
        })
    }
}

/// Where a request points: a tab (its last sub-tab) or a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Tab(Tab),
    Page(Page),
}

impl Target {
    pub fn resolve(self, ctx: &egui::Context) -> Page {
        match self {
            Target::Tab(t) => last_of(ctx, t),
            Target::Page(p) => p,
        }
    }
}

// ── state, in egui memory ──────────────────────────────────────────────────

fn open_id() -> egui::Id {
    egui::Id::new("city_data_open")
}

fn page_id() -> egui::Id {
    egui::Id::new("city_data_page")
}

fn last_id(t: Tab) -> egui::Id {
    egui::Id::new(("city_data_last", t.key()))
}

pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

pub fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// The page showing (or that will show when the view opens).
pub fn page(ctx: &egui::Context) -> Page {
    ctx.data(|d| d.get_temp::<Page>(page_id())).unwrap_or(Page::CityOverview)
}

/// The tab's last page this session, or its first.
pub fn last_of(ctx: &egui::Context, t: Tab) -> Page {
    ctx.data(|d| d.get_temp::<Page>(last_id(t))).filter(|p| p.tab() == t).unwrap_or(t.pages()[0])
}

/// Show a page (without opening the view).
fn set_page(ctx: &egui::Context, p: Page) {
    ctx.data_mut(|d| {
        d.insert_temp(page_id(), p);
        d.insert_temp(last_id(p.tab()), p);
    });
}

fn scroll_id() -> egui::Id {
    egui::Id::new("city_data_scroll_to")
}

/// Scroll the page to `y` points from its top on the next pass (the API's
/// `city_data {scroll}`, for screenshots of long pages).
pub fn scroll_to(ctx: &egui::Context, y: f32) {
    ctx.data_mut(|d| d.insert_temp(scroll_id(), y));
}

/// Open the view at a page.
pub fn open_at(ctx: &egui::Context, p: Page) {
    set_page(ctx, p);
    set_open(ctx, true);
}

/// A button elsewhere in the HUD that opens a page; lit while it shows.
/// Clicked again while lit, it closes the view.
pub fn link(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, p: Page, tip: &str) -> egui::Response {
    let lit = is_open(ui.ctx()) && page(ui.ctx()) == p;
    let r = ui.selectable_label(lit, text).on_hover_text(tip);
    if r.clicked() {
        if lit {
            set_open(ui.ctx(), false);
        } else {
            open_at(ui.ctx(), p);
        }
    }
    r
}

// ── drawing ────────────────────────────────────────────────────────────────

/// The view's background: the windows' fill, opaque.
const BG: egui::Color32 = egui::Color32::from_rgb(8, 10, 20);
/// The tab rows' strip, a shade off the page.
const STRIP: egui::Color32 = egui::Color32::from_rgb(13, 15, 27);
/// The widest a page's content grows: charts wider than this read badly.
const MAX_WIDTH: f32 = 1280.0;

/// J toggles the view and Esc closes it, unless a text field has the keys.
/// Returns whether the view is open after them.
pub fn keys(ctx: &egui::Context) -> bool {
    if ctx.text_edit_focused() || ctx.egui_wants_keyboard_input() {
        return is_open(ctx);
    }
    let (j, esc) = ctx.input(|i| (i.key_pressed(egui::Key::J) && i.modifiers.is_none(), i.key_pressed(egui::Key::Escape)));
    if j {
        set_open(ctx, !is_open(ctx));
    } else if esc && is_open(ctx) {
        set_open(ctx, false);
    }
    is_open(ctx)
}

/// Draw the view over `content` (the map between the bars) when it is open.
/// Returns the rect it covers.
pub fn show(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) -> Option<egui::Rect> {
    if !is_open(ctx) {
        return None;
    }
    let mut page = page(ctx);
    let mut keep = true;
    let flew = p.fly_to.is_some();
    let followed = p.follow.target;
    let picked = *p.selection;
    // [pt-ux] Above every pane (they are on the middle layer).
    egui::Area::new(egui::Id::new("city_data_view"))
        .order(egui::Order::Foreground)
        .fixed_pos(content.min)
        .movable(false)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(content);
            ui.painter().rect_filled(content, 0.0, BG);
            ui.set_min_size(content.size());
            ui.set_max_size(content.size());
            // The two tab rows on their strip.
            let rows = egui::Frame::new().fill(STRIP).inner_margin(egui::Margin { left: 20, right: 16, top: 8, bottom: 0 }).show(ui, |ui| {
                ui.set_min_width(content.width() - 36.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("CITY DATA").font(crate::ui::theme::lead()).color(IVORY));
                    ui.add_space(18.0);
                    for t in Tab::ALL {
                        if tab_button(ui, t.label(), page.tab() == t).clicked() && page.tab() != t {
                            page = last_of(ctx, t);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if pane::close_button(ui).on_hover_text("Close (Esc or J)").clicked() {
                            keep = false;
                        }
                        ui.label(egui::RichText::new("Esc closes · J opens").small().color(pane::CAPTION));
                    });
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(2.0);
                    for &sp in page.tab().pages() {
                        if sub_button(ui, sp.label(), page == sp).clicked() {
                            page = sp;
                        }
                    }
                    if page.charts() {
                        ui.add_space(24.0);
                        pane::span_picker(ui, pane::DATA_SPAN);
                    }
                });
                ui.add_space(4.0);
            });
            let under = rows.response.rect.bottom();
            ui.painter().hline(content.x_range(), under, egui::Stroke::new(1.0, super::theme::EDGE));
            set_page(ctx, page);
            // The page, scrolling, centred when the screen is wider than a page.
            let body = egui::Rect::from_min_max(egui::pos2(content.left(), under + 1.0), content.max);
            let width = (body.width() - 40.0).min(MAX_WIDTH);
            let left = body.left() + ((body.width() - width) / 2.0).max(20.0);
            let col = egui::Rect::from_min_max(egui::pos2(left - 20.0, body.top()), egui::pos2(body.right(), body.bottom()));
            ui.scope_builder(egui::UiBuilder::new().max_rect(col), |ui| {
                let mut scroll = egui::ScrollArea::vertical().id_salt(("city_data_scroll", page.key(), page.tab().key())).auto_shrink([false, false]);
                if let Some(y) = ctx.data_mut(|d| d.remove_temp::<f32>(scroll_id())) {
                    scroll = scroll.vertical_scroll_offset(y);
                }
                scroll.show(ui, |ui| {
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.vertical(|ui| {
                            ui.set_max_width(width);
                            ui.set_min_width(width);
                            pane::page_head(ui, page.title(), page.blurb());
                            body_of(ui, p, page);
                            ui.add_space(24.0);
                        });
                    });
                });
            });
        });
    // A "go" pressed on the page: show the map, where the camera went.
    if (!flew && p.fly_to.is_some()) || (p.follow.target.is_some() && p.follow.target != followed) {
        keep = false;
    }
    // [pt-ux] A name picked on the page: show its inspector over the map
    // (the panes are below the view while it is open).
    if *p.selection != picked && *p.selection != crate::tools::Selection::None {
        keep = false;
    }
    if !keep {
        set_open(ctx, false);
    }
    Some(content)
}

/// A top-level tab: display face, heading size, a gold bar under the one on.
fn tab_button(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    let color = if on { GOLD } else { egui::Color32::from_gray(185) };
    let r = ui.add(egui::Button::new(egui::RichText::new(text).font(crate::ui::theme::heading()).color(color)).frame(false).min_size(egui::vec2(0.0, 30.0)));
    if on {
        let rect = r.rect;
        ui.painter().hline(rect.x_range(), rect.bottom() + 1.0, egui::Stroke::new(2.5, GOLD));
    } else if r.hovered() {
        ui.painter().hline(r.rect.x_range(), r.rect.bottom() + 1.0, egui::Stroke::new(1.0, GOLD.gamma_multiply(0.5)));
    }
    ui.add_space(10.0);
    r
}

/// A sub-tab: a selectable label in the button face.
fn sub_button(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    ui.add(egui::Button::selectable(on, egui::RichText::new(text).font(crate::ui::theme::button())))
}

fn body_of(ui: &mut egui::Ui, p: &mut UiCtx, page: Page) {
    use Page::*;
    match page {
        CityOverview => super::detail::city_overview(ui, p),
        CityLand => super::detail::city_land(ui, p),
        CityDistricts => super::districts::page(ui, p),
        CitySkyline => super::skyline::page(ui, p),
        CityTourism => super::districts::tourism_page(ui, p),
        CityParks => super::parks::page(ui, p),
        EconomyOverview => super::economy::overview(ui, p),
        EconomyCashFlow => super::economy::cash_flow(ui, p),
        EconomyLedger => super::economy::ledger(ui, p),
        EconomyMoneyFlow => super::ledger::page(ui, p),
        EconomyTaxes => super::economy::taxes(ui, p),
        EconomyCorporations => super::estate::corporations_page(ui, p),
        MarketGoods => super::detail::market_goods(ui, p),
        MarketBusinesses => super::detail::market_businesses(ui, p),
        MarketTrade => super::detail::market_trade(ui, p),
        PeoplePopulation => super::people::population(ui, p),
        PeopleInequality => super::people::inequality_page(ui, p),
        PeopleConcerns => super::concerns::tab(ui, p),
        PeopleSocial => super::social::tab(ui, p),
        PeopleFavourites => super::people::favourites_page(ui, p),
        ServicesPower => super::detail::power(ui, p),
        ServicesTransit => super::detail::transit(ui, p),
        ServicesEmergencies => super::emergencies::page(ui, p),
        DirectoryBusinesses => super::directory::businesses(ui, p),
        DirectoryPeople => super::directory::people(ui, p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every page is on exactly one tab, and keys are unique within a tab.
    #[test]
    fn pages_and_keys() {
        let mut seen = Vec::new();
        for t in Tab::ALL {
            let mut keys = Vec::new();
            for p in t.pages() {
                assert_eq!(p.tab(), t);
                assert!(!seen.contains(p), "{p:?} on two tabs");
                seen.push(*p);
                assert!(!keys.contains(&p.key()), "{} twice on {}", p.key(), t.key());
                keys.push(p.key());
                assert_eq!(Page::parse(None, t.key(), Some(p.key())), Ok(*p));
            }
        }
        assert_eq!(seen.len(), 25, "[parks] City, Parks added; [ledger] Economy, Money flow");
    }

    /// The old window names still find their pages.
    #[test]
    fn old_window_names() {
        assert_eq!(Page::from_window("economy"), Some(Target::Tab(Tab::Economy)));
        assert_eq!(Page::from_window("Taxes"), Some(Target::Page(Page::EconomyTaxes)));
        assert_eq!(Page::from_window("power"), Some(Target::Page(Page::ServicesPower)));
        assert_eq!(Page::from_window("districts"), Some(Target::Page(Page::CityDistricts)));
        assert_eq!(Page::from_window("lines"), None);
        assert_eq!(Page::parse(None, "economy", Some("cash-flow")), Ok(Page::EconomyCashFlow));
        assert!(Page::parse(None, "economy", Some("nope")).is_err());
    }
}
