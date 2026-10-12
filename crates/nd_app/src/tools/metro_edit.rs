//! [w1] Editing a metro line on the map, stop by stop, as bus lines are
//! (`line_edit`): the Lines window's "Edit stops" puts the Buses tool's
//! Metro mode in edit mode for one line until "Finish editing" (or Enter,
//! or Esc).
//!
//! While editing, a click
//! - on a station not on the line puts it in where it belongs along the
//!   line (between the stations whose tunnel it lies by, or onto the nearer
//!   end), or at the slot picked with a "+" in the Lines window's diagram;
//! - on one of the line's stations picks it up: let go over another
//!   station and the line calls there instead;
//!
//! and a right-click (or Delete) on one of its stations takes it off the
//! line. Every change re-lays the tunnel at once and pays for the new
//! stretches only (`World::relay_metro_line`).

use nd_sim::subway::MetroLineId;
use nd_sim::{BuildingId, Vec2 as SVec2, World};

use super::{ToolState, TransitMode};

/// The metro line being edited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetroEdit {
    pub line: MetroLineId,
    /// The next station goes in at this index (a "+" in the diagram);
    /// `None`: where it belongs.
    pub slot: Option<usize>,
    /// The stop picked up to move.
    pub grab: Option<usize>,
}

/// What a click on a station does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MetroOp {
    /// Pick up stop `i` to move it.
    Grab(usize),
    /// Put the station into the line.
    Insert(BuildingId, Option<usize>),
    Miss,
}

/// What a press on `station` (the one under the pointer, if any) does.
pub fn click(edit: &MetroEdit, stations: &[BuildingId], station: Option<BuildingId>) -> MetroOp {
    match station {
        Some(s) => match stations.iter().position(|x| *x == s) {
            Some(i) => MetroOp::Grab(i),
            None => MetroOp::Insert(s, edit.slot),
        },
        None => MetroOp::Miss,
    }
}

fn name(w: &World, b: BuildingId) -> String {
    w.buildings.get(b).map_or_else(|| "that station".to_string(), |b| b.name.clone())
}

/// Start editing `line`: the Buses tool in Metro mode, the line open.
pub fn start(tools: &mut ToolState, line: MetroLineId) {
    tools.tool = super::Tool::Transit;
    tools.transit_mode = TransitMode::Metro;
    tools.metro_draft.clear();
    tools.metro_sel = Some(line);
    tools.metro_edit = Some(MetroEdit { line, slot: None, grab: None });
    tools.status = "Editing the metro line: click a station to add it (it goes in where it belongs), drag a stop onto another station to move it, right-click a stop to take it off. Enter finishes.".into();
}

/// Stop editing; the status line.
pub fn finish(w: &World, tools: &mut ToolState) -> String {
    let Some(e) = tools.metro_edit.take() else { return String::new() };
    let msg = match w.subway.lines.get(e.line) {
        Some(l) => format!("{} done: {} stations{}, {:.1} km of tunnel.", l.name, l.stations.len(), if l.looped { " in a loop" } else { "" }, l.length() / 1000.0),
        None => "Line gone.".into(),
    };
    tools.status = msg.clone();
    msg
}

/// Spent from the treasury by an edit, for the status line.
fn paid(before: f32, w: &World) -> String {
    let p = before - w.economy.treasury;
    if p > 0.5 {
        format!(" New tunnel: {p:.0}.")
    } else {
        String::new()
    }
}

/// The edit-mode input of the Metro mode. `press` and `release` are the
/// left button, `remove` a right-click or Delete, `finish` Enter.
pub fn input(w: &mut World, tools: &mut ToolState, ground: Option<SVec2>, press: bool, release: bool, remove: bool, finish_now: bool) {
    let Some(mut edit) = tools.metro_edit else { return };
    let Some(stations) = w.subway.lines.get(edit.line).map(|l| l.stations.clone()) else {
        tools.metro_edit = None;
        tools.status = "The line is gone.".into();
        return;
    };
    let hover = ground.and_then(|p| w.metro_station_at(p, crate::metro_ux::PICK_RADIUS));
    tools.metro_hover = hover;
    let before = w.economy.treasury;
    if press {
        tools.status = match click(&edit, &stations, hover) {
            MetroOp::Grab(i) => {
                edit.grab = Some(i);
                format!("{} picked: let go over another station to call there instead; right-click takes it off.", name(w, stations[i]))
            }
            MetroOp::Insert(s, at) => match w.insert_metro_stop(edit.line, at, s) {
                Ok(i) => {
                    if edit.slot.is_some() {
                        edit.slot = Some(i + 1);
                    }
                    format!("{} added as stop {} of {}.{}", name(w, s), i + 1, stations.len() + 1, paid(before, w))
                }
                Err(e) => format!("{} not added: {e}.", name(w, s)),
            },
            MetroOp::Miss => crate::metro_ux::missed_hint(w),
        };
    }
    if release {
        if let Some(i) = edit.grab.take() {
            if let Some(s) = hover.filter(|s| Some(s) != stations.get(i)) {
                tools.status = match w.move_metro_stop(edit.line, i, s) {
                    Ok(()) => format!("Stop {} now calls at {}.{}", i + 1, name(w, s), paid(before, w)),
                    Err(e) => format!("Not moved: {e}."),
                };
            }
        }
    }
    if remove {
        if let Some(i) = hover.and_then(|s| stations.iter().position(|x| *x == s)) {
            tools.status = match w.remove_metro_stop(edit.line, i) {
                Ok(()) => {
                    if let Some(s) = edit.slot {
                        edit.slot = Some(if s > i { s - 1 } else { s }.min(stations.len() - 1));
                    }
                    format!("{} taken off the line.", name(w, stations[i]))
                }
                Err(e) => format!("Not removed: {e}."),
            };
        }
    }
    tools.metro_edit = Some(edit);
    if finish_now {
        finish(w, tools);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    #[test]
    fn a_click_grabs_a_stop_of_the_line_or_inserts_another_station() {
        let mut lines: SlotMap<MetroLineId, ()> = SlotMap::with_key();
        let line = lines.insert(());
        let mut b: SlotMap<BuildingId, ()> = SlotMap::with_key();
        let (s0, s1, s2) = (b.insert(()), b.insert(()), b.insert(()));
        let mut e = MetroEdit { line, slot: None, grab: None };
        assert_eq!(click(&e, &[s0, s1], Some(s1)), MetroOp::Grab(1));
        assert_eq!(click(&e, &[s0, s1], Some(s2)), MetroOp::Insert(s2, None));
        assert_eq!(click(&e, &[s0, s1], None), MetroOp::Miss);
        e.slot = Some(1);
        assert_eq!(click(&e, &[s0, s1], Some(s2)), MetroOp::Insert(s2, Some(1)));
    }
}
