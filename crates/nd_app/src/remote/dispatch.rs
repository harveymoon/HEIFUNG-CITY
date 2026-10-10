//! The command registry and the dispatcher. `COMMANDS` is the one list of
//! commands: each has its name, its other names, what it may change, its
//! help (`help` is generated from here) and its handler. `remote_commands`
//! drains the queue once a frame; `execute` looks a command up, asks for
//! `confirm` before one that replaces the city, and runs it.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

use bevy::prelude::*;
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::server::{Inbox, Reply, QUEUED, TAKEN};
use super::view::*;
use super::*;

/// A command's handler.
pub(super) type Handler = fn(&Value, &mut Ctx) -> Result<Value, String>;

/// What a command may change. Only `Replace` is acted on today (it asks
/// for `confirm`); the rest say what a read-only or restricted mode of
/// the API would have to refuse.
#[derive(Clone, Copy)]
pub(super) enum Effect {
    /// Reads only.
    Read,
    /// What the player sees, not the city: camera, windows, map layers,
    /// tools, the selection.
    View,
    /// Changes the city.
    Game,
    /// Writes files (saves, exports, screenshots, flags).
    Files,
    /// Dev and testing levers (money, contamination, staged incidents).
    Dev,
    /// May call the committee's model backend, which can cost money.
    Paid,
    /// May throw the running city away; the check says whether this call
    /// does, after validating what the command validates first.
    Replace(fn(&Value, &Ctx) -> Result<Replaces, String>),
}

/// Whether one call of a `Replace` command replaces the city.
pub(super) enum Replaces {
    /// It does not (e.g. `game_history` listing).
    No,
    /// It does: `confirm: true` is needed when the city has play time no
    /// save holds (`Sim::unsaved_play`).
    IfUnsaved,
    /// It does, and always needs `confirm: true`; the refusal says why.
    Always(String),
}

pub(super) struct Cmd {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub effect: Effect,
    pub run: Handler,
    /// Field name and what it means, as `help` lists them.
    pub help: &'static [(&'static str, &'static str)],
}

/// A command by its name or one of its aliases.
fn lookup(name: &str) -> Option<&'static Cmd> {
    static BY_NAME: OnceLock<HashMap<&'static str, &'static Cmd>> = OnceLock::new();
    BY_NAME
        .get_or_init(|| COMMANDS.iter().flat_map(|c| std::iter::once(c.name).chain(c.aliases.iter().copied()).map(move |n| (n, c))).collect())
        .get(name)
        .copied()
}

pub(super) fn remote_commands(inbox: Res<Inbox>, mut ctx: Ctx) {
    // Take the batch out first: the lock must not be held while commands run.
    for p in inbox.drain() {
        // Withdrawn after a 504: running it now would surprise a client
        // that was told it did not run.
        if p.state.compare_exchange(QUEUED, TAKEN, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            continue;
        }
        let reply = run_one(&p.cmd, &mut ctx);
        // The HTTP side may have timed out and gone; nothing to do then.
        let _ = p.reply.send(reply);
    }
}

/// Run one command and shape its reply.
pub(super) fn run_one(cmd: &Value, ctx: &mut Ctx) -> Reply {
    let result = execute(cmd, ctx);
    // [fix/review-app] A command that replaced the city: reset what named
    // the old one's ids now, before the next command in this batch reads
    // it (`city_reset` would only run later this frame).
    {
        let Ctx { sim, epoch, tools, selection, eplan, follow, fly, .. } = ctx;
        crate::city_reset::catch_up(sim.generation, epoch, tools, selection, eplan, follow, fly);
    }
    match result {
        Ok(mut body) => {
            if body.get("ok").is_none() {
                body["ok"] = json!(true);
            }
            Reply { status: 200, body }
        }
        Err(e) => Reply { status: 400, body: json!({ "ok": false, "error": e }) },
    }
}

fn execute(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let name = cmd.get("cmd").and_then(Value::as_str).ok_or("missing \"cmd\" field; try {\"cmd\": \"help\"}")?;
    let c = lookup(name).ok_or_else(|| format!("unknown command {name:?}; try {{\"cmd\": \"help\"}}"))?;
    if let Effect::Replace(check) = c.effect {
        confirm_replace(cmd, ctx, c.name, check)?;
    }
    (c.run)(cmd, ctx)
}

/// [fix/review-app] A command that throws the running city away asks for
/// `"confirm": true`, so an agent cannot wipe a player's game by accident:
/// `load`, `new_game` and `generate_city` when the city has play time no
/// save holds (a city just saved, just loaded, not yet run or untouched
/// needs none), `game_history {action: revert}` always. The command's own
/// argument checks come first.
fn confirm_replace(cmd: &Value, ctx: &Ctx, name: &str, check: fn(&Value, &Ctx) -> Result<Replaces, String>) -> Result<(), String> {
    let why = match check(cmd, ctx)? {
        Replaces::No => return Ok(()),
        Replaces::IfUnsaved if !ctx.sim.unsaved_play() => return Ok(()),
        Replaces::IfUnsaved => {
            let clock = &ctx.sim.world.clock;
            format!(
                "{name} replaces the running city (day {}, {}), which has play time no save holds; save it first, or send confirm: true to replace it anyway",
                clock.day() + 1,
                clock.format_hhmm()
            )
        }
        Replaces::Always(why) => why,
    };
    if opt_bool(cmd, "confirm")? == Some(true) {
        return Ok(());
    }
    Err(why)
}

/// [waterfront] `shore`, `beaches`, `jetties` and `waterfront_parcels`
/// share one handler, which goes by the name it was called by.
fn shore_command(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    shore::command(cmd.get("cmd").and_then(Value::as_str).unwrap_or(""), cmd, &mut ctx.sim.world)
}

use Effect::*;

