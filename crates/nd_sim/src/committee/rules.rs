//! [committee] Alerts between meetings: plain rules on the city report, one
//! set per scope. A seated member whose scope a rule belongs to raises it
//! (the first such member, in seat order); nobody watches a scope with no
//! member, so a committee without a treasurer hears nothing about the
//! deficit until it is a meeting topic. Each rule has a cooldown so a
//! standing problem is not raised every day. The backend only phrases an
//! alert; whether there is one is decided here.

use super::report::{CityReport, Fact};
use super::{Alert, Committee, Scope};

pub struct Rule {
    pub key: &'static str,
    pub scope: Scope,
    /// Days before it may fire again.
    pub cooldown: u32,
    /// Plain words for the window and the docs.
    pub when: &'static str,
    pub check: fn(&CityReport, &Committee, u32) -> Option<(u8, Vec<Fact>)>,
}

/// A population below which most rules stay quiet (a hamlet has no crime
/// wave).
const TOWN: u32 = 150;

pub const RULES: &[Rule] = &[
    Rule {
        key: "transit_riders_down",
        scope: Scope::Transit,
        cooldown: 7,
        when: "transit riders down 20% or more week on week",
        check: |r, _, _| {
            (r.riders_prev >= 30.0 && r.riders_per_day <= r.riders_prev * 0.8).then(|| {
                let sev = if r.riders_per_day <= r.riders_prev * 0.6 { 3 } else { 2 };
                (sev, vec![Fact::new("transit riders", r.riders_per_day, "/day").vs(r.riders_prev)])
            })
        },
    },
    Rule {
        key: "transit_no_buses",
        scope: Scope::Transit,
        cooldown: 30,
        when: "a town of over 400 people with no bus line",
        check: |r, _, _| (r.population > 400 && r.bus_lines == 0).then(|| (1, vec![Fact::new("population", r.population as f32, ""), Fact::new("bus lines", 0.0, "")])),
    },
    Rule {
        key: "housing_rent_burden",
        scope: Scope::Housing,
        cooldown: 7,
        when: "over 35% of households pay more than 30% of income in rent, or that share is up 5 points in a week",
        check: |r, c, today| {
            if r.population < TOWN {
                return None;
            }
            let before = c.sample_before(today, 7).map(|s| s.rent_burden);
            let rising = before.is_some_and(|b| r.rent_burden >= b + 0.05);
            (r.rent_burden > 0.35 || rising).then(|| {
                let f = Fact::new("households over 30% rent burden", r.rent_burden, "%");
                (if r.rent_burden > 0.5 { 3 } else { 2 }, vec![match before {
                    Some(b) => f.vs(b),
                    None => f,
                }])
            })
        },
    },
    Rule {
        key: "housing_unhoused",
        scope: Scope::Housing,
        cooldown: 7,
        when: "more than 2% of people (and at least 5) unhoused",
        check: |r, _, _| {
            let share = r.unhoused as f32 / r.population.max(1) as f32;
            (r.unhoused >= 5 && share > 0.02).then(|| (if share > 0.06 { 3 } else { 2 }, vec![Fact::new("people unhoused", r.unhoused as f32, ""), Fact::new("share of the population", share, "%")]))
        },
    },
    Rule {
        key: "housing_shortage",
        scope: Scope::Housing,
        cooldown: 10,
        when: "under 2% of homes free while 20 or more wait to move in",
        check: |r, _, _| {
            let free = r.homes_free as f32 / r.homes.max(1) as f32;
            (r.homes > 0 && free < 0.02 && r.waiting >= 20).then(|| (2, vec![Fact::new("free homes", r.homes_free as f32, ""), Fact::new("waiting to move in", r.waiting as f32, "")]))
        },
    },
    Rule {
        key: "finance_deficit",
        scope: Scope::Finance,
        cooldown: 7,
        when: "the city spends more than it takes in, on a 7-day mean",
        check: |r, _, _| (r.income_per_day > 0.0 && r.net_per_day < -50.0).then(|| (if r.treasury < 0.0 { 3 } else { 2 }, vec![Fact::new("net per day", r.net_per_day, "credits"), Fact::new("treasury", r.treasury, "credits")])),
    },
    Rule {
        key: "finance_low_treasury",
        scope: Scope::Finance,
        cooldown: 5,
        when: "the treasury is under 10,000",
        check: |r, _, _| (r.treasury < 10_000.0).then(|| (if r.treasury < 0.0 { 3 } else { 2 }, vec![Fact::new("treasury", r.treasury, "credits")])),
    },
    Rule {
        key: "safety_crime_up",
        scope: Scope::Safety,
        cooldown: 7,
        when: "crime up 25% or more week on week (at least one a day)",
        check: |r, _, _| (r.crime_per_day >= 1.0 && r.crime_prev > 0.0 && r.crime_per_day >= r.crime_prev * 1.25).then(|| (2, vec![Fact::new("crimes", r.crime_per_day, "/day").vs(r.crime_prev)])),
    },
    Rule {
        key: "safety_police_coverage",
        scope: Scope::Safety,
        cooldown: 14,
        when: "under half the homes within reach of a police station",
        check: |r, _, _| (r.population >= TOWN && r.coverage[0] < 0.5).then(|| (if r.coverage[0] < 0.2 { 2 } else { 1 }, vec![Fact::new("homes with police cover", r.coverage[0], "%")])),
    },
    Rule {
        key: "safety_fire_coverage",
        scope: Scope::Safety,
        cooldown: 14,
        when: "under half the homes within reach of a fire station",
        check: |r, _, _| (r.population >= TOWN && r.coverage[1] < 0.5).then(|| (1, vec![Fact::new("homes with fire cover", r.coverage[1], "%")])),
    },
    Rule {
        key: "safety_unanswered",
        scope: Scope::Safety,
        cooldown: 7,
        when: "emergency calls go unanswered (half a call a day or more)",
        check: |r, _, _| (r.unanswered_per_day >= 0.5).then(|| (3, vec![Fact::new("unanswered calls", r.unanswered_per_day, "/day")])),
    },
    Rule {
        key: "environment_contamination_spreading",
        scope: Scope::Environment,
        cooldown: 7,
        when: "contaminated ground up 10% (and at least two cells) in a week",
        check: |r, c, today| {
            let before = c.sample_before(today, 7)?.contaminated_cells;
            (r.contaminated_cells >= before + 2 && r.contaminated_cells as f32 >= before as f32 * 1.1).then(|| (2, vec![Fact::new("contaminated ground cells", r.contaminated_cells as f32, "").vs(before as f32)]))
        },
    },
    Rule {
        key: "environment_hotspot",
        scope: Scope::Environment,
        cooldown: 14,
        when: "a patch of ground over 60% contaminated",
        check: |r, _, _| (r.contamination_peak > 0.6).then(|| (2, vec![Fact::new("worst contamination", r.contamination_peak, "%"), Fact::new("contaminated ground cells", r.contaminated_cells as f32, "")])),
    },
    Rule {
        key: "business_unemployment",
        scope: Scope::Business,
        cooldown: 7,
        when: "unemployment over 10%",
        check: |r, _, _| (r.population >= TOWN && r.unemployment > 0.10).then(|| (if r.unemployment > 0.2 { 3 } else { 2 }, vec![Fact::new("unemployment", r.unemployment, "%"), Fact::new("open jobs", r.jobs_free as f32, "")])),
    },
    Rule {
        key: "business_demand_unmet",
        scope: Scope::Business,
        cooldown: 14,
        when: "commercial demand over 70%",
        check: |r, _, _| (r.population >= TOWN && r.demand[1] > 0.7).then(|| (1, vec![Fact::new("commercial demand", r.demand[1], "%")])),
    },
    Rule {
        key: "utilities_unpowered",
        scope: Scope::Utilities,
        cooldown: 3,
        when: "any building without power",
        check: |r, _, _| (r.unpowered > 0).then(|| (if r.unpowered > 20 { 3 } else { 2 }, vec![Fact::new("unpowered buildings", r.unpowered as f32, "")])),
    },
    Rule {
        key: "utilities_brownouts",
        scope: Scope::Utilities,
        cooldown: 7,
        when: "brownouts this week",
        check: |r, _, _| (r.brownout_hours_week > 0).then(|| (2, vec![Fact::new("brownout hours this week", r.brownout_hours_week as f32, "")])),
    },
    Rule {
        key: "utilities_road_wear",
        scope: Scope::Utilities,
        cooldown: 14,
        when: "streets worn past 45% on average",
        check: |r, _, _| (r.road_wear > 0.45).then(|| (if r.road_wear > 0.65 { 3 } else { 1 }, vec![Fact::new("road wear", r.road_wear, "%"), Fact::new("road maintenance funding", r.road_funding, "%")])),
    },
    Rule {
        key: "welfare_health_coverage",
        scope: Scope::Welfare,
        cooldown: 14,
        when: "under half the homes within reach of a clinic or hospital",
        check: |r, _, _| (r.population >= TOWN && r.coverage[2] < 0.5).then(|| (if r.coverage[2] < 0.2 { 2 } else { 1 }, vec![Fact::new("homes near a clinic", r.coverage[2], "%")])),
    },
    Rule {
        key: "welfare_school_coverage",
        scope: Scope::Welfare,
        cooldown: 14,
        when: "under 40% of homes within reach of a school",
        check: |r, _, _| (r.population >= TOWN && r.coverage[3] < 0.4).then(|| (1, vec![Fact::new("homes near a school", r.coverage[3], "%")])),
    },
];

