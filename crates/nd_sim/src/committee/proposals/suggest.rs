//! Members' suggestions, how urgent they feel, and how each leans on a
//! proposal.

use super::*;

/// The area of the report where `f` is smallest among areas with homes
/// (ties: more homes first).
pub(super) fn worst_area(r: &CityReport, f: impl Fn(&crate::committee::report::AreaSummary) -> f32) -> Option<String> {
    r.areas.iter().filter(|a| a.homes > 0).min_by(|a, b| f(a).total_cmp(&f(b)).then(b.homes.cmp(&a.homes))).map(|a| a.name.clone())
}

pub(super) fn most(r: &CityReport, f: impl Fn(&crate::committee::report::AreaSummary) -> f32) -> Option<String> {
    r.areas.iter().max_by(|a, b| f(a).total_cmp(&f(b))).filter(|a| f(a) > 0.0).map(|a| a.name.clone())
}

/// What the rules would have a member table, with the reason in figures;
/// None when their scope has nothing pressing. Not yet validated.
pub fn suggest(member: &Politician, r: &CityReport) -> Option<(Action, String)> {
    let l = &member.leanings;
    let pct = |x: f32| format!("{:.0}%", x * 100.0);
    let free = |a: &crate::committee::report::AreaSummary| a.free_parcels as f32;
    match member.scope {
        Scope::Transit => {
            if r.depots == 0 && r.population > 150 {
                let near = most(r, |a| a.homes as f32).unwrap_or_default();
                return Some((Action::BuildDepot { near }, format!("{} people and not one bus depot", r.population)));
            }
            let wanted = 1 + r.homes / 1500;
            let falling = r.riders_prev > 30.0 && r.riders_per_day < r.riders_prev * 0.85;
            if r.depots > 0 && (r.bus_lines < wanted || falling) {
                let from = most(r, |a| a.homes as f32)?;
                let to = r.areas.iter().filter(|a| a.name != from).max_by(|a, b| a.jobs.cmp(&b.jobs)).map(|a| a.name.clone())?;
                let why = if falling { format!("riders fell from {:.0} to {:.0} a day", r.riders_prev, r.riders_per_day) } else { format!("{} bus lines for {} homes", r.bus_lines, r.homes) };
                return Some((Action::AddBusLine { from, to }, why));
            }
            if r.tolls > 0 && l.growth < -0.2 {
                return Some((Action::SetToll { toll: None, price: 6.0 }, "make drivers pay for the road and the bus will look better".into()));
            }
            None
        }
        Scope::Housing => {
            let tight = r.homes > 0 && (r.homes_free as f32) < r.homes as f32 * 0.03;
            if r.rent_burden > 0.3 || tight || r.unhoused >= 5 {
                let area = most(r, free)?;
                let why = if r.rent_burden > 0.3 { format!("{} of households pay over 30% of income in rent", pct(r.rent_burden)) } else if r.unhoused >= 5 { format!("{} people unhoused", r.unhoused) } else { format!("only {} free homes", r.homes_free) };
                return Some((Action::Rezone { area, to: "residential".into() }, why));
            }
            None
        }
        Scope::Finance => {
            if r.net_per_day < -50.0 || r.treasury < 10_000.0 {
                if r.road_funding > 1.05 && l.spending < 0.0 {
                    return Some((Action::RoadRepair { funding: 1.0 }, format!("we lose {} a day; the roads can live on what they need", thousands(-r.net_per_day))));
                }
                let category = if member.temper == Temper::Populist { "commercial" } else { "residential" };
                return Some((Action::ChangeTax { category: category.into(), delta_pct: 1.0 }, format!("net {} a day, treasury {}", thousands(r.net_per_day), thousands(r.treasury))));
            }
            if r.net_per_day > 400.0 && r.treasury > 120_000.0 && l.spending > -0.2 {
                return Some((Action::ChangeTax { category: "residential".into(), delta_pct: -1.0 }, format!("a surplus of {} a day: give some back", thousands(r.net_per_day))));
            }
            None
        }
        Scope::Safety => {
            if r.population >= 150 && r.coverage[0] < 0.6 {
                let near = worst_area(r, |a| a.coverage[0]).unwrap_or_default();
                return Some((Action::BuildService { service: "police".into(), tier: "low".into(), near }, share_of_homes(r.coverage[0], "have police cover")));
            }
            if r.population >= 150 && r.coverage[1] < 0.6 {
                let near = worst_area(r, |a| a.coverage[1]).unwrap_or_default();
                return Some((Action::BuildService { service: "fire".into(), tier: "low".into(), near }, share_of_homes(r.coverage[1], "have fire cover")));
            }
            if r.crime_per_day >= 1.0 && r.crime_per_day > r.crime_prev * 1.2 {
                let near = most(r, |a| a.homes as f32).unwrap_or_default();
                return Some((Action::BuildService { service: "police".into(), tier: "mid".into(), near }, format!("crime up from {:.1} to {:.1} a day", r.crime_prev, r.crime_per_day)));
            }
            None
        }
        Scope::Environment => {
            if let Some(a) = r.areas.iter().filter(|a| a.contamination >= 1.0).max_by(|a, b| a.contamination.total_cmp(&b.contamination)) {
                return Some((Action::Remediate { area: a.name.clone() }, format!("{:.0} cells' worth of poisoned ground in {}", a.contamination, a.name)));
            }
            if r.population >= 150 && (r.demand[5] > 0.4 || l.growth < -0.3) {
                let area = most(r, free)?;
                return Some((Action::Rezone { area, to: "park".into() }, format!("park demand at {}", pct(r.demand[5]))));
            }
            None
        }
        Scope::Business => {
            if r.population >= 150 && r.unemployment > 0.08 {
                let area = most(r, free)?;
                let to = if l.growth > 0.2 { "industrial" } else { "commercial" };
                return Some((Action::Rezone { area, to: to.into() }, format!("unemployment at {}", pct(r.unemployment))));
            }
            if r.demand[1] > 0.55 {
                let area = most(r, free)?;
                return Some((Action::Rezone { area, to: "commercial".into() }, format!("commercial demand at {}", pct(r.demand[1]))));
            }
            if r.net_per_day > 0.0 && l.growth > 0.4 {
                return Some((Action::ChangeTax { category: "commercial".into(), delta_pct: -1.0 }, "lighter taxes bring the shops in".into()));
            }
            None
        }
        Scope::Utilities => {
            if r.unpowered > 0 || r.brownout_hours_week > 0 {
                let plant = if l.growth > 0.3 { "coal" } else { "solar" };
                let near = most(r, |a| a.homes as f32).unwrap_or_default();
                return Some((Action::BuildPower { plant: plant.into(), near }, format!("{} buildings without power, {} brownout hours this week", r.unpowered, r.brownout_hours_week)));
            }
            if r.road_wear > 0.4 && r.road_funding < 1.3 {
                return Some((Action::RoadRepair { funding: (r.road_funding + 0.3).min(crate::road_wear::MAX_FUNDING) }, format!("road wear at {}", pct(r.road_wear))));
            }
            None
        }
        Scope::Welfare => {
            if r.population >= 150 && r.coverage[2] < 0.6 {
                let near = worst_area(r, |a| a.coverage[2]).unwrap_or_default();
                return Some((Action::BuildService { service: "health".into(), tier: "low".into(), near }, share_of_homes(r.coverage[2], "are near a clinic")));
            }
            if r.population >= 150 && r.coverage[3] < 0.5 {
                let near = worst_area(r, |a| a.coverage[3]).unwrap_or_default();
                return Some((Action::BuildService { service: "school".into(), tier: "low".into(), near }, share_of_homes(r.coverage[3], "are near a school")));
            }
            None
        }
    }
}

