//! [places] JSON for taste and memory of places: a citizen's taste,
//! favourite and sworn-off places and acquaintances; a place's character
//! and reputation; the venue ratings for the economy report.

use nd_sim::places::{PlaceCharacter, Reputation};
use nd_sim::{BuildingId, CitizenId, World};
use serde_json::{json, Value};

use super::view::{key_str, round, tick_time};

fn character(ch: &PlaceCharacter) -> Value {
    json!({
        "summary": ch.summary(), "vibe": ch.vibe.label(), "tier": ch.tier.label(),
        "polish": round(ch.polish), "price": round(ch.price), "noise": round(ch.noise), "crowd": round(ch.crowd),
        "crowd_class": round(ch.crowd_class), "safety": round(ch.safety), "clean": round(ch.clean), "sketchiness": round(ch.sketchiness()),
    })
}

fn reputation(r: Option<&Reputation>) -> Value {
    match r {
        None => json!({ "rated": 0 }),
        Some(r) => json!({
            "rating": round(r.rating), "stars": round(r.stars()), "rated": r.rated, "regulars": r.regulars, "never_again": r.never_again,
            "visits_today": r.visits_today, "regular_visits_today": r.regular_visits_today, "regular_revenue_today": round(r.regular_revenue_today),
            "visits_yesterday": r.visits_yesterday, "regular_revenue_yesterday": round(r.regular_revenue_yesterday),
            "great_nights": r.great_nights, "fights": r.fights,
        }),
    }
}

/// `inspect {citizen}` → `places`.
pub fn citizen_places(w: &World, id: CitizenId) -> Value {
    let Some(c) = w.citizens.get(id) else { return Value::Null };
    let name = |b: BuildingId| w.buildings.get(b).map(|x| x.name.clone());
    let row = |b: BuildingId, m: &nd_sim::places::PlaceMemory| {
        json!({
            "id": key_str(b), "name": name(b), "land_use": m.land_use.label(), "visits": m.visits, "score": round(m.score),
            "best": round(m.best), "worst": round(m.worst), "regular": m.regular, "never_again": m.never, "last_visit": tick_time(m.last_visit),
        })
    };
    let t = w.taste_of(id);
    json!({
        "taste": t.map(|t| json!({
            "likes_loud": round(t.loud), "likes_crowded": round(t.crowded), "likes_cheap": round(t.cheap), "likes_posh_crowd": round(t.posh),
            "needs_polish": round(t.polish), "needs_safety": round(t.safety), "needs_clean": round(t.clean), "novelty": round(t.novelty),
        })),
        "favourites": w.favourite_places(id).iter().map(|(b, m)| row(*b, m)).collect::<Vec<_>>(),
        "regular_spots": c.places.places.iter().filter(|p| p.regular).map(|p| name(p.place)).collect::<Vec<_>>(),
        "never_again": c.places.places.iter().filter(|p| p.never).map(|p| row(p.place, p)).collect::<Vec<_>>(),
        "acquaintances": c.places.acquaintances.iter().map(|a| json!({
            "id": key_str(a.who), "name": w.citizens.get(a.who).map(|x| x.name.clone()), "met_at": name(a.place), "times": a.times,
        })).collect::<Vec<_>>(),
    })
}

/// `inspect {building}` → `place` (null for anything but a venue, shop or park).
pub fn building_place(w: &World, id: BuildingId) -> Value {
    let Some(ch) = w.place_character(id) else { return Value::Null };
    json!({ "character": character(&ch), "reputation": reputation(w.reputation(id)) })
}

/// `report.economy.venues`: every venue's rating and regulars, and the
/// town's going-out numbers.
pub fn venue_ratings(w: &World) -> Value {
    let mut rows: Vec<(f32, Value)> = Vec::new();
    let (mut regulars, mut never, mut revenue, mut regular_revenue) = (0u32, 0u32, 0.0f32, 0.0f32);
    for (id, b) in w.buildings.iter() {
        let Some(z) = b.business.as_ref().filter(|z| z.kind == nd_sim::BusinessKind::Venue) else { continue };
        let Some(ch) = w.place_character(id) else { continue };
        let r = w.reputation(id);
        regulars += r.map_or(0, |r| r.regulars);
        never += r.map_or(0, |r| r.never_again);
        revenue += z.revenue_today;
        regular_revenue += r.map_or(0.0, |r| r.regular_revenue_today);
        rows.push((
            r.filter(|r| r.rated > 0).map_or(-9.0, |r| r.rating),
            json!({
                "id": key_str(id), "name": b.name, "vibe": ch.vibe.label(), "character": ch.summary(), "closed": z.closed,
                "revenue_today": round(z.revenue_today), "reputation": reputation(r),
            }),
        ));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    let rated: Vec<f32> = rows.iter().map(|r| r.0).filter(|r| *r > -9.0).collect();
    let p = &w.places;
    json!({
        "count": rows.len(),
        "mean_rating": if rated.is_empty() { Value::Null } else { json!(round(rated.iter().sum::<f32>() / rated.len() as f32)) },
        "regulars": regulars, "never_again": never,
        "revenue_today": round(revenue), "regular_revenue_today": round(regular_revenue),
        "regular_share_of_revenue": if revenue > 0.0 { round(regular_revenue / revenue) } else { 0.0 },
        "outings_together_today": p.outings_together_today, "outings_together_total": p.outings_together_total,
        "explorations_total": p.explorations_total, "regulars_made_total": p.regulars_made_total, "never_again_total": p.never_again_total,
        "venues": rows.into_iter().map(|r| r.1).collect::<Vec<_>>(),
    })
}
