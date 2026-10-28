//! [stories] District stories: a named preset with a short backstory that
//! sets a district's architecture (homes look, shops look, industry
//! sub-zone, Historic freeze, height limit) and its own policies (district
//! taxes and levers, [`Policies`]). The player picks a story for a district
//! and may fine-tune its policy dials and height limit; the building types
//! only change by switching the story.
//!
//! The story → architecture mapping lives in one place, [`Story::preset`]:
//! each story reuses today's looks until it gets its own (the planned look
//! is [`Story::planned_look`]); a new look drops in there as a new
//! `ResidentialSpec`/`CommercialSpec` (as Capsule did) and one line here.
//!
//! What each policy does in the sim (`world/stories.rs`): every amount
//! moves through the ledger's door (`World::pay_city`/`city_pays`), on the
//! city's books as district levies or district spending.

use crate::districts::{CommercialSpec, District, ResidentialSpec};
use crate::industry::IndustryKind;
use serde::{Deserialize, Serialize};

/// Highest share of residents' rent the city may pay in a district.
pub const MAX_RENT_SUPPORT: f32 = 0.5;
/// Highest levy on nightlife or industrial takings.
pub const MAX_LEVY: f32 = 0.2;
/// Highest heritage grant, dollars a building a day.
pub const MAX_HERITAGE_GRANT: f32 = 100.0;
/// Business-rate range on shops' and offices' takings: relief below 0.
pub const MIN_BUSINESS_RATE: f32 = -0.05;
pub const MAX_BUSINESS_RATE: f32 = 0.10;
/// Hours a late licence keeps nightlife open past its usual close.
pub const LATE_HOURS: u8 = 2;

/// A district's own taxes and levers. All zero/off is no policy at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Policies {
    /// Share of residents' rent the city pays them back (student or key
    /// worker rent support). Supported homes look cheaper to people
    /// choosing where to live.
    #[serde(default)]
    pub rent_support: f32,
    /// Noise and alcohol levy: share of venues' and bars' takings to the city.
    #[serde(default)]
    pub nightlife_levy: f32,
    /// Late licence: venues and bars open `LATE_HOURS` longer; the
    /// residents hear it (a little mood a night).
    #[serde(default)]
    pub late_hours: bool,
    /// Pollution levy: share of industry's takings to the city; operators
    /// fit filters, so the district's dirty plants foul less.
    #[serde(default)]
    pub pollution_levy: f32,
    /// Heritage grant: dollars a day the city pays the owner of each
    /// standing building for its upkeep; the buildings wear more slowly.
    #[serde(default)]
    pub heritage_grant: f32,
    /// Business rate on shops' and offices' takings: a levy above 0, relief
    /// (the city pays) below.
    #[serde(default)]
    pub business_rate: f32,
    /// Bed levy: share of a hotel night added to the tourist tax here.
    #[serde(default)]
    pub bed_levy: f32,
}

/// One policy dial, for the books and the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Policy {
    RentSupport,
    NightlifeLevy,
    LateHours,
    PollutionLevy,
    HeritageGrant,
    BusinessRate,
    BedLevy,
}

/// Number of [`Policy`]s: the district books' row length.
pub const POLICIES: usize = 7;

