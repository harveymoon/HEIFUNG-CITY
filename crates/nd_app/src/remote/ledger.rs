//! [ledger] `ledger`: the city's money as accounts (`nd_sim::bank`). With no
//! field: the books' state and today's flows; `account`: one balance and its
//! recent moves; `kind`: one kind of flow day by day; `day`: one closed day;
//! `flows`: where rent, wages, profits and taxes go (the money-flow page);
//! `trace` / `check`: the debug levers.

use nd_sim::bank::{dollars, Account, Cell, Class, Kind, Tx};
use nd_sim::World;
use serde_json::{json, Value};

use super::view::*;

/// The account named by `v`: "treasury", "outside", "landlords", "airline",
/// "household 12:3", "business 4:1", "owner 7:1", "person 9:5",
/// "tourist 3:1", "tower 0".
pub fn parse_account(v: &Value) -> Result<Account, String> {
    let s = v.as_str().ok_or("account must be a string like \"household 12:3\" or \"treasury\"")?;
    Account::parse(s).ok_or_else(|| format!("unknown account {s:?}: treasury|outside|landlords|airline|household ID|person ID|business ID|owner ID|tourist ID|tower N"))
}

fn r(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// One move as its account saw it.
pub fn tx_json(w: &World, t: &Tx) -> Value {
    json!({
        "tick": t.tick, "when": crate::ui::fmt::day_time(t.tick),
        "kind": t.kind.key(), "label": t.kind.label(),
        "other": t.other.label(), "other_name": account_name(w, t.other),
        "amount": r(t.dollars()),
    })
}

/// A name for an account a player would recognise.
pub fn account_name(w: &World, a: Account) -> String {
    match a {
        Account::Outside => "outside world".into(),
        Account::Treasury => "city treasury".into(),
        Account::Landlords => "independent landlords".into(),
        Account::Airline => w.air.company.name.clone(),
        Account::Household(h) => w.households.get(h).and_then(|h| h.members.first()).and_then(|c| w.citizens.get(*c)).map_or("a household (gone)".into(), |c| format!("{}'s household", c.name)),
        Account::Person(c) => w.citizens.get(c).map_or("someone (gone)".into(), |c| c.name.clone()),
        Account::Business(b) => w.buildings.get(b).map_or("a business (gone)".into(), |b| b.name.clone()),
        Account::Owner(o) => w.estate.owners.get(o).map_or("an owner (gone)".into(), |o| o.name.clone()),
        Account::Tourist(_) => "a visitor".into(),
        Account::Tower(i) => w.skyline.developers.get(i as usize).map_or("a tower developer".into(), |d| d.name.clone()),
    }
}

fn cell_json(c: &Cell) -> Value {
    json!({ "kind": c.kind.key(), "from": c.from.label(), "to": c.to.label(), "amount": r(c.dollars()), "moves": c.n })
}

fn cells_json(cells: &[Cell]) -> Value {
    let mut v: Vec<&Cell> = cells.iter().collect();
    v.sort_by(|a, b| b.units.abs().cmp(&a.units.abs()));
    Value::Array(v.into_iter().map(cell_json).collect())
}

/// Per kind: the day's total and how much of it crossed the city limits.
fn by_kind(cells: &[Cell]) -> Value {
    let mut out = serde_json::Map::new();
    for k in Kind::all() {
        let mine: Vec<&Cell> = cells.iter().filter(|c| c.kind == k).collect();
        if mine.is_empty() {
            continue;
        }
        let total: i64 = mine.iter().map(|c| c.units).sum();
        let inn: i64 = mine.iter().filter(|c| c.from == Class::Outside).map(|c| c.units).sum();
        let out_: i64 = mine.iter().filter(|c| c.to == Class::Outside).map(|c| c.units).sum();
        out.insert(k.key(), json!({ "total": r(dollars(total)), "from_outside": r(dollars(inn)), "to_outside": r(dollars(out_)) }));
    }
    Value::Object(out)
}

/// The money-flow summary: by group (property, wages, profits, city,
/// trade, other), each kind's flows between classes, and the day's net
/// with the outside world.
pub fn flows(w: &World, cells: &[Cell]) -> Value {
    let mut groups = serde_json::Map::new();
    for g in ["property", "wages", "profits", "city", "trade", "other"] {
        let mine: Vec<Cell> = cells.iter().filter(|c| c.kind.group() == g).copied().collect();
        if mine.is_empty() {
            continue;
        }
        groups.insert(g.into(), json!({ "total": r(dollars(mine.iter().map(|c| c.units).sum())), "flows": cells_json(&mine) }));
    }
    let inn: i64 = cells.iter().filter(|c| c.from == Class::Outside).map(|c| c.units).sum();
    let out: i64 = cells.iter().filter(|c| c.to == Class::Outside).map(|c| c.units).sum();
    let _ = w;
    json!({ "groups": groups, "in_from_outside": r(dollars(inn)), "out_to_outside": r(dollars(out)), "net_with_outside": r(dollars(inn - out)) })
}

pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    if let Some(v) = field(cmd, "check") {
        w.economy.bank.check = v.as_bool().ok_or("check must be true or false")?;
    }
    if let Some(v) = cmd.get("trace") {
        let a = if v.is_null() || v == &json!(false) { None } else { Some(parse_account(v)?) };
        w.economy.bank.set_trace(a);
    }
    if let Some(v) = field(cmd, "account") {
        let a = parse_account(v)?;
        let bank = &w.economy.bank;
        let recent: Vec<Value> = bank.recent(a).iter().rev().map(|t| tx_json(w, t)).collect();
        let traced = (bank.trace == Some(a)).then(|| bank.trace_log.iter().rev().take(500).map(|t| tx_json(w, t)).collect::<Vec<_>>());
        return Ok(json!({
            "account": a.label(), "name": account_name(w, a), "class": w.account_class(a).label(),
            "open": bank.has(a), "balance": r(bank.balance(a)),
            "recent": recent, "trace": traced,
        }));
    }
    let bank = &w.economy.bank;
    if let Some(v) = field(cmd, "kind") {
        let s = v.as_str().ok_or("kind must be a string")?;
        let k = Kind::parse(s).ok_or_else(|| format!("unknown kind {s:?}: {}", Kind::all().map(|k| k.key()).collect::<Vec<_>>().join("|")))?;
        let days: Vec<Value> = bank
            .history
            .iter()
            .map(|d| {
                let mine: Vec<Cell> = d.cells.iter().filter(|c| c.kind == k).copied().collect();
                json!({ "day": d.day + 1, "total": r(d.of(k)), "flows": cells_json(&mine) })
            })
            .collect();
        let today: Vec<Cell> = bank.today_cells().into_iter().filter(|c| c.kind == k).collect();
        return Ok(json!({ "kind": k.key(), "label": k.label(), "group": k.group(), "today": { "total": r(bank.today_of(k)), "flows": cells_json(&today) }, "days": days }));
    }
    if let Some(v) = field(cmd, "day") {
        let d = v.as_u64().ok_or("day must be a whole number (the game's day, from 1)")?;
        let s = bank.day(d.saturating_sub(1)).ok_or_else(|| format!("day {d} is not kept (the last {} days are)", nd_sim::bank::HISTORY_DAYS))?;
        return Ok(json!({ "day": d, "by_kind": by_kind(&s.cells), "flows": cells_json(&s.cells), "summary": flows(w, &s.cells) }));
    }
    if field(cmd, "flows").is_some() {
        let today = bank.today_cells();
        let yesterday = bank.yesterday().map(|d| flows(w, &d.cells));
        return Ok(json!({ "today": flows(w, &today), "yesterday": yesterday }));
    }
    let today = bank.today_cells();
    let drifts: Vec<Value> = bank.drifts.iter().rev().take(20).map(|d| json!({ "tick": d.tick, "account": d.account, "amount": r(d.dollars), "last_kind": d.last_kind.map(|k| k.key()) })).collect();
    Ok(json!({
        "sum_units": w.ledger_sum(),
        "balanced": w.ledger_sum() == 0,
        "accounts": bank.accounts(),
        "treasury": r(bank.balance(Account::Treasury)),
        "landlords": r(bank.balance(Account::Landlords)),
        "outside": r(bank.balance(Account::Outside)),
        "untracked_total": r(dollars(bank.untracked_units)), "untracked_moves": bank.untracked_moves,
        "orphans": bank.orphans,
        "moves_today": bank.moves_today,
        "check": bank.check, "trace": bank.trace.map(|a| a.label()),
        "drifts": drifts,
        "today": by_kind(&today),
        "yesterday": bank.yesterday().map(|d| json!({ "day": d.day + 1, "by_kind": by_kind(&d.cells) })),
        "days_kept": bank.history.len(),
        "note": "amounts in dollars; balances are exact in 1/10000 dollar units and sum to zero with the outside world. kinds: see `ledger {kind}`; accounts are spelled \"household 12:3\", \"business 4:1\", \"owner 7:1\", \"person 9:5\", \"treasury\", \"landlords\", \"outside\"",
    }))
}
