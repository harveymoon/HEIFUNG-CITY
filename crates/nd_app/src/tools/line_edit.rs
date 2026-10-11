//! [lines] Editing a bus line on the map: the Lines window's "Edit line"
//! (or a click on a street with the Buses tool, which starts a new line)
//! puts the Buses tool in edit mode for one line until "Finish editing
//! line" (or Enter, or Esc).
//!
//! While editing, a click
//! - on a street adds a stop where it belongs along the loop (between the
//!   two stops whose leg it lies on, not on the end; `bus_edit::insert_index`),
//!   or at the slot picked with a "+" in the Lines window's diagram.
//!   [fix/buildings-ui] While a new line is being drawn, plain clicks add
//!   stops in the order clicked (each after the last) and Shift+click
//!   inserts one where it belongs; editing an existing line keeps inserting
//!   where it belongs;
//! - on one of the line's own stops picks it up to drag it;
//! - on another line's stop gives this line a stop at that same kerb (the
//!   selection does not jump to the other line);
//!
//! and a right-click or Delete on one of its stops removes it. The state and
//! what a click means are plain data and functions ([`LineEdit`], [`click`]),
//! so they are tested without a world.

use nd_sim::{RouteId, Vec2 as SVec2};

use super::{Tool, ToolState, TransitMode};

/// How near (metres) the pointer must be to a stop to pick it.
pub const STOP_PICK: f32 = 5.0;

/// The line being edited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineEdit {
    pub route: RouteId,
    /// The next stop goes in at this index (picked with a "+" between two
    /// stops in the diagram); `None`: wherever it belongs.
    pub slot: Option<usize>,
    /// The tool to go back to when editing ends.
    pub back_to: Tool,
    /// [fix/buildings-ui] A new line being drawn: plain clicks append stops
    /// in order (Shift+click inserts where it belongs).
    pub creating: bool,
}

/// What is under the pointer, as editing sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// One of the edited line's own stops.
    Own(usize),
    /// Another line's stop.
    Other(RouteId, usize),
    /// A street a stop can go by (the point clicked).
    Street(SVec2),
    /// Nothing a stop can go on.
    Nothing,
}

/// What a click does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditOp {
    /// Pick up the line's stop to drag it (and mark it).
    Grab(usize),
    /// A stop at the other line's stop, at the slot or where it belongs.
    Share { from: (RouteId, usize), at: Option<usize> },
    /// A stop by the street at `p`.
    Insert { p: SVec2, at: Option<usize> },
    /// Nothing: say why.
    Miss,
}

/// Start editing `route` (from `tool`). Already editing: the new line is
/// edited instead and the tool to go back to is kept.
pub fn begin(current: Option<LineEdit>, route: RouteId, tool: Tool) -> LineEdit {
    let back_to = current.map_or(if tool == Tool::Transit { Tool::Inspect } else { tool }, |e| e.back_to);
    LineEdit { route, slot: None, back_to, creating: false }
}

/// Index meaning "after the last stop" (`put_bus_stop` clamps it).
pub const APPEND: usize = usize::MAX;

/// Where a new stop goes: the picked slot; else, drawing a new line, after
/// the last stop unless `shift` (then, like editing, where it belongs:
/// `None`).
pub fn target(edit: &LineEdit, shift: bool) -> Option<usize> {
    edit.slot.or((edit.creating && !shift).then_some(APPEND))
}

/// What a click on `hit` does (`shift`: Shift held).
pub fn click(edit: &LineEdit, hit: Hit, shift: bool) -> EditOp {
    match hit {
        Hit::Own(i) => EditOp::Grab(i),
        Hit::Other(r, i) if r == edit.route => EditOp::Grab(i),
        Hit::Other(r, i) => EditOp::Share { from: (r, i), at: target(edit, shift) },
        Hit::Street(p) => EditOp::Insert { p, at: target(edit, shift) },
        Hit::Nothing => EditOp::Miss,
    }
}