pub fn rule(key: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.key == key)
}

/// Every rule that holds today, ignoring who sits and the cooldowns:
/// (rule, severity, facts). The meeting uses it for the topics of concern.
pub fn firing(report: &CityReport, c: &Committee, today: u32) -> Vec<(&'static Rule, u8, Vec<Fact>)> {
    RULES.iter().filter_map(|r| (r.check)(report, c, today).map(|(s, f)| (r, s, f))).collect()
}

/// Check every rule; raise an alert from the watching member for each that
/// holds and is off its cooldown. The text is a placeholder until a backend
/// phrases it (the app asks; a plain line stands in). Returns the new
/// alerts' ids.
pub fn raise_alerts(c: &mut Committee, report: &CityReport, today: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for (rule, severity, facts) in firing(report, c, today) {
        let Some(member) = c.watcher(rule.scope) else { continue };
        if c.rule_last.get(rule.key).is_some_and(|&d| today < d + rule.cooldown) {
            continue;
        }
        c.rule_last.insert(rule.key.to_string(), today);
        let text = format!("{}: {}.", capitalise_first(rule.when), facts.iter().map(Fact::line).collect::<Vec<_>>().join(", "));
        out.push(c.push_alert(Alert { day: today, member, rule: rule.key.into(), severity, facts, text, ..Default::default() }));
    }
    out
}

fn capitalise_first(s: &str) -> String {
    super::capitalise(s)
}
