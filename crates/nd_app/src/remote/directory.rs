//! [directory] `directory {kind, sort, desc, search, district, ...}`: the
//! rows of City Data's Directory tab, sorted and filtered by the same code
//! as the page (`crate::directory`), so a reader sees what the player sees.
//! With `show: true` the page opens with those settings (for screenshots).

use serde_json::{json, Value};

use super::view::{key_str, opt_bool, opt_str, opt_u64, round, v2};
use super::window::WindowRequests;
use crate::directory::{self as dir, BizCol, BizState, Filter, PersonCol, Status};
use crate::ui::city_data::{Page, Target};
use crate::ui::directory::TableState;

pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_LIMIT: usize = 1000;

/// What the API and the page share: the filter from the command's fields.
fn filter_of(cmd: &Value) -> Result<Filter, String> {
    let mut f = Filter { search: opt_str(cmd, "search")?.or(opt_str(cmd, "filter")?).unwrap_or("").to_string(), ..Default::default() };
    f.district = opt_str(cmd, "district")?.filter(|s| !s.trim().is_empty()).map(|s| s.trim().to_string());
    if let Some(k) = opt_str(cmd, "business_kind")?.or(opt_str(cmd, "of")?) {
        f.kind = Some(nd_sim::BusinessKind::from_key(k).ok_or_else(|| format!("unknown business kind {k:?}"))?);
    }
    if let Some(s) = opt_str(cmd, "state")? {
        f.state = Some(BizState::parse(s).ok_or_else(|| format!("unknown state {s:?}; one of {}", BizState::ALL.map(|s| s.key()).join(", ")))?);
    }
    if let Some(c) = opt_str(cmd, "class")? {
        f.class = Some(dir::class_parse(c).ok_or_else(|| format!("unknown class {c:?}; one of low, middle, high, elite"))?);
    }
    if let Some(s) = opt_str(cmd, "status")? {
        f.status = Some(Status::parse(s).ok_or_else(|| format!("unknown status {s:?}; one of {}", Status::ALL.map(|s| s.key()).join(", ")))?);
    }
    Ok(f)
}

fn opt_r(v: Option<f32>) -> Value {
    v.map_or(Value::Null, |x| json!(round(x)))
}

pub fn directory(w: &nd_sim::World, windows: &mut WindowRequests, cmd: &Value) -> Result<Value, String> {
    let kind = opt_str(cmd, "kind")?.unwrap_or("businesses").trim().to_ascii_lowercase();
    let people = match kind.as_str() {
        "businesses" | "business" | "firms" => false,
        "people" | "citizens" | "persons" => true,
        other => return Err(format!("unknown kind {other:?}; businesses or people")),
    };
    let filter = filter_of(cmd)?;
    let desc = opt_bool(cmd, "desc")?;
    let offset = opt_u64(cmd, "offset")?.unwrap_or(0) as usize;
    let limit = (opt_u64(cmd, "limit")?.unwrap_or(DEFAULT_LIMIT as u64) as usize).min(MAX_LIMIT);
    let show = opt_bool(cmd, "show")?.unwrap_or(false);
    let start = std::time::Instant::now();
    let mut v = if people {
        if filter.kind.is_some() || filter.state.is_some() {
            return Err("business_kind and state filter businesses; for people use class, status, district".into());
        }
        let col = match opt_str(cmd, "sort")? {
            Some(s) => PersonCol::parse(s).ok_or_else(|| format!("unknown column {s:?}; one of {}", PersonCol::ALL.map(|c| c.key()).join(", ")))?,
            None => PersonCol::Name,
        };
        let desc = desc.unwrap_or(false);
        let rows = dir::people(w);
        let read_ms = start.elapsed().as_secs_f32() * 1000.0;
        let order = dir::order(&rows, &filter, col, desc);
        let out: Vec<Value> = order
            .iter()
            .skip(offset)
            .take(limit)
            .map(|&i| {
                let r = &rows[i as usize];
                json!({
                    "id": key_str(r.id), "name": r.name, "age": r.age, "class": dir::class_key(r.class), "income": round(r.income),
                    "work": r.work.map(key_str), "work_label": r.work_label, "home": r.home.map(key_str), "home_label": r.home_label,
                    "street": r.street, "district": r.district, "household": r.household, "mood": round(r.mood), "satiety": round(r.satiety),
                    "hungry_days": r.hungry_days, "ill": r.ill, "commute": r.commute.map(|m| m.key()), "favourite": r.favourite,
                    "friends": r.friends, "status": r.status.key(), "pos": v2(r.pos),
                })
            })
            .collect();
        if show {
            windows.directory_people = Some(TableState { filter: filter.clone(), col, desc });
            windows.request_data(Some(Target::Page(Page::DirectoryPeople)), Some(true));
        }
        json!({ "kind": "people", "read_ms": round(read_ms), "sort": col.key(), "desc": desc, "total": rows.len(), "matched": order.len(), "offset": offset, "rows": out })
    } else {
        if filter.class.is_some() || filter.status.is_some() {
            return Err("class and status filter people; for businesses use business_kind, state, district".into());
        }
        let col = match opt_str(cmd, "sort")? {
            Some(s) => BizCol::parse(s).ok_or_else(|| format!("unknown column {s:?}; one of {}", BizCol::ALL.map(|c| c.key()).join(", ")))?,
            None => BizCol::Name,
        };
        let desc = desc.unwrap_or(false);
        let rows = dir::businesses(w);
        let read_ms = start.elapsed().as_secs_f32() * 1000.0;
        let order = dir::order(&rows, &filter, col, desc);
        let out: Vec<Value> = order
            .iter()
            .skip(offset)
            .take(limit)
            .map(|&i| {
                let r = &rows[i as usize];
                json!({
                    "id": key_str(r.id), "name": r.name, "kind": r.kind.key(), "kind_label": r.kind_label, "owner": r.owner, "owner_kind": r.owner_kind,
                    "district": r.district, "street": r.street, "staff": r.staff, "jobs": r.jobs,
                    "revenue_today": round(r.revenue_today), "spend_today": round(r.spend_today),
                    "revenue_yesterday": opt_r(r.revenue_yesterday), "profit_yesterday": opt_r(r.profit_yesterday),
                    "money": round(r.money), "rating": opt_r(r.rating), "rated": r.rated, "footfall": r.footfall,
                    "state": r.state.key(), "pos": r.pos.map(v2),
                })
            })
            .collect();
        if show {
            windows.directory_biz = Some(TableState { filter: filter.clone(), col, desc });
            windows.request_data(Some(Target::Page(Page::DirectoryBusinesses)), Some(true));
        }
        json!({ "kind": "businesses", "read_ms": round(read_ms), "sort": col.key(), "desc": desc, "total": rows.len(), "matched": order.len(), "offset": offset, "rows": out })
    };
    v["took_ms"] = json!(round(start.elapsed().as_secs_f32() * 1000.0));
    v["shown"] = json!(show);
    Ok(v)
}