impl LineEdit {
    /// A stop went in at `at`: a picked slot moves on past it, so clicks in
    /// a row insert in a row.
    pub fn inserted(&mut self, at: usize) {
        if self.slot.is_some() {
            self.slot = Some(at + 1);
        }
    }

    /// Stop `idx` was removed: a slot after it moves down with the stops.
    pub fn removed(&mut self, idx: usize, stops_left: usize) {
        if let Some(s) = self.slot {
            self.slot = Some(if s > idx { s - 1 } else { s }.min(stops_left));
        }
    }

    /// Pick the slot between stops `i - 1` and `i` (`i == n` is after the
    /// last); picking it again lets it go.
    pub fn toggle_slot(&mut self, i: usize) {
        self.slot = if self.slot == Some(i) { None } else { Some(i) };
    }
}

/// Whether editing goes on: the line still exists and the Buses tool is in
/// hand in its Lines mode.
pub fn still_editing(line_exists: bool, tool: Tool, mode: TransitMode) -> bool {
    line_exists && tool == Tool::Transit && mode == TransitMode::Route
}

/// What is under the pointer at `p` for editing `route`: the nearest stop
/// within [`STOP_PICK`] (the line's own first, so a kerb two lines share
/// picks this line's stop), else a street, else nothing.
pub fn hit_at(w: &nd_sim::World, route: RouteId, p: SVec2) -> Hit {
    if let Some(r) = w.transit.routes.get(route) {
        let own = r.stops.iter().enumerate().map(|(i, s)| (i, s.pos.distance(p))).filter(|(_, d)| *d <= STOP_PICK).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = own {
            return Hit::Own(i);
        }
    }
    if let Some((r, i)) = w.transit.stop_near(p, STOP_PICK) {
        return Hit::Other(r, i);
    }
    if nd_sim::transit::BusStop::snap(&w.roads, p).is_some() {
        return Hit::Street(p);
    }
    Hit::Nothing
}

/// Where a stop for `hit` would go in the edited line: its point and the
/// index it would take (for the map's preview of the insert).
pub fn preview(w: &nd_sim::World, edit: &LineEdit, hit: Hit, shift: bool) -> Option<(SVec2, usize)> {
    let r = w.transit.routes.get(edit.route)?;
    let at = match hit {
        Hit::Other(o, i) if o != edit.route => w.transit.routes.get(o)?.stops.get(i)?.pos,
        Hit::Street(p) => nd_sim::transit::BusStop::snap(&w.roads, p)?.pos,
        _ => return None,
    };
    let stops: Vec<SVec2> = r.stops.iter().map(|s| s.pos).collect();
    let idx = target(edit, shift).map_or_else(|| nd_sim::world::bus_edit::insert_index(&stops, &r.legs, at), |s| s.min(stops.len()));
    Some((at, idx))
}

/// Start editing `route`: the Buses tool in its Lines mode, the line picked.
pub fn start(tools: &mut ToolState, route: RouteId) {
    tools.line_edit = Some(begin(tools.line_edit, route, tools.tool));
    tools.tool = Tool::Transit;
    tools.transit_mode = TransitMode::Route;
    tools.route_sel = Some(route);
    tools.stop_sel = None;
    tools.stop_drag = None;
    tools.status = "Editing the line: click streets to add stops (they go in where they belong along the loop), drag a stop to move it, right-click one to remove it, click another line's stop to share it.".into();
}

/// [fix/buildings-ui] Start drawing the new line `route`: clicks add its
/// stops in order.
pub fn start_new(tools: &mut ToolState, route: RouteId) {
    start(tools, route);
    if let Some(e) = tools.line_edit.as_mut() {
        e.creating = true;
    }
    tools.status = "New line: click streets for its stops in order, round the loop; Shift+click inserts a stop where it belongs. Drag a stop to move it, right-click to remove it; Enter finishes.".into();
}