/// "no homes have police cover" / "only 34% of homes have police cover".
pub(super) fn share_of_homes(x: f32, what: &str) -> String {
    if x < 0.005 {
        format!("no homes {what}")
    } else {
        format!("only {:.0}% of homes {what}", x * 100.0)
    }
}

/// How pressing a member's topic is, for choosing who tables (higher
/// first): the worst firing rule in their scope.
pub fn urgency(member: &Politician, r: &CityReport, c: &Committee, today: u32) -> u8 {
    crate::committee::rules::firing(r, c, today).iter().filter(|(rule, _, _)| rule.scope == member.scope).map(|(_, s, _)| *s).max().unwrap_or(0)
}

/// How a member would vote by their leanings, temper and memory, and the
/// main reason. The stand-in votes so; a model is shown it as a hint and
/// may differ.
pub fn lean(member: &Politician, p: &Proposal, proposer: Option<&Politician>, r: &CityReport, seed: u64) -> (Vote, String) {
    if p.by == member.id {
        return (Vote::Yes, "I tabled it".into());
    }
    let l = &member.leanings;
    let (growth, order, relief) = p.action.traits();
    let mut reasons: Vec<(f32, String)> = Vec::new();
    let mut add = |v: f32, why: String| reasons.push((v, why));
    add(0.1, "it does no harm".into());
    if p.action.scope() == member.scope {
        add(0.6, "it is my portfolio and it is overdue".into());
    }
    if growth.abs() > 0.05 {
        let v = l.growth * growth * 0.9;
        add(v, if v >= 0.0 { if growth > 0.0 { "growth and jobs come first".into() } else { "clean ground matters more than another tower".into() } } else if growth > 0.0 { "it dirties the city for a few jobs".into() } else { "it holds the city back".into() });
    }
    if order.abs() > 0.05 {
        let v = l.order * order * 0.8;
        add(v, if v >= 0.0 { "people want to feel safe".into() } else { "we do not need more uniforms on the street".into() });
    }
    if relief.abs() > 0.05 {
        let mut v = relief * 0.4;
        if member.temper == Temper::Populist {
            v *= 2.0;
        }
        add(v, if v >= 0.0 { "it helps ordinary people".into() } else { "it hits ordinary people".into() });
    }
    if p.cost > 0.0 {
        let burden = (p.cost / r.treasury.max(2_000.0)).min(1.5);
        let v = if l.spending < 0.0 { l.spending * burden * 1.3 } else { l.spending * 0.25 };
        add(v, if v < 0.0 { format!("{} credits we cannot spare", thousands(p.cost)) } else { "worth every credit".into() });
    }
    if let Action::ChangeTax { delta_pct, .. } = &p.action {
        let v = if *delta_pct > 0.0 { l.spending * 0.4 - 0.15 } else { -l.spending * 0.4 + 0.1 };
        add(v, if *delta_pct > 0.0 { if v >= 0.0 { "the city needs the revenue".into() } else { "no new taxes".into() } } else if v >= 0.0 { "people keep more of what they earn".into() } else { "we cannot afford a tax cut".into() });
    }
    // Memory: consistent with the last vote on the same kind, unless it
    // went badly.
    if let Some(m) = member.past_vote_on(p.action.kind_key()) {
        let sign = match m.vote {
            Some(Vote::Yes) => 1.0,
            Some(Vote::No) => -1.0,
            _ => 0.0,
        };
        let k = if m.verdict < 0 { -0.25 } else { 0.4 };
        if sign != 0.0 {
            add(sign * k, if sign * k > 0.0 { format!("I voted {} on \"{}\" on day {} and I stand by it", m.vote.map_or("", |v| v.key()), m.proposal, m.day + 1) } else { format!("\"{}\" did not work out, and I have learned from it", m.proposal) });
        }
    }
    match member.temper {
        Temper::Contrarian if proposer.is_some_and(|x| x.scope != member.scope) => add(-0.45, "somebody has to say no".into()),
        Temper::OldGuard if matches!(p.action, Action::AddBusLine { .. } | Action::Rezone { .. } | Action::BuildPower { .. }) => add(-0.3, "we have tried new things before".into()),
        Temper::Dealmaker if proposer.is_some_and(|x| x.party == member.party) => add(0.35, "a friend's proposal; they will remember".into()),
        Temper::Idealist => {
            for r in reasons.iter_mut() {
                r.0 *= 1.3;
            }
        }
        _ => {}
    }
    let noise = ((mix(seed, (member.id as u64) << 16 | p.id as u64) % 1000) as f32 / 1000.0 - 0.5) * 0.3;
    let score: f32 = reasons.iter().map(|r| r.0).sum::<f32>() + noise;
    let firm = matches!(member.temper, Temper::Firebrand | Temper::Idealist);
    let vote = if score > 0.2 || (firm && score > 0.0) {
        Vote::Yes
    } else if score < -0.2 || (firm && score <= 0.0) {
        Vote::No
    } else {
        Vote::Abstain
    };
    let reason = match vote {
        Vote::Yes => reasons.iter().filter(|r| r.0 > 0.0).max_by(|a, b| a.0.total_cmp(&b.0)),
        Vote::No => reasons.iter().filter(|r| r.0 < 0.0).min_by(|a, b| a.0.total_cmp(&b.0)),
        Vote::Abstain => None,
    }
    .map_or_else(|| "the case is not made either way".to_string(), |r| r.1.clone());
    (vote, vary(&reason, mix(seed, (member.id as u64) << 20 | p.id as u64)))
}

/// Other ways of saying the stock reasons, so a table of six does not
/// sound like one voice.
pub(super) fn vary(reason: &str, h: u64) -> String {
    let alts: &[&str] = match reason {
        "it helps ordinary people" => &["it helps ordinary people", "people on my street need this", "it makes life easier for working families", "it is the decent thing to do"],
        "the case is not made either way" => &["the case is not made either way", "I am not convinced, one way or the other", "I need to see more before I commit", "it could go either way"],
        "somebody has to say no" => &["somebody has to say no", "too easy a consensus", "I do not trust a vote this comfortable", "nobody has asked what it breaks"],
        "it does no harm" => &["it does no harm", "no reason to stand in the way", "fine by me"],
        "people want to feel safe" => &["people want to feel safe", "safe streets come first", "my voters want a patrol car they can see"],
        "it is my portfolio and it is overdue" => &["it is my portfolio and it is overdue", "this is my brief, and it is late", "I have asked for this for weeks"],
        "we have tried new things before" => &["we have tried new things before", "untested, and I have seen how that ends", "the old way worked well enough"],
        _ => &[],
    };
    if alts.is_empty() {
        reason.to_string()
    } else {
        alts[(h % alts.len() as u64) as usize].to_string()
    }
}