impl Policy {
    pub const ALL: [Policy; POLICIES] = [Policy::RentSupport, Policy::NightlifeLevy, Policy::LateHours, Policy::PollutionLevy, Policy::HeritageGrant, Policy::BusinessRate, Policy::BedLevy];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Policy::RentSupport => "rent support",
            Policy::NightlifeLevy => "noise & alcohol levy",
            Policy::LateHours => "late licence",
            Policy::PollutionLevy => "pollution levy",
            Policy::HeritageGrant => "heritage grant",
            Policy::BusinessRate => "business rate",
            Policy::BedLevy => "bed levy",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Policy::RentSupport => "rent_support",
            Policy::NightlifeLevy => "nightlife_levy",
            Policy::LateHours => "late_hours",
            Policy::PollutionLevy => "pollution_levy",
            Policy::HeritageGrant => "heritage_grant",
            Policy::BusinessRate => "business_rate",
            Policy::BedLevy => "bed_levy",
        }
    }

    /// What it does, for the UI's hover.
    pub fn help(self) -> &'static str {
        match self {
            Policy::RentSupport => "The city pays residents back this share of their rent (student or key-worker support). Homes here look cheaper to people choosing where to live, and more homes go up.",
            Policy::NightlifeLevy => "Venues and bars pay this share of their takings to the city. Fewer new venues open here.",
            Policy::LateHours => "Venues and bars stay open two hours later: more custom, but residents hear it (a little mood a night). Nightlife builds here more readily.",
            Policy::PollutionLevy => "Industry pays this share of its takings to the city and fits filters: its plants foul the neighbours less. Less industry opens here.",
            Policy::HeritageGrant => "The city pays each building's owner this much a day for upkeep: buildings here wear more slowly.",
            Policy::BusinessRate => "Shops and offices pay this share of their takings on top of business tax; below zero the city pays them (relief). Relief draws shops, a levy puts them off.",
            Policy::BedLevy => "Added to the tourist tax on every hotel night here.",
        }
    }

    /// `(min, max)` of the dial (the late licence is on or off).
    pub fn range(self) -> (f32, f32) {
        match self {
            Policy::RentSupport => (0.0, MAX_RENT_SUPPORT),
            Policy::NightlifeLevy | Policy::PollutionLevy | Policy::BedLevy => (0.0, MAX_LEVY),
            Policy::LateHours => (0.0, 1.0),
            Policy::HeritageGrant => (0.0, MAX_HERITAGE_GRANT),
            Policy::BusinessRate => (MIN_BUSINESS_RATE, MAX_BUSINESS_RATE),
        }
    }

    /// A share (shown in %) rather than dollars or a switch.
    pub fn is_share(self) -> bool {
        !matches!(self, Policy::LateHours | Policy::HeritageGrant)
    }

    pub fn parse(s: &str) -> Option<Policy> {
        let s = s.trim().to_ascii_lowercase();
        Policy::ALL.into_iter().find(|p| p.key() == s || p.label() == s)
    }
}

impl Policies {
    /// The dial's value (the late licence as 0 or 1).
    pub fn get(&self, p: Policy) -> f32 {
        match p {
            Policy::RentSupport => self.rent_support,
            Policy::NightlifeLevy => self.nightlife_levy,
            Policy::LateHours => self.late_hours as u8 as f32,
            Policy::PollutionLevy => self.pollution_levy,
            Policy::HeritageGrant => self.heritage_grant,
            Policy::BusinessRate => self.business_rate,
            Policy::BedLevy => self.bed_levy,
        }
    }

    /// Set a dial, clamped to its range.
    pub fn set(&mut self, p: Policy, v: f32) {
        let (lo, hi) = p.range();
        let v = if v.is_finite() { v.clamp(lo, hi) } else { 0.0 };
        match p {
            Policy::RentSupport => self.rent_support = v,
            Policy::NightlifeLevy => self.nightlife_levy = v,
            Policy::LateHours => self.late_hours = v >= 0.5,
            Policy::PollutionLevy => self.pollution_levy = v,
            Policy::HeritageGrant => self.heritage_grant = v,
            Policy::BusinessRate => self.business_rate = v,
            Policy::BedLevy => self.bed_levy = v,
        }
    }

    /// Every dial within its range (a hand-edited save, the API).
    pub fn clamped(mut self) -> Self {
        for p in Policy::ALL {
            self.set(p, self.get(p));
        }
        self
    }

    pub fn is_none(&self) -> bool {
        Policy::ALL.iter().all(|p| self.get(*p) == 0.0)
    }

    /// The dial's value in words ("25%", "$40/building/day", "on").
    pub fn value_label(&self, p: Policy) -> String {
        let v = self.get(p);
        match p {
            Policy::LateHours => if self.late_hours { "on".into() } else { "off".into() },
            Policy::HeritageGrant => format!("${v:.0}/building/day"),
            Policy::BusinessRate if v < 0.0 => format!("{:.0}% relief", -v * 100.0),
            _ => format!("{:.0}%", v * 100.0),
        }
    }