/// Stop editing: back to the tool editing started from. A line left with
/// no stops is removed; one stop short of a loop says so. The status line.
pub fn finish(w: &mut nd_sim::World, tools: &mut ToolState) -> String {
    let Some(edit) = tools.line_edit.take() else { return String::new() };
    tools.stop_drag = None;
    tools.tool = edit.back_to;
    let msg = match w.transit.routes.get(edit.route).map(|r| (r.name.clone(), r.stops.len())) {
        None => "Line gone.".to_string(),
        Some((_, 0)) => {
            w.remove_bus_route(edit.route);
            tools.route_sel = None;
            "No stops: the line was dropped.".to_string()
        }
        Some((name, 1)) => format!("{name} has one stop: a line needs two or more before buses run it."),
        Some((name, n)) => format!("{name} done: {n} stops in a loop."),
    };
    tools.status = msg.clone();
    msg
}

/// The edit-mode input of the Buses tool (see the module docs). `press`
/// and `release` are the left button; `remove` a right-click or Delete.
pub fn input(w: &mut nd_sim::World, tools: &mut ToolState, ground: Option<SVec2>, press: bool, shift: bool, release: bool, remove: bool) {
    let Some(mut edit) = tools.line_edit else { return };
    let hit = ground.map_or(Hit::Nothing, |p| hit_at(w, edit.route, p));
    tools.hover_stop = match hit {
        Hit::Own(i) => Some((edit.route, i)),
        Hit::Other(r, i) => Some((r, i)),
        _ => None,
    };
    if press {
        tools.status = match click(&edit, hit, shift) {
            EditOp::Grab(i) => {
                tools.stop_drag = Some((edit.route, i));
                tools.stop_sel = Some((edit.route, i));
                format!("Stop {} picked: drag it along the streets to move it; right-click or Delete removes it.", i + 1)
            }
            EditOp::Share { from, at } => match w.share_bus_stop(edit.route, from, at) {
                Ok(i) => {
                    edit.inserted(i);
                    let other = w.transit.routes.get(from.0).map_or(String::new(), |r| r.name.clone());
                    format!("Stop {} added at {other}'s stop {}: riders can change there.", i + 1, from.1 + 1)
                }
                Err(e) => format!("No stop: {e}."),
            },
            EditOp::Insert { p, at } => match w.insert_bus_stop(edit.route, at, p) {
                Ok(i) => {
                    edit.inserted(i);
                    let n = w.transit.routes.get(edit.route).map_or(0, |r| r.stops.len());
                    if i + 1 == n { format!("Stop {} added at the end of the loop.{}", i + 1, if edit.creating { " Shift+click inserts a stop where it belongs." } else { "" }) } else { format!("Stop inserted as stop {} of {n}, between the stops either side of it.", i + 1) }
                }
                Err(e) => format!("No stop: {e}."),
            },
            EditOp::Miss => "Click by a street to add a stop, or on a stop.".into(),
        };
    }
    if release {
        if let Some((r, i)) = tools.stop_drag.take() {
            if let Some(p) = ground {
                let moved = w.transit.routes.get(r).and_then(|x| x.stops.get(i)).is_some_and(|s| s.pos.distance(p) > 2.5);
                if moved && w.move_bus_stop(r, i, p) {
                    tools.status = format!("Stop {} moved.", i + 1);
                }
            }
        }
    }
    if remove {
        if let Hit::Own(i) = hit {
            w.remove_bus_stop(edit.route, i);
            tools.stop_sel = None;
            tools.hover_stop = None;
            let left = w.transit.routes.get(edit.route).map_or(0, |r| r.stops.len());
            edit.removed(i, left);
            tools.status = format!("Stop {} removed; {left} left.", i + 1);
        }
    }
    // The last stop removed takes the line with it.
    if w.transit.routes.contains_key(edit.route) {
        tools.line_edit = Some(edit);
    } else {
        tools.line_edit = None;
        tools.route_sel = None;
        tools.status = "Last stop removed: the line is gone.".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn ids() -> (RouteId, RouteId) {
        let mut m: SlotMap<RouteId, ()> = SlotMap::with_key();
        (m.insert(()), m.insert(()))
    }

    #[test]
    fn editing_remembers_the_tool_it_started_from() {
        let (a, b) = ids();
        let e = begin(None, a, Tool::Inspect);
        assert_eq!(e, LineEdit { route: a, slot: None, back_to: Tool::Inspect, creating: false });
        // Started from the Buses tool itself: back to Inspect after.
        assert_eq!(begin(None, a, Tool::Transit).back_to, Tool::Inspect);
        // Switching lines mid-edit keeps the way back.
        let e2 = begin(Some(LineEdit { slot: Some(2), ..e }), b, Tool::Transit);
        assert_eq!(e2, LineEdit { route: b, slot: None, back_to: Tool::Inspect, creating: false });
    }

    #[test]
    fn clicks_grab_own_stops_share_others_and_insert_on_streets() {
        let (a, b) = ids();
        let mut e = begin(None, a, Tool::Inspect);
        assert_eq!(click(&e, Hit::Own(3), false), EditOp::Grab(3));
        // Another line's stop does not switch lines: it is shared.
        assert_eq!(click(&e, Hit::Other(b, 1), false), EditOp::Share { from: (b, 1), at: None });
        let p = SVec2::new(4.0, 5.0);
        assert_eq!(click(&e, Hit::Street(p), false), EditOp::Insert { p, at: None });
        assert_eq!(click(&e, Hit::Street(p), true), EditOp::Insert { p, at: None });
        assert_eq!(click(&e, Hit::Nothing, false), EditOp::Miss);
        // With a slot picked, inserts and shares go there.
        e.toggle_slot(2);
        assert_eq!(click(&e, Hit::Street(p), false), EditOp::Insert { p, at: Some(2) });
        assert_eq!(click(&e, Hit::Other(b, 0), false), EditOp::Share { from: (b, 0), at: Some(2) });
        e.toggle_slot(2);
        assert_eq!(e.slot, None, "picking the slot again lets it go");
    }

    /// [fix/buildings-ui] Drawing a new line: plain clicks add stops in
    /// sequence, Shift+click inserts where the stop belongs.
    #[test]
    fn a_new_line_takes_stops_in_order_and_shift_inserts() {
        let (a, b) = ids();
        let e = LineEdit { creating: true, ..begin(None, a, Tool::Transit) };
        let p = SVec2::new(4.0, 5.0);
        assert_eq!(click(&e, Hit::Street(p), false), EditOp::Insert { p, at: Some(APPEND) });
        assert_eq!(click(&e, Hit::Other(b, 1), false), EditOp::Share { from: (b, 1), at: Some(APPEND) });
        assert_eq!(click(&e, Hit::Street(p), true), EditOp::Insert { p, at: None });
        // A slot picked in the diagram still wins.
        let mut e = e;
        e.toggle_slot(1);
        assert_eq!(click(&e, Hit::Street(p), false), EditOp::Insert { p, at: Some(1) });
        assert_eq!(click(&e, Hit::Street(p), true), EditOp::Insert { p, at: Some(1) });
    }

    #[test]
    fn a_slot_follows_the_stops_as_they_change() {
        let (a, _) = ids();
        let mut e = begin(None, a, Tool::Inspect);
        // No slot: inserting leaves it none.
        e.inserted(4);
        assert_eq!(e.slot, None);
        e.toggle_slot(2);
        e.inserted(2);
        assert_eq!(e.slot, Some(3), "the next click goes in after the one just added");
        e.removed(0, 4);
        assert_eq!(e.slot, Some(2));
        e.removed(5, 4);
        assert_eq!(e.slot, Some(2), "a stop after the slot leaves it");
        e.slot = Some(4);
        e.removed(3, 2);
        assert_eq!(e.slot, Some(2), "never past the end");
    }

    #[test]
    fn editing_ends_with_the_line_or_the_tool() {
        assert!(still_editing(true, Tool::Transit, TransitMode::Route));
        assert!(!still_editing(false, Tool::Transit, TransitMode::Route), "line deleted");
        assert!(!still_editing(true, Tool::Road, TransitMode::Route), "another tool picked");
        assert!(!still_editing(true, Tool::Transit, TransitMode::Metro), "metro mode");
    }
}
