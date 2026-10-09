//! Service statistics: calls, responses and why calls went unanswered.

use super::*;

/// Figures for the panel, the report and the alerts.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ServiceStats {
    /// Share of homes covered, per covering kind (police, fire, health, school).
    #[serde(deserialize_with = "crate::economy::padded")]
    pub coverage: [f32; 4],
    #[serde(deserialize_with = "crate::economy::padded")]
    pub incidents_today: [u32; IncidentKind::SLOTS],
    #[serde(deserialize_with = "crate::economy::padded")]
    pub resolved_today: [u32; IncidentKind::SLOTS],
    pub unanswered_today: u32,
    pub arrests_today: u32,
    pub response_ticks_today: u64,
    pub responses_today: u32,
    /// Average minutes from call to arrival, yesterday.
    pub avg_response_yesterday: f32,
    pub units: u32,
    pub units_busy: u32,
    pub fires_burning: u32,
    pub burned_down_total: u32,
    pub in_hospital: u32,
    pub waiting_for_ambulance: u32,
    pub upkeep_today: f32,
    pub tickets_today: f32,
    pub stadium_crowd_today: u32,
    pub dispatchers_on_shift: u32,
    pub jailed: u32,
    pub held_at_stations: u32,
    pub released_early_total: u32,
    pub out_of_town_calls_today: u32,
    pub out_of_town_fees_today: f32,
    /// Children who went to school today.
    pub pupils_today: u32,
    /// Hospital bills paid by households today, and what the city covered.
    pub bills_paid_today: f32,
    pub bills_covered_today: f32,
    /// Workers' wage bonus from schooling, summed over the city, today.
    pub education_bonus_today: f32,
    // ── response, per incident kind (fire, crime, medical) ──
    /// Minutes from call to arrival of each call answered today, and yesterday's.
    #[serde(deserialize_with = "crate::economy::padded")]
    pub response_min_today: [Vec<u16>; IncidentKind::SLOTS],
    #[serde(deserialize_with = "crate::economy::padded")]
    pub response_min_yesterday: [Vec<u16>; IncidentKind::SLOTS],
    /// Calls past their dispatch time waiting for a unit, now; the most today.
    #[serde(deserialize_with = "crate::economy::padded")]
    pub queued: [u32; IncidentKind::SLOTS],
    #[serde(deserialize_with = "crate::economy::padded")]
    pub queue_max_today: [u32; IncidentKind::SLOTS],
    /// Minutes today every unit of the kind was busy, and since when the
    /// current spell has lasted (None when a unit is free).
    #[serde(deserialize_with = "crate::economy::padded")]
    pub saturated_min_today: [u32; IncidentKind::SLOTS],
    #[serde(deserialize_with = "crate::economy::padded")]
    pub saturated_since: [Option<u64>; IncidentKind::SLOTS],
    /// Yesterday's saturated minutes, for the alert after midnight.
    #[serde(deserialize_with = "crate::economy::padded")]
    pub saturated_min_yesterday: [u32; IncidentKind::SLOTS],
    /// Calls that gave up unanswered today, per kind, by why nobody came.
    #[serde(deserialize_with = "crate::economy::padded")]
    pub unanswered_why_today: [UnansweredWhy; IncidentKind::SLOTS],
    /// Mutual aid from the next town while every unit was busy, today.
    pub mutual_aid_today: u32,
    /// Units pulled off a lower-priority call for a more urgent one, today.
    pub diverted_today: u32,
    /// Units on patrol or standby, now.
    pub on_patrol: u32,
    /// Calls no free unit of ours had a road to (handed to the next town), today.
    pub unreachable_today: u32,
    /// [emergency] Car chases started today, ended by a tag (caught) and
    /// lost, and ever.
    pub chases_today: u32,
    pub chases_tagged_today: u32,
    pub chases_lost_today: u32,
    pub chases_total: u32,
    pub chases_tagged_total: u32,
    /// [fire-scene] Second alarms answered by a second engine.
    pub second_alarms_today: u32,
    pub second_alarms_total: u32,
}

/// Why calls went unanswered.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UnansweredWhy {
    /// Every unit of the kind was out on other calls the whole wait.
    pub all_units_busy: u32,
    /// A unit was free but no road led there.
    pub no_route: u32,
    /// No crewed station of the kind and no dispatcher to call for help.
    pub no_station: u32,
}

/// Median and 90th percentile of a list of minutes, and the share at or
/// under `within`.
pub fn response_summary(v: &[u16], within: u16) -> Option<(u16, u16, f32)> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_unstable();
    let at = |q: f32| s[((s.len() - 1) as f32 * q).round() as usize];
    Some((at(0.5), at(0.9), s.iter().filter(|m| **m <= within).count() as f32 / s.len() as f32))
}

impl ServiceStats {
    /// Average minutes from call to arrival today (yesterday's if none yet).
    pub fn avg_response(&self) -> f32 {
        if self.responses_today > 0 {
            self.response_ticks_today as f32 / self.responses_today as f32
        } else {
            self.avg_response_yesterday
        }
    }
}