    /// The dials that are set, in words: "rent support 25%, late licence on".
    pub fn summary(&self) -> String {
        let v: Vec<String> = Policy::ALL.iter().filter(|p| self.get(**p) != 0.0).map(|p| format!("{} {}", p.label(), self.value_label(*p))).collect();
        if v.is_empty() {
            "no district taxes".into()
        } else {
            v.join(", ")
        }
    }

    /// How readily new buildings of `lu` go up here, from the policies
    /// (1 = as anywhere): support and relief draw, levies put off.
    pub fn pull(&self, lu: crate::zoning::LandUse) -> f32 {
        use crate::zoning::LandUse;
        let p = match lu {
            LandUse::Residential => (1.0 + self.rent_support) * if self.late_hours { 0.95 } else { 1.0 },
            LandUse::Commercial => 1.0 - 4.0 * self.business_rate,
            LandUse::Entertainment => (1.0 - 2.5 * self.nightlife_levy) * if self.late_hours { 1.2 } else { 1.0 },
            LandUse::Industrial => 1.0 - 2.5 * self.pollution_levy,
            _ => 1.0,
        };
        p.clamp(0.5, 1.5)
    }
}

/// What a story sets: its architecture and its starting policies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub residential: Option<ResidentialSpec>,
    pub commercial: Option<CommercialSpec>,
    pub industry: Option<IndustryKind>,
    pub historic: bool,
    pub height_limit: Option<f32>,
    pub policies: Policies,
}

/// A district's story.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Story {
    Capsule,
    EcoVillage,
    WalledQuarter,
    AfterglowMile,
    FoundersCampus,
    OldTownCharter,
    QuietAcres,
    Promenade,
    FoundryRow,
    LanternLanes,
    Shantytown,
    NeoTokyoHeights,
}