pub(super) static COMMANDS: &[Cmd] = &[
    Cmd { name: "help", aliases: &[], effect: Read, run: |_, _| Ok(help::help()), help: &[] },
    Cmd { name: "status", aliases: &[], effect: Read, run: |_, ctx| Ok(status::status(ctx)), help: &[] },
    Cmd { name: "memory", aliases: &[], effect: Read, run: |_, ctx| Ok(status::memory_json(ctx)), help: &[
        ("note", "[memory] the process working set, peak and commit charge (MB), the renderer's mesh/material/image and entity counts, and the world's big counts"),
    ] },
    Cmd { name: "pause", aliases: &[], effect: Game, run: time::pause, help: &[
        ("paused", "bool, omitted toggles"),
    ] },
    Cmd { name: "speed", aliases: &[], effect: Game, run: time::speed, help: &[
        ("index", "0..5"),
        ("value", "multiple of real time for what moves (1 = people walk at their true pace; the clock runs 15x faster than that), snapped to the nearest step"),
    ] },
    Cmd { name: "step", aliases: &[], effect: Game, run: time::step, help: &[
        ("ticks", "advance n sim minutes now, even while paused (default 1, max 100000); the committee's round runs each new day, and a meeting it calls stops the step there (reply `stopped`)"),
    ] },
    Cmd { name: "seed_demo", aliases: &[], effect: Game, run: city::seed_demo, help: &[] },
    Cmd { name: "save", aliases: &[], effect: Files, run: files::save, help: &[
        ("name", "optional: save as a named game, saves/<name>.ron (file-safe) with a sidecar <name>.meta.json and a map picture <name>.png"),
        ("path", "optional; neither: writes saves/city.ron and a dated slot (with a picture)"),
    ] },
    Cmd { name: "city_name", aliases: &["city_identity"], effect: Game, run: identity::city_name, help: &[
        ("name", "optional: rename the city (tidied, at most 40 characters; empty leaves it unnamed)"),
        ("returns", "name, display_name, game_id (null for a legacy save until it is next saved), history_entries"),
    ] },
    Cmd { name: "game_history", aliases: &[], effect: Replace(identity::replaces), run: identity::game_history, help: &[
        ("action", "list (default)|bookmark|unmark|mark|revert"),
        ("game_id", "list: another game's history"),
        ("marked_only", "list: bool"),
        ("path", "bookmark/unmark/revert: an entry's path from list (or file: its file name)"),
        ("label", "bookmark/mark: the moment's name"),
        ("confirm", "revert: must be true (the running city is replaced, as Load)"),
        ("returns", "entries newest first: path, file, day, population, treasury, buildings, saved_unix, city, bookmark, thumbnail"),
    ] },
    Cmd { name: "load", aliases: &[], effect: Replace(files::load_replaces), run: files::load, help: &[
        ("path", "a .ron save (see files)"),
        ("confirm", "true to replace a city with play time no save holds (refused without it)"),
    ] },
    Cmd { name: "new_game", aliases: &[], effect: Replace(city::new_game_replaces), run: city::new_game, help: &[
        ("preset", "river_valley|islands|coast|delta|plains|flat (default river_valley)"),
        ("seed", "u64"),
        ("water", "0..1"),
        ("relief", "0..1 height variation"),
        ("branching", "0..1"),
        ("islands", "0..8"),
        ("climate", "temperate|desert|tropical|arctic|volcanic (default desert; iceland = volcanic)"),
        ("highway", "ignored: every new game has the highway and the trunk line"),
        ("confirm", "true to replace a city with play time no save holds (refused without it)"),
        ("note", "throws the city away; omitted levers take the preset's"),
    ] },
    Cmd { name: "generate_city", aliases: &[], effect: Replace(city::generate_city_replaces), run: city::generate_city, help: &[
        ("preset", "grid_plaza|coastal_river|old_town|twin_centres|perfect_city (default grid_plaza)"),
        ("seed", "u64 (default 1; perfect_city 5): the city and its landscape"),
        ("landscape", "optional landscape preset to lay it on instead of the city preset's own (river_valley|islands|coast|delta|plains|flat)"),
        ("water", "0..1"),
        ("relief", "0..1"),
        ("branching", "0..1"),
        ("climate", "temperate|desert|tropical|arctic|volcanic (default: the city preset's, desert)"),
        ("confirm", "true to replace a city with play time no save holds (refused without it)"),
        ("note", "throws the city away: a new game with a whole generated city (tensor-field streets, zoning, highway interchanges, a coal plant); returns the landscape summary and city {arterial, collector, local, zones, lots, interchanges, way_in, centres}"),
    ] },
    Cmd { name: "terrain_at", aliases: &[], effect: Read, run: |cmd, ctx| Ok(land::terrain_at(&ctx.sim.world, req_point(cmd)?)), help: &[
        ("x", "f"),
        ("y", "f"),
        ("returns", "natural/ground/surface heights, slope, water (kind, current, width), nearest shore, hot spot, power sites and dock placement for the lot there, survey (every land layer's value 0..1 there: resources, and the chance of each industry kind)"),
    ] },
    Cmd { name: "remediate", aliases: &[], effect: Game, run: city::remediate, help: &[
        ("x", "f"),
        ("y", "f"),
        ("radius", "m, default 100, at most 300"),
        ("note", "pay to dig out the contamination there at once (450 per cell of full contamination)"),
    ] },
    Cmd { name: "cleanup", aliases: &["clean_up"], effect: Game, run: city::cleanup, help: &[
        ("x", "f"),
        ("y", "f"),
        ("radius", "m, default 100, at most 300"),
        ("quote", "bool: only price it"),
        ("note", "the Bulldoze tool's Clean up ground: paid up front (450 per cell of full contamination), a crew digs it out from the middle over 2-6 days; returns the quote and the crews at work. tool {tool: bulldoze, kind: cleanup, radius, hover: [x, y]} shows the preview"),
    ] },
    Cmd { name: "contaminate", aliases: &[], effect: Dev, run: dev::contaminate, help: &[
        ("x", "f"),
        ("y", "f"),
        ("radius", "m, default 60"),
        ("amount", "0..1, default 0.6"),
        ("note", "testing: pour contamination onto the ground"),
    ] },
    Cmd { name: "paint_mural", aliases: &[], effect: Dev, run: dev::paint_mural, help: &[
        ("building", "id"),
        ("group", "crew id (default: the first street-art crew)"),
        ("quality", "-1..1, default 0.6"),
        ("seed", "u64"),
        ("note", "testing: a mural on that building's street face now"),
    ] },
    Cmd { name: "dirt_at", aliases: &["contamination_at"], effect: Read, run: city::dirt_at, help: &[
        ("x", "f"),
        ("y", "f"),
        ("returns", "contamination, dirty_nearby {plants [{building, name, what, distance_m, level, reach_m}], home_exposure, farm_exposure}"),
    ] },
    Cmd { name: "resource", aliases: &[], effect: Read, run: city::resource, help: &[
        ("x", "f"),
        ("y", "f (or z)"),
        ("returns", "wind (site_factor, mean_ms, now_ms, typical_yield, yield_now, cut_out_now) and solar (site_factor, climate_sun, sun, yield_now) at the point, and the climate"),
    ] },
    Cmd { name: "grid", aliases: &[], effect: Read, run: |_, ctx| Ok(land::grid_json(&ctx.sim.world)), help: &[
        ("returns", "time-of-use import/export price now, demand now vs the full draw, weather, day_ahead (24 h: hour, temp_c, demand_mw, import_price, export_price if the weather holds), last_24h (recorded demand, supply, imports, exports, price, temperature)"),
    ] },
    Cmd { name: "weather", aliases: &[], effect: Read, run: |_, ctx| Ok(land::weather_json(&ctx.sim.world)), help: &[
        ("returns", "label, cloud, wind (0..1), wind_ms, temp_c, spell (normal|heat wave|cold snap|storm), spell_hours_left, climate, turbine cut-in/rated/cut-out m/s"),
    ] },
    Cmd { name: "landscape", aliases: &[], effect: Read, run: |_, ctx| Ok(land::summary(&ctx.sim.world)), help: &[
        ("returns", "preset and levers, rivers (width, current, hydro_ok), lakes, sea, hot spots, highway ends and through cars, bridges, docks, ships"),
    ] },
    Cmd { name: "ships", aliases: &[], effect: Read, run: |_, ctx| Ok(economy::ships(&ctx.sim.world)), help: &[
        ("returns", "every ship and boat: freighters, ferries, marina boats and (fishing) trawlers with name, port, at, heading_deg, state (in_port|outbound|fishing|hauling|returning|unloading), at_sea, nets_out, fish_aboard, catch_today; count, trawlers_at_sea"),
    ] },
    Cmd { name: "set_weather", aliases: &[], effect: Dev, run: dev::set_weather, help: &[
        ("cloud", "0..1 (rain or snow above 0.6, by climate)"),
        ("wind", "0.05..1"),
        ("temp_c", "-50..55"),
        ("note", "dev: sets the sky now; it drifts again hour by hour"),
    ] },
    Cmd { name: "grant", aliases: &[], effect: Dev, run: dev::grant, help: &[
        ("amount", "money added to the treasury (dev), default 100000"),
    ] },
    Cmd { name: "walk_to", aliases: &[], effect: Dev, run: dev::walk_to, help: &[
        ("citizen", "id"),
        ("to", "[x,y]"),
        ("from", "[x,y] (default: where they are)"),
        ("note", "dev: sends the citizen out on foot by the walk graph (footbridges included)"),
    ] },
    Cmd { name: "list_saves", aliases: &[], effect: Read, run: |_, _| Ok(files::list_saves()), help: &[
        ("note", "newest first: path, name, kind (named|latest|autosave|dated), modified_unix, bytes, day, population, treasury, buildings, landscape (null for saves without a sidecar), thumbnail (absolute path of the PNG, or null), city (\"Unnamed city\" when none), game_id; by_city groups the paths by city name"),
    ] },
    Cmd { name: "plan", aliases: &[], effect: Read, run: |_, ctx| Ok(planning::plan(ctx)), help: &[] },
    Cmd { name: "add_road", aliases: &[], effect: Game, run: planning::add_road, help: &[
        ("points", "[[x,y] or [x,y,level],...] (2+; level metres above ground, negative below)"),
        ("elevation", "level for points without one (number), or a list"),
        ("tunnel", "bool: default level -8 m"),
        ("lanes_fwd", "0..4 (streets)"),
        ("lanes_bwd", "0..4"),
        ("kind", "street|path|rail"),
        ("curved", "bool"),
        ("snap", "bool, default true (a path end also snaps onto a park gate)"),
        ("from_junction", "path only: start on this grounds_junctions index"),
        ("to_junction", "path only: end on this index"),
        ("implement", "bool, default false (pending in the project)"),
        ("returns", "gesture_id, joined (path ends on a grounds gate), project state"),
    ] },
    Cmd { name: "grounds_junctions", aliases: &["park_junctions"], effect: Read, run: planning::grounds_junctions, help: &[
        ("lot", "optional lot id"),
        ("kind", "park|plaza|stadium|campus|station|beach"),
        ("near", "[x,y]"),
        ("radius", "m around near (default 150)"),
        ("returns", "junctions [{index, pos, kind, lot, attached, walk_node, gesture_id}] (the Path tool's snap rings) and links (paths joined to grounds)"),
    ] },
    Cmd { name: "add_roundabout", aliases: &[], effect: Game, run: planning::add_roundabout, help: &[
        ("center", "[x,y] (snaps onto a junction within 12 m or a street within 6 m)"),
        ("radius", "m to the ring centreline, 12..120 (or diameter 24..240)"),
        ("lanes", "1|2|3 ring lanes"),
        ("implement", "bool"),
        ("returns", "gesture_id, roundabout {joins, cuts_roads, takes_buildings}, project state"),
    ] },
    Cmd { name: "add_interchange", aliases: &["interchange"], effect: Game, run: planning::add_interchange, help: &[
        ("from", "[x,y]: one end of the cross street, on one side of the highway"),
        ("to", "[x,y]: the other end, across the highway"),
        ("lanes_fwd", "cross street lanes (default 2)"),
        ("lanes_bwd", "(default 2)"),
        ("check", "bool: only plan it, add nothing"),
        ("implement", "bool"),
        ("returns", "gesture_ids, interchange {kept_street, terminals, crossings, roads [{points, lanes}]}, or refused {why, at}; project state"),
    ] },
    Cmd { name: "add_zone", aliases: &[], effect: Game, run: planning::add_zone, help: &[
        ("polygon", "[[x,y],...] (3+)"),
        ("land_use", "residential|commercial|industrial|entertainment|civic|park"),
        ("implement", "bool"),
    ] },
    Cmd { name: "paint", aliases: &[], effect: Game, run: planning::paint, help: &[
        ("land_use", "residential|...|park, beach (a public beach: a park on parcels that run down to the water), or none to unzone"),
        ("at", "[x,y] brush centre"),
        ("radius", "m (default the tool's)"),
        ("industry", "industrial only: auto|farming|oil|refinery|manufacturing|logistics|fishing|tech|mining|smelting|factory_farming, the sub-zone set on implement"),
        ("rect", "[[x0,y0],[x1,y1]] marquee"),
        ("polygon", "[[x,y],...]"),
        ("implement", "bool"),
        ("note", "zones pre-cut parcels: brush = parcels the circle touches, rect/polygon = parcels whose centre is inside; parcels that refuse the use are listed with why; one call is one undo step"),
    ] },
    Cmd { name: "bulldoze", aliases: &[], effect: Game, run: planning::bulldoze, help: &[
        ("gesture_id", "int"),
        ("implement", "bool"),
        ("building", "or a city building's id: it goes at once, its ground back to the zoning plan"),
    ] },
    Cmd { name: "project", aliases: &[], effect: Game, run: planning::project, help: &[
        ("action", "list|new|switch|rename|delete|park|start|implement|undo|redo|discard"),
        ("id", "project id (default: the active one)"),
        ("name", "for new/rename, or to pick a project by name"),
        ("note", "several named projects; the active one is edited, the rest previewed dim; implement builds one at a time, the later one wins where two change the same gesture or parcel (warnings in the reply)"),
    ] },
    Cmd { name: "place", aliases: &[], effect: Game, run: place::place, help: &[
        ("kind", "toll_booth|toll_plaza (on a road: across the carriageway)|station|freight_yard|depot|parking|coal|solar|wind|nuclear|hydro|tidal|geothermal|shipping_port|ferry_terminal|police|fire|hospital|clinic|school|dispatch|monument|stadium|jail|trash_depot (services: tier or kind:tier)"),
        ("tier", "low|mid|high"),
        ("x", "f"),
        ("y", "f"),
        ("note", "hydro beside a river or stream with a current >= 0.8 m/s, tidal on the sea shore, geothermal over a hot spot; docks at the water's edge (they build a pier and a street stub)"),
    ] },
    Cmd { name: "plant_preview", aliases: &[], effect: Read, run: place::plant_preview, help: &[
        ("kind", "solar|wind|coal|..."),
        ("x", "f"),
        ("y", "f"),
        ("note", "read-only: MW, cost and upkeep on the site a click there takes (a lot, or unzoned ground joined into one), the Sunfield / Wind Park it would join and what it adds, and any problem"),
    ] },
    Cmd { name: "taxes", aliases: &["tax"], effect: Game, run: |cmd, ctx| taxes::command(cmd, &mut ctx.sim.world), help: &[
        ("set", "{line: rate, ...} where line is a key (residential_low|residential_mid|residential_high|grocery|bakery|gadget_shop|parking|nightlife_low|nightlife_mid|nightlife_high|farming|oil|refinery|manufacturing|logistics|fishing|tech) or a land use for its whole group; rate 0.2 or 20 (= 20%)"),
        ("category", "one line (with rate)"),
        ("import_duty", "share of import prices (0..1)"),
        ("reset", "bool: back to the standard rates"),
        ("returns", "every line: rate, default, pull, buildings, homes/jobs, today, yesterday, recent_per_day; by_land_use; duty"),
    ] },
    Cmd { name: "tolls", aliases: &["toll"], effect: Game, run: |cmd, ctx| taxes::tolls(cmd, &mut ctx.sim.world), help: &[
        ("action", "list (default) | price | remove | inspect"),
        ("id", "booth id (or at: [x,y])"),
        ("price", "toll per car (lorries pay double)"),
        ("returns", "booths with cars and revenue today/yesterday/total, paid/free today, upkeep"),
    ] },
    Cmd { name: "incident", aliases: &[], effect: Dev, run: incidents::incident, help: &[
        ("kind", "fire|crime|medical|crash|takeover|chase"),
        ("building", "id (fire, crime; medical without citizen)"),
        ("intensity", "fire, dev: start it this big (0..1; one engine knocks off 0.012 a minute)"),
        ("citizen", "id (medical)"),
        ("car", "id (crash; default the car nearest at)"),
        ("at", "[x,y] (crash, takeover: the junction nearest)"),
        ("speed", "m/s (crash, default 25 = written off)"),
    ] },
    Cmd { name: "incidents", aliases: &[], effect: Read, run: incidents::incidents, help: &[
        ("active", "bool (only those under way)"),
        ("limit", "default 50, newest first"),
        ("kind", "fire|crime|medical|crash"),
    ] },
    Cmd { name: "units", aliases: &[], effect: Read, run: |cmd, ctx| Ok(units::units_list(&ctx.sim.world, opt_str(cmd, "kind")?, opt_bool(cmd, "out")?.unwrap_or(false), ctx.sim.frac())), help: &[
        ("kind", "optional: fire_engine|police_car|ambulance (or Engine|Car|Medic)"),
        ("out", "bool: only units out of the station"),
        ("returns", "units: unit id, title, kind, state, doing, pos, incident; select {unit} / inspect {unit} take the id"),
    ] },
    Cmd { name: "society", aliases: &[], effect: Game, run: |cmd, ctx| society::command(cmd, &mut ctx.sim.world), help: &[
        ("action", "report (default)|evict|offend|trash"),
        ("building", "id (evict: its households go on the street; offend: the target; trash: one building's bins)"),
        ("citizen", "id (offend)"),
        ("kind", "theft|burglary|assault|murder|vandalism (offend, with building)"),
        ("kg", "trash to add, default 100"),
    ] },
    Cmd { name: "social", aliases: &[], effect: Read, run: |cmd, ctx| social::command(cmd, &ctx.sim.world), help: &[
        ("days", "daily history to include (default 7)"),
        ("talks", "latest conversations to list (default 10)"),
        ("group", "optional group id: that group in full, with its members"),
        ("note", "people talking: today's and yesterday's conversations, friendships, feuds, fights, meet-ups, jobs and flats through friends, couples moving in; pairs by bond city-wide; every crew and club with momentum, reputation, base and works (gigs, matches, readings, murals, gardens, businesses, petitions, protests); petitions to the player; murals, gardens, gigs; news. inspect {citizen} has a social block (ties with bond and how they met, groups, recent conversations)"),
    ] },
    Cmd { name: "street_events", aliases: &["nights"], effect: Read, run: |cmd, ctx| nights::command(cmd, &ctx.sim.world), help: &[
        ("limit", "rows of history and news (default 10)"),
        ("fresh", "bool: weigh the warehouses for a rave now (else the last 22:00 roll's)"),
        ("returns", "race (stage, meet, finish, route_m, racers with place and speed, crowd, incident, arrests, fines), race_planned, race_crew, rave (place, door, guests, queue, queue_at_door, inside, peak, complaints, incident, pull), rave_planned, rave_spots (warehouses with youth, artists, art_scene, neighbours, culture, has_scene, night_chance, why), history, news, totals"),
    ] },
    Cmd { name: "race", aliases: &[], effect: Game, run: |cmd, ctx| nights::race(cmd, &mut ctx.sim.world), help: &[
        ("at", "[x,y] optional: meet at the empty lot or surface car park nearest it"),
        ("returns", "race id and detail; errors if fewer than two locals with a fast car are free, or no lot pair 0.9-2.4 km apart"),
    ] },
    Cmd { name: "rave", aliases: &[], effect: Game, run: |cmd, ctx| nights::rave(cmd, &mut ctx.sim.world), help: &[
        ("building", "id optional: the warehouse"),
        ("at", "[x,y] optional: the old warehouse nearest it"),
        ("returns", "rave id and detail (with no field: the warehouse with the strongest scene)"),
    ] },
    Cmd { name: "roads", aliases: &["road_condition", "road_budget"], effect: Game, run: |cmd, ctx| nights::roads(cmd, &mut ctx.sim.world), help: &[
        ("funding", "optional 0..1.5: road maintenance funding level (1 = what the network needs to stand still)"),
        ("age_days", "dev, optional: wear every street that many days at yesterday's traffic (the crews work each night unless crews: false; nothing is charged)"),
        ("near", "[x,y] optional: list the pieces nearest it"),
        ("limit", "pieces listed (default 10, worst first)"),
        ("returns", "pieces, lane_km, mean_wear, worn, crumbling, waiting_for_repair, mean_wear_by_land_value {cheap, middle, dear}, budget {funding, need_per_day, allowance_per_day, spent_yesterday, repaired_yesterday, repaired_land_value_yesterday, ledger_today/yesterday, repaired_total}, days, pieces_listed (wear, condition, label, speed_factor, crash_factor, land_value, repair_cost, queue_place, street)"),
    ] },
    Cmd { name: "repair_road", aliases: &[], effect: Game, run: road::repair_road, help: &[
        ("road", "id or [x,y]; default the selected road"),
        ("returns", "repaired, cost, treasury, road (the card); pays the crews' price (lane-m x wear x 6) from the treasury as road maintenance"),
    ] },
    Cmd { name: "rebuild_roads", aliases: &[], effect: Dev, run: dev::rebuild_roads, help: &[
        ("returns", "segments_before, segments, roads_version"),
        ("note", "dev: lays the roads, lots and lanes again from the plan as an implement does, with the current road-building rules (nothing in the plan changes)"),
    ] },
    Cmd { name: "district", aliases: &["districts"], effect: Game, run: |cmd, ctx| districts::district(cmd, &mut ctx.sim.world, &mut ctx.overlays, &mut ctx.tools), help: &[
        ("action", "list (default)|create|paint|erase|set|remove|at|select|layer"),
        ("id", "district id (or district: id or name)"),
        ("at", "[x,y] brush centre"),
        ("radius", "m, default 40"),
        ("polygon", "[[x,y],...]"),
        ("rect", "[[x0,y0],[x1,y1]]"),
        ("residential", "kowloon|suburban|eco|none"),
        ("commercial", "neon_strip|tech_campus|night_market|tourist|none"),
        ("industry", "farming|oil|refinery|manufacturing|logistics|fishing|tech|none"),
        ("historic", "bool"),
        ("height_limit", "metres (null or 0 lifts it): nothing new rises over it in the district"),
        ("rename", "new name"),
        ("on", "bool (layer)"),
    ] },
    Cmd { name: "tourism", aliases: &["tourists"], effect: Game, run: |cmd, ctx| districts::tourism(cmd, &mut ctx.sim.world), help: &[
        ("action", "refresh (re-read attractions now)"),
        ("limit", "tourists listed, default 20"),
        ("returns", "attractiveness, demand, ways in, today's and yesterday's arrivals, spending and tourist tax, hotels, attractions, tourists"),
    ] },
    Cmd { name: "concerns", aliases: &[], effect: Read, run: |cmd, ctx| concerns::command(cmd, &ctx.sim.world), help: &[
        ("days", "daily counts and happiness over the last n days (default 30)"),
        ("concern", "optional one of housing|food|jobs|safety|trash|noise|commute|parking|power|services|nightlife|displacement|other"),
        ("quotes", "recent quotes per concern (default 5)"),
        ("note", "what people complain about, ranked by people this week with the trend against last week, where it comes from, and city happiness (daily, hourly for 48 h)"),
    ] },
    Cmd { name: "committee", aliases: &[], effect: Paid, run: |cmd, ctx| committee::command(cmd, &mut ctx.sim, &mut ctx.committee), help: &[
        ("action", "state (default)|members|appoint {id, seat?}|dismiss {id}|alerts {limit, mark_seen}|check_alerts|projects|implement {project}|dismiss_project {project}|pick_site {project, option}|refresh_sites {project}|report|minutes {meeting?}|settings {backend: mock|local|claude, local_url, local_model, claude_model, claude_effort, claude_fallbacks, timeout_s, auto_meetings, veto_allowed, save, test}"),
        ("note", "the mayor's AI committee (docs/AI_COMMITTEE.md): fictional politicians, six seats, alerts, projects; never takes or returns an API key; the backend fields need the player's leave (the Committee window's settings)"),
    ] },
    Cmd { name: "committee_meeting", aliases: &["meeting"], effect: Paid, run: |cmd, ctx| committee::meeting_command(cmd, &mut ctx.sim, &mut ctx.committee), help: &[
        ("start", "bool: call a meeting now (pauses the sim)"),
        ("vote", "{proposal, vote: yes|no|abstain|veto} (the mayor's vote)"),
        ("abstain_rest", "bool"),
        ("fast_forward", "bool: show all; with the stand-in, run to the mayor's vote or the end now"),
        ("transcript_from", "line index"),
        ("returns", "the meeting in session (stage, transcript, proposals with votes and tally) or the last minutes"),
    ] },
    Cmd { name: "diary", aliases: &[], effect: Paid, run: |cmd, ctx| committee::diary(cmd, &mut ctx.sim, &mut ctx.committee), help: &[
        ("citizen", "id"),
        ("refresh", "bool: write it again"),
        ("returns", "day, text, pending (a model still writing: call again), by_model, backend; display only, never in the save"),
    ] },
    Cmd { name: "voices", aliases: &[], effect: Read, run: |cmd, ctx| committee::voices(cmd, &ctx.sim, &ctx.committee), help: &[
        ("limit", "default 20"),
        ("returns", "conversations of spotlit or favourite citizens that got a model's line (template and line), lines_written, diaries_written, backend"),
    ] },
    Cmd { name: "estate", aliases: &["land_market"], effect: Game, run: |cmd, ctx| property::command(cmd, &mut ctx.sim.world), help: &[
        ("action", "summary (default: counters, floors histogram, the billionaire gate, elites with their convoys, NIMBY zones, forms, owners) | pressure ({at: [x,y]} for one point, else every busy cell) | spawn_elite (dev: a billionaire now, needs quiet land) | expand {building} (dev: branch or merge now) | buyout {building} (dev) | drive {citizen, to} (dev)"),
    ] },
    Cmd { name: "ownership", aliases: &[], effect: Read, run: |cmd, ctx| property::ownership(cmd, &ctx.sim.world), help: &[
        ("building", "id: what to highlight when it is selected: the owner and every property with its outline (a rich person's other homes and businesses too)"),
    ] },
    Cmd { name: "corps", aliases: &["corp", "megacorp"], effect: Game, run: |cmd, ctx| corp::command(cmd, &mut ctx.sim.world), help: &[
        ("action", "summary (default: corps with brand/logo, HQ, holdings by use and industry, staff, housing, market share; campuses; rules; biggest companies vs thresholds) | rules {allowed, max_share 0.02-0.25, levy 0-0.2} | promote {building} (dev: its company becomes a mega-corp now) | buy {corp|building, n} (dev) | finish {corp} (dev: HQ now, sites finished) | drive {corp, lot} (road metres from HQ)"),
    ] },
    Cmd { name: "campus", aliases: &["university"], effect: Game, run: |cmd, ctx| corp::campus(cmd, &mut ctx.sim.world), help: &[
        ("action", "summary (default) | grow {building, days} (dev: run the campus's days with one annex each, sites finished)"),
    ] },
    Cmd { name: "skyline", aliases: &["towers", "height_limit"], effect: Game, run: |cmd, ctx| skyline::command(cmd, &mut ctx.sim.world), help: &[
        ("action", "summary (default) | limit | sites | start | finish | fund"),
        ("limit", "limit: the city's height limit in metres, null or 0 for none (also cmd height_limit {limit})"),
        ("vertiport_clearance", "limit: bool, keep the approach round the player's vertiports clear (towers there held to 120 m)"),
        ("height", "start: metres (else the developer decides: over the record when demand and money allow)"),
        ("design", "start: deco_spire|bundled|twist|needle"),
        ("at", "start: [x,y] build as near here as a site allows"),
        ("force", "start (dev, default true): skip the money, demand, population and downtown tests (height limits and site rules still hold); sites: list every site that passes the limits"),
        ("rich", "start: give the developers $2B first"),
        ("amount", "fund (dev): dollars to every tower developer (default 1e9)"),
        ("limit_rows", "rows of the ranking and sites (default 15)"),
        ("returns", "summary: limit_m, vertiport_clearance, classes_m, record_m, tallest_planned_m, population_cap_m, demand, classes (standing/rising), towers (id, name, height_m, league, design, developer, lots, pos, under_construction, progress, rank, homes, jobs), ranking (the tallest standing), records, news, developers (name, capital), totals; inspect {building} has a tower block (height_m, rank, rank_label, height_limit_m; for a league tower league, design, developer, lots, pending_lots, site_m2, floors, started_day, topped_out_day, record_bid, progress, days_left)"),
    ] },
    Cmd { name: "pressure", aliases: &["pressures"], effect: Game, run: |cmd, ctx| economy::command(cmd, &mut ctx.sim.world), help: &[
        ("note", "the pressures report section (what the city lacks, by kind)"),
        ("open", "optional: a business kind key (mechanic, car_dealer, golf_range, ...) to open on the vacant lot at `at`"),
        ("at", "[x, y] with open"),
        ("finish", "bool: built at once"),
    ] },
    Cmd { name: "shore", aliases: &["waterfront"], effect: Read, run: shore_command, help: &[
        ("returns", "[waterfront] waterfront lots (vacant ones), their frontage, public beaches (open, bathers now), private jetties, the waterfront trades open (beach resorts, pier restaurants, beach bars, boat rentals, waterfront hotels) and their pressures; whether parcels reach the water"),
    ] },
    Cmd { name: "beaches", aliases: &[], effect: Read, run: shore_command, help: &[
        ("returns", "[waterfront] every public beach: access, open (fouled water closes it), note, visitors, swimmers, people_here (the sim's own), draw, capacity, crowded, catchment, appeal (weather and hour), contamination, today's peak and visits"),
    ] },
    Cmd { name: "jetties", aliases: &[], effect: Read, run: shore_command, help: &[
        ("returns", "[waterfront] every private jetty: owner, kind (fishing jetty, cargo quay, restaurant pier, boat pontoon), root, tip, berth, water, loads landed and coming"),
    ] },
    Cmd { name: "waterfront_parcels", aliases: &[], effect: Game, run: shore_command, help: &[
        ("on", "bool, omitted reads: parcels along a road near the water run down to it (on for new cities; a save from before has it off; lots follow at the next implement)"),
    ] },
    Cmd { name: "bus", aliases: &[], effect: Game, run: bus::bus, help: &[
        ("action", "select|new_route|add_stop|move_stop|remove_stop|set_buses|remove_route|rename|recolour|line|select_stop|list"),
        ("route", "id"),
        ("x", "f"),
        ("y", "f"),
        ("index", "stop index"),
        ("n", "buses"),
        ("name", "rename"),
        ("color", "0..7"),
        ("look", "select_stop: point the camera"),
    ] },
    Cmd { name: "subway", aliases: &["metro"], effect: Game, run: |cmd, ctx| subway::command(cmd, &mut ctx.sim.world), help: &[
        ("action", "status (default)|station|line|preview|headway|remove_line|inspect_line|inspect_station"),
        ("x", "f (station)"),
        ("y", "f (station)"),
        ("stations", "[id or [x,y], ...] (line, preview)"),
        ("points", "[[x,y], ...] (line: places stations where none is near)"),
        ("loop", "bool (line, preview): back to the first station"),
        ("line", "id"),
        ("minutes", "headway"),
        ("headway", "minutes (line)"),
        ("station", "id or [x,y]"),
    ] },
    Cmd { name: "metro_line", aliases: &[], effect: Game, run: |cmd, ctx| subway::metro_line(cmd, &mut ctx.sim.world, &mut ctx.tools), help: &[
        ("stations", "[id or [x,y], ...]: clicked in order, as the player does"),
        ("click", "id or [x,y]: one more"),
        ("loop", "bool: click the first station again"),
        ("undo", "bool or n: take back stops"),
        ("finish", "bool: lay the line"),
        ("cancel", "bool: drop the draft first"),
        ("headway", "minutes (finish)"),
        ("tool", "bool (default true): pick the metro line tool, which shows the Metro layer"),
    ] },
    Cmd { name: "find", aliases: &[], effect: Read, run: inspect::find, help: &[
        ("what", "citizens|buildings|lots|parcels|cars|trains|drones|routes|gestures"),
        ("near", "[x,y]"),
        ("radius", "m (default 100 with near)"),
        ("filter", "object; citizens: activity,name,class,visible,spotlight; buildings: land_use,tier,business_kind,name,under_construction,plant,rail,depot,derelict; lots: land_use,vacant,park,min_area; cars/trains: kind"),
        ("limit", "default 50"),
    ] },
    Cmd { name: "inspect", aliases: &[], effect: Read, run: inspect::inspect, help: &[
        ("citizen|building|lot|car|train|drone|vehicle|route", "id (vehicle: a private car's record; a unit's car also gets unit)"),
        ("unit", "id: an emergency unit: title, kind, station, state, doing, crew (citizen, name, role, out, pos, pose), incident (fire: how_bad, intensity, minutes_to_out, progress, damage; else patient, suspect, people), and for a fire engine at work scene (seconds, water_litres, aim, ladder {yaw_deg, pitch_deg, length, outriggers, tip}, hoses, streams)"),
        ("gesture", "int; with no id, the current selection"),
        ("junction", "[x,y]: the nearest junction's signal, lane arrows per arriving street and left turn waits"),
        ("segment", "id: a road piece: level (ground, viaduct, cutting, tunnel, bridge), levels, heights, flags, the ways it passes over or under"),
        ("road", "id, or [x,y]: the piece a click there picks (else the nearest), as segment, plus road: the road card (kind, lanes, limit, level, run, now {vehicles by kind, bikes, walkers, speed, congestion, queue}, flow, hourly (72 h), decay {wear, condition, effects, wear_per_day, next_condition, repair_cost, queue_place, last_repaired_day}, tolls, crossings, bus_stops, kerb parking)"),
        ("toll", "int: a toll booth in full (traffic, lorries, free, avoided, queue, hourly)"),
        ("ship", "id (landscape.ships): a ferry or freighter: title, doing, passengers, cargo, route; a ferry terminal or shipping port building also gets routes (to, ships, headway_min, next_departure, passengers_today)"),
    ] },
    Cmd { name: "lane_use", aliases: &[], effect: Game, run: junction::lane_use, help: &[
        ("junction", "[x,y]"),
        ("approach", "[x,y] on the arriving street"),
        ("lanes", "[\"L\", \"S+R\", ...] leftmost first (L, S, R joined with +), or \"reset\" for the demand-chosen default"),
    ] },
    Cmd { name: "intersection", aliases: &[], effect: Game, run: |cmd, ctx| junction::command(cmd, &mut ctx.sim.world, &mut ctx.tools), help: &[
        ("at", "[x,y] near a junction or crosswalk (default: the editor's)"),
        ("signal", "auto|signals|none"),
        ("greens", "[seconds per phase, ...] (phases as listed) or \"default\""),
        ("left_arrow", "auto|always|never"),
        ("arrow_seconds", "3..30"),
        ("left_turns", "bool: allow (true) or forbid every left"),
        ("no_left", "[approach index or [x,y], ...]: approaches with the left forbidden (the rest allowed)"),
        ("approach", "index or [x,y] (with lanes)"),
        ("lanes", "[\"L\", \"S+R\", ...] leftmost first, or \"reset\""),
        ("crossing", "index or [x,y]"),
        ("upgrade", "none|button|footbridge (with crossing; a footbridge replaces the crosswalk under it)"),
        ("keep_crosswalk", "bool (with crossing): keep the crosswalk at grade under its footbridge (default false: the bridge replaces it)"),
        ("open", "bool: show the Intersection editor modal"),
        ("close", "bool"),
        ("select_approach", "index (open)"),
        ("select_crossing", "walk crossing index (open)"),
        ("returns", "intersection: at, signalized, rule, approaches (lanes, offered, left_forbidden), phases (green_s, running), crossings (upgrade, signalised, button_light, crosswalk_kept, crosswalk_removed); done; editor_open"),
    ] },
    Cmd { name: "level_crossings", aliases: &["level_crossing", "crossings", "rail_crossings"], effect: Read, run: |cmd, ctx| crossings::command(cmd, &ctx.sim.world), help: &[
        ("at", "[x,y]: the nearest level crossing (within radius, default 60 m)"),
        ("index", "int: that crossing"),
        ("returns", "count, shut_now, crossings: index, node, at, guard (barriers|lights|footpath gates: barriers on streets of 3+ lanes), phase (open|warning|lowering|closed|raising), lights_flashing, closed_to_traffic, barrier_down (0 up..1 down), train_over, eta_s, trains_coming [{train, seconds}], vehicles_waiting, people_waiting, trains_last_hour, trains_per_hour, trains_total, closures, closure_s, average_closure_s, vehicles_held, average_wait_s, trains_held, arms [{segment, away, lanes_in, lanes_out, stop_line_m, barrier, barrier_length_m}]"),
    ] },
    Cmd { name: "tow_car", aliases: &["tow", "boot"], effect: Game, run: city::tow_car, help: &[
        ("car", "a car id on the road (it pulls over first)"),
        ("vehicle", "or a vehicle record id (parked or driving)"),
        ("returns", "tow_job, vehicle, company (null: an out-of-town firm)"),
    ] },
    Cmd { name: "parking_price", aliases: &[], effect: Game, run: city::parking_price, help: &[
        ("building", "id of a parking lot the city runs"),
        ("price", "the fee, 0..50"),
        ("returns", "building, price (as set, clamped)"),
    ] },
    Cmd { name: "select", aliases: &[], effect: View, run: inspect::select, help: &[
        ("citizen|building|car|vehicle|bot", "id (vehicle: a parked car; bot: a delivery robot)"),
        ("unit", "id: an emergency unit (fire engine, police car, ambulance) driving or standing on scene (see units); a click on one picks it the same way"),
        ("toll", "int: a toll booth"),
        ("road|segment", "id or [x,y]: a street piece"),
        ("ship", "id: a ferry or freighter (landscape.ships)"),
        ("at", "[x,y] picks like a click (vehicles, people, booths, parked cars, then the road piece, then lots); nothing clears"),
    ] },
    Cmd { name: "selected", aliases: &[], effect: Read, run: |_, ctx| Ok(inspect::selected_detail(&ctx.sim, *ctx.selection, ctx.tools.route_sel, &ctx.assets, &ctx.tools.favorites)), help: &[
        ("", "full detail of what the player has selected: a car with why (reason, text, by), jam (chain to the head of the queue, head_reason, cycle), trip (elapsed_min, expected_min, anger), lanes, driver, cargo; a citizen (and driving_car); a building; a parked car; the bus line picked (bus_line)"),
    ] },
    Cmd { name: "flags", aliases: &[], effect: View, run: flags::flags, help: &[
        ("since", "flag id: only newer ones"),
        ("clear", "bool: empty the list after reading"),
    ] },
    Cmd { name: "flag", aliases: &[], effect: Files, run: flags::flag, help: &[
        ("note", "text: flag the current selection, as F8 does in game"),
    ] },
    Cmd { name: "favourite", aliases: &["favorite"], effect: View, run: flags::favourite, help: &[
        ("citizen", "id"),
        ("on", "bool, omitted toggles"),
    ] },
    Cmd { name: "tool", aliases: &[], effect: View, run: tool::tool, help: &[
        ("name", "inspect|road|zone|edit|bulldoze|power|transit|rail"),
        ("preset", "Path|Rail|Lane|Street|Avenue|One-way|Ramp|Highway|Tunnel|Roundabout (Tunnel switches the view underground, the others back to ground)"),
        ("elevation", "road tool level for the next point, m (-20..24)"),
        ("depth", "underground|ground|sky: the view layer"),
        ("draw", "[[x,y,level?],...]: the road being drawn, for its preview"),
        ("cursor", "[x,y]: where the pointer stands for the road preview (until the mouse is used)"),
        ("diameter", "roundabout 24..240 m"),
        ("ring_lanes", "roundabout 1|2|3"),
        ("ring_at", "[x,y]: show the roundabout preview there as if hovered (until the mouse is used)"),
        ("kind", "sub-mode of the active tool"),
        ("mode", "zone tool: polygon|brush|marquee"),
        ("radius", "zone brush radius m"),
        ("unzone", "bool"),
        ("industry", "zone tool sub-zone for industrial land: auto|farming|oil|..."),
        ("land_use", "(or none: the brush unzones)"),
        ("lanes_fwd", ""),
        ("lanes_bwd", ""),
        ("curved", ""),
        ("show_routes", ""),
        ("route", "id"),
    ] },
    Cmd { name: "camera", aliases: &[], effect: View, run: camera::camera, help: &[
        ("look_at", "[x,y]"),
        ("x", ""),
        ("z", ""),
        ("distance", "12..2500"),
        ("yaw", "rad"),
        ("pitch", "rad"),
        ("follow", "citizen/car id or {citizen|car|bot|building: id}; any camera command ends a follow"),
        ("mode", "orbit|walk"),
    ] },
    Cmd { name: "follow", aliases: &[], effect: View, run: follow::follow, help: &[
        ("citizen|car|bot|building|vehicle", "id, or the selection"),
        ("stop", "bool: end the follow (also on: false)"),
        ("pov", "bool: look through their eyes (Walk view) or back to orbit"),
        ("fly", "bool, default true: fly in smoothly first"),
        ("note", "the camera stays on it (pan locked, rotate and zoom free) until stopped, Esc, or it is gone"),
    ] },
    Cmd { name: "zoom_to", aliases: &["zoom"], effect: View, run: follow::zoom_to, help: &[
        ("citizen|car|bot|building|vehicle", "id, or the selection"),
        ("at", "[x,y] instead of a thing"),
        ("distance", "m (default by kind)"),
        ("pitch", "rad (default 0.62)"),
        ("note", "a smooth move of 0.7..2.2 s; replies at once"),
    ] },
    Cmd { name: "gift", aliases: &[], effect: Game, run: follow::gift, help: &[
        ("citizen", "id, or the selected citizen"),
        ("amount", "default 1000"),
        ("note", "from the treasury to their household (booked as gifts to residents)"),
    ] },
    Cmd { name: "overlay", aliases: &[], effect: View, run: look::overlay, help: &[
        ("active", "the map layer to show, or none (layers are exclusive: showing one hides the others)"),
        ("layer", "zoning|land_value|districts|traffic|traffic_types|paths|people|pollution, a land survey layer (farmland|oil|geothermal|rock|sand|water|shipping|best_industry|best_farming|best_oil|best_refinery|best_manufacturing|best_logistics|best_fishing|best_tech), or projects (the dim other-projects switch, not a layer)"),
        ("on", "bool, omitted toggles; on hides every other layer"),
        ("land", "a land survey layer key, or none"),
        ("<layer name>", "bool (older form): true shows it alone, false hides it if it shows"),
    ] },
    Cmd { name: "lighting", aliases: &[], effect: View, run: look::lighting, help: &[
        ("follow_clock", "bool"),
        ("hour", "0..24 (also unpins from the clock)"),
    ] },
    Cmd { name: "heat_vision", aliases: &["heat"], effect: View, run: look::heat_vision, help: &[
        ("on", "bool, omitted toggles: the drone thermal camera view (also key H)"),
    ] },
    Cmd { name: "graphics", aliases: &["quality"], effect: View, run: look::graphics, help: &[
        ("quality", "auto|low|medium|high: pin the graphics tier (pin one before screenshots that must compare) or hand it back to the frame-rate throttler; omitted reports the setting, the tier in force, the smoothed frame time and the effects on the camera"),
    ] },
    Cmd { name: "screenshot", aliases: &[], effect: Files, run: files::screenshot, help: &[
        ("path", "optional .png (see files); default screenshots/shot-<unix ms>.png; written a frame or two later"),
    ] },
    Cmd { name: "events", aliases: &[], effect: Read, run: events::events, help: &[
        ("since", "seq"),
        ("limit", "default 500"),
    ] },
    Cmd { name: "thoughts", aliases: &[], effect: Read, run: events::thoughts, help: &[
        ("limit", "default 50"),
    ] },
    Cmd { name: "report", aliases: &[], effect: Read, run: review::report, help: &[
        ("sections", "optional subset of overview, people, economy, land, services, industry, society, alerts, history (default all but history): the one-call city review"),
        ("history_days", "days of the history section (default 30, max 365)"),
    ] },
    Cmd { name: "industry", aliases: &[], effect: Game, run: |cmd, ctx| map::industry(cmd, &mut ctx.sim.world), help: &[
        ("lot", "lot id, or"),
        ("polygon", "[[x, y], ...] area, or"),
        ("pos", "[x, y] (query only)"),
        ("kind", "farming | oil | refinery | manufacturing | logistics | fishing | tech | mining | smelting | factory_farming | none: sets the industry sub-zone bias (none hands the area back to the ground); omit to query the ground, contamination and industry weights of the lot"),
    ] },
    Cmd { name: "map", aliases: &[], effect: Read, run: review::map, help: &[
        ("cell_m", "cell size in metres, default 100 (grown to keep at most 400 cells)"),
        ("layer", "summary (table of cells) | population | land_value | jobs | hunger | vacancy | power | traffic | fertility | oil | contamination | industry | crime | homeless | trash (grid + ASCII heat map)"),
    ] },
    Cmd { name: "history", aliases: &[], effect: Read, run: review::history, help: &[
        ("keys", "optional daily/hourly series names (default all)"),
        ("days", "last n days (default all kept, up to 365)"),
    ] },
    Cmd { name: "list", aliases: &[], effect: Read, run: review::list, help: &[
        ("what", "businesses|buildings|citizens|lots|routes|plants|facilities"),
        ("sort", "column"),
        ("desc", "bool"),
        ("filter", "{column: substring | value | {min, max}}"),
        ("fields", "[columns] (id always kept)"),
        ("offset", "default 0"),
        ("limit", "default 50, max 1000"),
    ] },
    Cmd { name: "export", aliases: &[], effect: Files, run: files::export, help: &[
        ("path", "optional .json (see files); default exports/city-<unix ms>.json: report + map + history + every row of every list"),
    ] },
    Cmd { name: "directory", aliases: &[], effect: View, run: |cmd, ctx| directory::directory(&ctx.sim.world, &mut ctx.windows, cmd), help: &[
        ("kind", "businesses|people (default businesses)"),
        ("sort", "businesses: name|kind|owner|district|street|staff|revenue_today|revenue_yesterday|profit_yesterday|money|rating|footfall|state; people: name|age|class|income|job|home|household|mood|health|commute|favourite|friends|status"),
        ("desc", "bool"),
        ("search", "words to find in name, kind, owner, street, district, workplace, home"),
        ("district", "district name"),
        ("business_kind", "businesses: a kind key"),
        ("state", "businesses: building|shut|broke|no_staff|open|closed"),
        ("class", "people: low|middle|high|elite"),
        ("status", "people: working|at_home|asleep|out|travelling|ill|homeless|arriving|away"),
        ("offset", "default 0"),
        ("limit", "default 50, max 1000"),
        ("show", "bool: also open City Data's Directory page with these settings"),
        ("note", "the rows of City Data's Directory tab, sorted and filtered by the page's own code; missing values sort last"),
    ] },
    Cmd { name: "build_highlight", aliases: &[], effect: View, run: window::build_highlight, help: &[
        ("category", "a Build section (power|services|transit|rail|water|attractions), a sub-section (safety, health and education, sanitation, parking, metro, buses...) or a kind (health, police, fire, school, metro_station, coal...): opens the Build window there (a kind also picks it, as a click on its tile) and highlights those buildings on the map with their service areas"),
        ("off", "bool: close the Build window (the highlight goes with it)"),
        ("note", "no fields: what is highlighted now and the summary (worked out the frame after a change)"),
    ] },
    Cmd { name: "window", aliases: &[], effect: View, run: window::window, help: &[
        ("name", "a tool window: lines|view (or land)|saves|help|dev, or build (the Build window; section roads|zoning|parks|districts|power|services|transit|rail|water|attractions), new_game, title (the title screen), history (the Game history window); the old report names city|economy|taxes|market|people|power|transit|emergencies|districts|tourism open their City Data tab or page"),
        ("open", "bool, omitted toggles"),
        ("sub", "with a City Data name: the sub-tab (people's old tab overview|concerns|social works too)"),
    ] },
    Cmd { name: "city_data", aliases: &[], effect: View, run: window::city_data, help: &[
        ("tab", "city|economy|market|people|services|directory (omitted with open: the page it was on)"),
        ("sub", "the tab's sub-tab: city overview|land|districts|skyline|tourism; economy overview|cash_flow|ledger|taxes; market goods|businesses|trade; people population|inequality|concerns|social|favourites; services power|transit|emergencies; directory businesses|people (omitted: the tab's last one)"),
        ("open", "bool; omitted with a tab opens; no fields reports the view and the list of tabs"),
        ("scroll", "points from the page's top to scroll to (for screenshots of long pages)"),
        ("sort", "{table, col, desc}: sort a page's table by column col (0-based; omitted: the given order), as clicking its heading does"),
        ("note", "the full-screen City Data view (J in the game, Esc closes); status.city_data says what it shows"),
    ] },
];

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crate::assets::GameAssets;
    use crate::camera::{CameraMode, WalkState};
    use crate::render::{Lighting, Overlays, RenderStats};
    use nd_sim::GestureId;
    use crate::sim::Sim;
    use crate::tools::{EffectivePlan, Selection, ToolState};
    use crate::ui::{Saves, UiFlags};
    use bevy::diagnostic::DiagnosticsStore;
    use bevy::ecs::system::RunSystemOnce;

    /// A headless app holding everything `Ctx` borrows (no window, no
    /// renderer, no files read).
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let mut committee = crate::committee::CommitteeRt::default();
        committee.settings = Default::default();
        committee.courier = nd_minds::committee::Courier::mock();
        app.insert_resource(Sim::new())
            .insert_resource(GameAssets::for_tests())
            .insert_resource(committee)
            .insert_resource(Saves::default())
            .init_resource::<ToolState>()
            .init_resource::<Selection>()
            .init_resource::<EffectivePlan>()
            .init_resource::<UiFlags>()
            .init_resource::<Overlays>()
            .init_resource::<Lighting>()
            .init_resource::<CameraMode>()
            .init_resource::<WalkState>()
            .init_resource::<crate::camera::CameraFollow>()
            .init_resource::<crate::camera::CameraFly>()
            .init_resource::<EventLog>()
            .init_resource::<History>()
            .init_resource::<WindowRequests>()
            .init_resource::<RenderStats>()
            .init_resource::<DiagnosticsStore>()
            .init_resource::<crate::flags::ClaudeFlags>()
            .init_resource::<crate::build_highlight::BuildHighlight>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<crate::city_reset::CityEpoch>();
        app
    }

    /// Run one command as the API does; (HTTP status, body).
    fn send(app: &mut App, cmd: Value) -> (u16, Value) {
        let r = app.world_mut().run_system_once(move |mut ctx: Ctx| run_one(&cmd, &mut ctx)).expect("the command runs");
        (r.status, r.body)
    }

    fn tick(app: &App) -> u64 {
        app.world().resource::<Sim>().world.clock.tick
    }

    fn start(app: &App) -> u64 {
        app.world().resource::<Sim>().saved_tick.expect("a city starts clean")
    }

    #[test]
    fn read_only_commands_leave_the_city_alone() {
        let mut app = app();
        let generation = app.world().resource::<Sim>().generation;
        for cmd in ["status", "help", "report", "list_saves"] {
            let (status, body) = send(&mut app, json!({ "cmd": cmd }));
            assert_eq!(status, 200, "{cmd}: {body}");
            assert_eq!(body["ok"], true);
        }
        assert_eq!(tick(&app), start(&app));
        assert_eq!(app.world().resource::<Sim>().generation, generation);
        let (status, body) = send(&mut app, json!({ "cmd": "no_such_command" }));
        assert_eq!(status, 400);
        assert_eq!(body["ok"], false);
    }

    #[test]
    fn mutating_commands_change_the_city() {
        let mut app = app();
        let t0 = tick(&app);
        let (status, body) = send(&mut app, json!({ "cmd": "step", "ticks": 5 }));
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["ticks"], 5);
        assert_eq!(tick(&app), t0 + 5);
        let before = app.world().resource::<Sim>().world.economy.treasury;
        let (status, _) = send(&mut app, json!({ "cmd": "grant", "amount": 1000 }));
        assert_eq!(status, 200);
        assert_eq!(app.world().resource::<Sim>().world.economy.treasury, before + 1000.0);
        // An amount that is infinite as an f32 is refused.
        let (status, _) = send(&mut app, json!({ "cmd": "grant", "amount": 1e39 }));
        assert_eq!(status, 400);
        assert!(app.world().resource::<Sim>().world.economy.treasury.is_finite());
    }

    /// [fix/review-app] `new_game` and `generate_city` need `confirm` when
    /// the city has play time no save holds, and a city that was replaced
    /// leaves no selection or tool ids behind.
    #[test]
    fn replacing_a_played_city_needs_confirm() {
        let mut app = app();
        // A city just started has nothing to lose.
        let (status, body) = send(&mut app, json!({ "cmd": "new_game", "preset": "flat" }));
        assert_eq!(status, 200, "{body}");
        // Left running empty, it still has nothing to lose.
        send(&mut app, json!({ "cmd": "step", "ticks": 30 }));
        let (status, body) = send(&mut app, json!({ "cmd": "new_game", "preset": "flat" }));
        assert_eq!(status, 200, "an untouched city: {body}");
        // Something built and run: now it does.
        send(&mut app, json!({ "cmd": "seed_demo" }));
        let t0 = tick(&app);
        send(&mut app, json!({ "cmd": "step", "ticks": 30 }));
        let generation = app.world().resource::<Sim>().generation;
        for cmd in ["new_game", "generate_city"] {
            let (status, body) = send(&mut app, json!({ "cmd": cmd }));
            assert_eq!(status, 400, "{cmd}: {body}");
            assert!(body["error"].as_str().unwrap().contains("confirm: true"), "{body}");
        }
        assert_eq!(app.world().resource::<Sim>().generation, generation, "nothing replaced");
        assert_eq!(tick(&app), t0 + 30);
        // With confirm it goes ahead; the old ids go with the old city.
        app.world_mut().resource_mut::<ToolState>().selected_gesture = Some(GestureId(3));
        let (status, body) = send(&mut app, json!({ "cmd": "new_game", "preset": "flat", "confirm": true }));
        assert_eq!(status, 200, "{body}");
        assert!(app.world().resource::<Sim>().generation > generation);
        assert_eq!(app.world().resource::<ToolState>().selected_gesture, None);
        assert_eq!(app.world().resource::<crate::city_reset::CityEpoch>().0, app.world().resource::<Sim>().generation);
    }

    /// [fix/review-app] The committee's backend is the player's to change.
    #[test]
    fn the_committee_backend_needs_the_players_leave() {
        let mut app = app();
        let (status, body) = send(&mut app, json!({ "cmd": "committee", "action": "settings", "backend": "claude" }));
        assert_eq!(status, 400, "{body}");
        let (status, body) = send(&mut app, json!({ "cmd": "committee", "action": "settings", "local_url": "http://example.invalid" }));
        assert_eq!(status, 400, "{body}");
        let (status, body) = send(&mut app, json!({ "cmd": "committee", "action": "settings", "veto_allowed": false }));
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["remote_may_switch"], false);
        app.world_mut().resource_mut::<crate::committee::CommitteeRt>().remote_may_switch = true;
        let (status, body) = send(&mut app, json!({ "cmd": "committee", "action": "settings", "backend": "mock" }));
        assert_eq!(status, 200, "{body}");
    }

    /// Names and aliases are unique across the registry.
    #[test]
    fn names_and_aliases_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for c in COMMANDS {
            for n in std::iter::once(c.name).chain(c.aliases.iter().copied()) {
                assert!(seen.insert(n), "{n} is named twice");
                assert!(!n.is_empty() && n.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'), "{n:?} is not a command name");
            }
        }
        assert!(seen.len() > 100, "{}", seen.len());
    }

    /// Every command documents its fields, but for the few that take none.
    #[test]
    fn every_command_has_help() {
        let bare: Vec<&str> = COMMANDS.iter().filter(|c| c.help.is_empty()).map(|c| c.name).collect();
        assert_eq!(bare, ["help", "status", "seed_demo", "plan"], "commands with no help");
        // A field may go without text (`tool`'s `curved`), but not both.
        for c in COMMANDS {
            for (field, text) in c.help {
                assert!(!field.is_empty() || !text.is_empty(), "{} has an empty help row", c.name);
            }
        }
    }

    /// Every command that can replace the city refuses to without
    /// `confirm` once the city has play time no save holds.
    #[test]
    fn replacing_commands_refuse_without_confirm() {
        let mut app = app();
        send(&mut app, json!({ "cmd": "seed_demo" }));
        send(&mut app, json!({ "cmd": "step", "ticks": 30 }));
        let generation = app.world().resource::<Sim>().generation;
        let replacing: Vec<&str> = COMMANDS.iter().filter(|c| matches!(c.effect, Effect::Replace(_))).map(|c| c.name).collect();
        assert_eq!(replacing, ["game_history", "load", "new_game", "generate_city"]);
        for c in COMMANDS.iter().filter(|c| matches!(c.effect, Effect::Replace(_))) {
            let cmd = match c.name {
                "load" => json!({ "cmd": "load", "path": "saves/none.ron" }),
                // Revert needs an entry of this game's history on disk;
                // the check names the city's day in its refusal.
                "game_history" => {
                    let Effect::Replace(check) = c.effect else { unreachable!() };
                    let ok = app.world_mut().run_system_once(move |ctx: Ctx| matches!(check(&json!({ "cmd": "game_history" }), &ctx), Ok(Replaces::No))).expect("runs");
                    assert!(ok, "listing the history replaces nothing");
                    continue;
                }
                n => json!({ "cmd": n }),
            };
            let (status, body) = send(&mut app, cmd.clone());
            assert_eq!(status, 400, "{cmd}: {body}");
            assert!(body["error"].as_str().unwrap().contains("send confirm: true"), "{body}");
        }
        assert_eq!(app.world().resource::<Sim>().generation, generation, "nothing replaced");
    }

    /// `help` lists exactly the registry: every command under its name,
    /// every alias pointing at its command.
    #[test]
    fn help_matches_the_registry() {
        let mut app = app();
        let (_, body) = send(&mut app, json!({ "cmd": "help" }));
        let listed: Vec<&String> = body["commands"].as_object().expect("commands").keys().collect();
        let mut names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        names.sort_unstable();
        assert_eq!(listed, names);
        let aliases = body["aliases"].as_object().expect("aliases");
        assert_eq!(aliases.len(), COMMANDS.iter().map(|c| c.aliases.len()).sum::<usize>());
        for (a, c) in aliases {
            assert_eq!(lookup(a).map(|x| x.name), c.as_str(), "alias {a}");
        }
    }
}