impl Story {
    pub const ALL: [Story; 12] = [
        Story::Capsule,
        Story::EcoVillage,
        Story::WalledQuarter,
        Story::AfterglowMile,
        Story::FoundersCampus,
        Story::OldTownCharter,
        Story::QuietAcres,
        Story::Promenade,
        Story::FoundryRow,
        Story::LanternLanes,
        Story::Shantytown,
        Story::NeoTokyoHeights,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Story::Capsule => "Capsule",
            Story::EcoVillage => "Eco Village",
            Story::WalledQuarter => "The Walled Quarter",
            Story::AfterglowMile => "Afterglow Mile",
            Story::FoundersCampus => "Founders' Campus",
            Story::OldTownCharter => "Old Town Charter",
            Story::QuietAcres => "Quiet Acres",
            Story::Promenade => "The Promenade",
            Story::FoundryRow => "Foundry Row",
            Story::LanternLanes => "Lantern Lanes",
            Story::Shantytown => "Riverbank Shantytown",
            Story::NeoTokyoHeights => "Neo Tokyo Heights",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Story::Capsule => "capsule",
            Story::EcoVillage => "eco_village",
            Story::WalledQuarter => "walled_quarter",
            Story::AfterglowMile => "afterglow_mile",
            Story::FoundersCampus => "founders_campus",
            Story::OldTownCharter => "old_town_charter",
            Story::QuietAcres => "quiet_acres",
            Story::Promenade => "promenade",
            Story::FoundryRow => "foundry_row",
            Story::LanternLanes => "lantern_lanes",
            Story::Shantytown => "shantytown",
            Story::NeoTokyoHeights => "neo_tokyo_heights",
        }
    }

    /// By key or label, case-insensitive ("Eco Village", "eco_village").
    pub fn parse(s: &str) -> Option<Story> {
        let s = s.trim().to_ascii_lowercase();
        let k = s.replace([' ', '-'], "_").replace('\'', "");
        Story::ALL.into_iter().find(|x| x.key() == k || x.label().to_ascii_lowercase() == s || x.key().trim_start_matches("the_") == k.trim_start_matches("the_"))
    }

    /// The short backstory shown in the picker.
    pub fn backstory(self) -> &'static str {
        match self {
            Story::Capsule => "An eccentric inventor won this land in a grant and swore to build the future on it, domes and all, after the ship that came down in the hills. Only high tech opens here; start-ups get rate relief.",
            Story::EcoVillage => "A co-op of growers and engineers bought out the old allotments: planted towers, solar roofs, vertical farms. Dirty industry pays a stiff levy to stay.",
            Story::WalledQuarter => "Nobody planned it and nobody inspects it: flats stacked on flats, stalls in every stairwell, open all hours. Cheap to live in, cheap to trade in.",
            Story::AfterglowMile => "The strip that never sleeps: karaoke towers, signs over every door, a late licence and a levy on every round to pay for the street cleaners.",
            Story::FoundersCampus => "Two dropouts' garage became a campus, and the campus became a district. Glass offices and labs; the city pays part of students' and starters' rent.",
            Story::OldTownCharter => "The old town's charter freezes what stands: no towers, no redevelopment, just repairs. Owners get heritage grants; visitors pay a small bed levy.",
            Story::QuietAcres => "Front gardens, cul-de-sacs, porch lights and dark streets. Low houses only, and venues pay a noise levy to keep the nights quiet.",
            Story::Promenade => "The harbour front made over for visitors: hotels first, shops for strollers, planted terraces. A bed levy pays for the boardwalk.",
            Story::FoundryRow => "Brick, steel and furnace glow: the works and the rows of workers' homes beside them. A pollution levy pays for the filters; key workers get rent support.",
            Story::LanternLanes => "Paper lanterns over narrow lanes, stalls under tarps, kitchens open late. Stallholders pay a lower rate and the night market keeps going past midnight.",
            Story::Shantytown => "The riverbank homes nobody planned: low shacks and stilt houses, water tanks on every roof. The city helps with the rent until something better comes.",
            Story::NeoTokyoHeights => "Glass towers for the rich over a hectic street: sky bridges, rooftop pools, neon at their feet. Shops pay a luxury rate; the street runs late.",
        }
    }

    /// The look the story will get its own buildings in (Round 4); until
    /// then it reuses today's (`preset`).
    pub fn planned_look(self) -> &'static str {
        match self {
            Story::Capsule => "Capsule",
            Story::EcoVillage => "Solarpunk Green",
            Story::WalledQuarter => "Kowloon stacks",
            Story::AfterglowMile => "Neon strip",
            Story::FoundersCampus => "Glass Campus",
            Story::OldTownCharter => "Old Stone",
            Story::QuietAcres => "Garden Suburb",
            Story::Promenade => "Harbour Pastel",
            Story::FoundryRow => "Brick & Steel",
            Story::LanternLanes => "Lantern Alley",
            Story::Shantytown => "Patchwork",
            Story::NeoTokyoHeights => "Skyline Luxe",
        }
    }

    /// What the story sets. **The one place a story maps to its looks**:
    /// when a story gets its own look, add it as a `ResidentialSpec` /
    /// `CommercialSpec` (appended, as Capsule was) and change its line here.
    pub fn preset(self) -> Preset {
        use CommercialSpec as C;
        use ResidentialSpec as R;
        let p = Policies::default();
        let (residential, commercial, industry, historic, height_limit, policies) = match self {
            Story::Capsule => (Some(R::Capsule), Some(C::Capsule), None, false, None, Policies { business_rate: -0.03, ..p }),
            // [r4-solarpunk] Its own look, Solarpunk Green, for homes, shops
            // and its small industry; still a farming district (its farms
            // keep their farmsteads).
            Story::EcoVillage => (Some(R::SolarpunkGreen), Some(C::SolarpunkGreen), Some(IndustryKind::Farming), false, Some(40.0), Policies { pollution_levy: 0.10, ..p }),
            Story::WalledQuarter => (Some(R::Kowloon), Some(C::NightMarket), None, false, None, Policies { late_hours: true, business_rate: -0.02, ..p }),
            Story::AfterglowMile => (None, Some(C::NeonStrip), None, false, None, Policies { late_hours: true, nightlife_levy: 0.08, ..p }),
            Story::FoundersCampus => (Some(R::Eco), Some(C::TechCampus), None, false, None, Policies { rent_support: 0.15, business_rate: -0.02, ..p }),
            Story::OldTownCharter => (None, None, None, true, Some(24.0), Policies { heritage_grant: 40.0, bed_levy: 0.05, ..p }),
            // [garden] The Garden Suburb, homes and shops.
            Story::QuietAcres => (Some(R::Garden), Some(C::Garden), None, false, Some(18.0), Policies { nightlife_levy: 0.10, ..p }),
            // [r4-harbour] Its own look, Harbour Pastel, for homes, the front and its small industry.
            Story::Promenade => (Some(R::HarbourPastel), Some(C::HarbourPastel), None, false, Some(60.0), Policies { bed_levy: 0.05, ..p }),
            Story::FoundryRow => (Some(R::BrickSteel), Some(C::BrickSteel), Some(IndustryKind::Manufacturing), false, None, Policies { pollution_levy: 0.05, rent_support: 0.10, ..p }),
            Story::LanternLanes => (Some(R::Lantern), Some(C::Lantern), None, false, None, Policies { late_hours: true, business_rate: -0.02, ..p }),
            // [r4-patchwork] Its own look, Patchwork, for homes, its trade and
            // its small industry; the 12 m limit and rent support kept.
            Story::Shantytown => (Some(R::Patchwork), Some(C::Patchwork), None, false, Some(12.0), Policies { rent_support: 0.20, ..p }),
            Story::NeoTokyoHeights => (Some(R::Eco), Some(C::NeonStrip), None, false, None, Policies { business_rate: 0.03, late_hours: true, ..p }),
        };
        Preset { residential, commercial, industry, historic, height_limit, policies }
    }

    /// [malls] How readily strip malls and malls go up in the district,
    /// against a district with no story (1): the suburb's shopping parades
    /// and the neon strip's malls most, the night market's stalls and the
    /// walled quarter's alleys least, none behind the old town's charter.
    pub fn mall_bias(self) -> f32 {
        match self {
            Story::QuietAcres => 1.6,
            Story::AfterglowMile | Story::NeoTokyoHeights => 1.3,
            Story::Promenade => 1.0,
            Story::FoundersCampus | Story::FoundryRow => 0.8,
            Story::EcoVillage | Story::Capsule | Story::Shantytown => 0.5,
            Story::WalledQuarter => 0.4,
            Story::LanternLanes => 0.3,
            Story::OldTownCharter => 0.0,
        }
    }

    /// What the story sets, in words, for the picker.
    pub fn sets(self) -> String {
        let p = self.preset();
        let mut v = vec![
            format!("homes: {}", p.residential.map_or("any", |r| r.label())),
            format!("shops: {}", p.commercial.map_or("any", |c| c.label())),
            format!("industry: {}", p.industry.map_or("by the ground", |i| i.label())),
        ];
        if p.historic {
            v.push("Historic freeze".into());
        }
        v.push(match p.height_limit {
            Some(m) => format!("height limit {m:.0} m"),
            None => "no height limit".into(),
        });
        format!("{}; {}", v.join(", "), p.policies.summary())
    }

    /// Whether `d`'s building-type dials are exactly this story's.
    pub fn fits(self, d: &District) -> bool {
        let p = self.preset();
        p.residential == d.residential && p.commercial == d.commercial && p.industry == d.industry && p.historic == d.historic
    }

    /// The story an old district's dials are, if any: the one whose
    /// architecture is exactly the district's (its height limit aside), or
    /// was before the story got its own look ([`Story::fitted_before`]).
    /// A district with no dials, or dials no story has, has none.
    pub fn matching(d: &District) -> Option<Story> {
        if d.residential.is_none() && d.commercial.is_none() && d.industry.is_none() && !d.historic {
            return None;
        }
        Story::ALL.into_iter().find(|s| s.fits(d)).or_else(|| Story::ALL.into_iter().find(|s| s.fitted_before(d)))
    }

    /// [garden] Whether `d`'s dials are the architecture this story had
    /// before its own look (Quiet Acres: Suburban homes, shops as they
    /// come), so a save from then still finds its story.
    pub fn fitted_before(self, d: &District) -> bool {
        use crate::districts::ResidentialSpec;
        match self {
            Story::QuietAcres => d.residential == Some(ResidentialSpec::Suburban) && d.commercial.is_none() && d.industry.is_none() && !d.historic,
            // [r4-harbour] The Promenade: Eco homes and the Tourist quarter.
            Story::Promenade => d.residential == Some(ResidentialSpec::Eco) && d.commercial == Some(crate::districts::CommercialSpec::Tourist) && d.industry.is_none() && !d.historic,
            // [r4-solarpunk] Eco Village: Eco homes, shops as they come,
            // farming industry.
            Story::EcoVillage => d.residential == Some(ResidentialSpec::Eco) && d.commercial.is_none() && d.industry == Some(IndustryKind::Farming) && !d.historic,
            // [r4-patchwork] Riverbank Shantytown: Kowloon homes, shops as
            // they come.
            Story::Shantytown => d.residential == Some(ResidentialSpec::Kowloon) && d.commercial.is_none() && d.industry.is_none() && !d.historic,
            _ => false,
        }
    }

    /// The story nearest a custom district's dials, to suggest: most dials
    /// in common, fewest at odds (`None` with no dials in common).
    pub fn closest(d: &District) -> Option<Story> {
        let score = |s: Story| {
            let p = s.preset();
            let mut n = 0i32;
            let mut cmp = |a: bool, b: bool, same: bool| {
                if a && b {
                    n += if same { 3 } else { -2 };
                } else if a || b {
                    n -= 1;
                }
            };
            cmp(p.residential.is_some(), d.residential.is_some(), p.residential == d.residential);
            cmp(p.commercial.is_some(), d.commercial.is_some(), p.commercial == d.commercial);
            cmp(p.industry.is_some(), d.industry.is_some(), p.industry == d.industry);
            cmp(p.historic, d.historic, true);
            n
        };
        let best = Story::ALL.into_iter().max_by_key(|s| (score(*s), -(s.index() as i32)))?;
        (score(best) > 0).then_some(best)
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zoning::LandUse;

    /// [stories] Every story sets something and no two set the same
    /// architecture (so an old district's dials match at most one), keys
    /// and labels parse back, and every preset's policies are in range.
    #[test]
    fn stories_are_distinct_and_parse() {
        for (i, a) in Story::ALL.iter().enumerate() {
            let p = a.preset();
            assert!(p.residential.is_some() || p.commercial.is_some() || p.industry.is_some() || p.historic, "{a:?} sets a building type");
            assert_eq!(p.policies, p.policies.clamped(), "{a:?} policies in range");
            assert!(!a.backstory().is_empty() && !a.sets().is_empty());
            assert_eq!(Story::parse(a.key()), Some(*a));
            assert_eq!(Story::parse(a.label()), Some(*a));
            for b in &Story::ALL[i + 1..] {
                let q = b.preset();
                assert!((p.residential, p.commercial, p.industry, p.historic) != (q.residential, q.commercial, q.industry, q.historic), "{a:?} and {b:?} build alike");
            }
        }
        assert_eq!(Story::parse("Founders Campus"), Some(Story::FoundersCampus));
        assert_eq!(Story::parse("walled quarter"), Some(Story::WalledQuarter));
    }

    /// [stories] Levies put builders off and support draws them, within bounds.
    #[test]
    fn policies_pull_growth_and_clamp() {
        let mut p = Policies::default();
        assert_eq!(p.pull(LandUse::Residential), 1.0);
        p.set(Policy::RentSupport, 0.9);
        assert_eq!(p.rent_support, MAX_RENT_SUPPORT, "clamped");
        assert!(p.pull(LandUse::Residential) > 1.4);
        p.set(Policy::BusinessRate, -1.0);
        assert_eq!(p.business_rate, MIN_BUSINESS_RATE);
        assert!(p.pull(LandUse::Commercial) > 1.0);
        p.set(Policy::BusinessRate, 0.1);
        assert!(p.pull(LandUse::Commercial) < 1.0);
        p.set(Policy::PollutionLevy, 0.2);
        assert_eq!(p.pull(LandUse::Industrial), 0.5);
        p.set(Policy::LateHours, 1.0);
        assert!(p.late_hours && p.pull(LandUse::Entertainment) > 1.0);
        assert!(p.summary().contains("late licence on"));
        assert_eq!(Policy::parse("bed_levy"), Some(Policy::BedLevy));
    }
}
