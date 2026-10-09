//! Pure text formats shared by the HUD: numbers, money, plurals, game
//! times and durations. No egui, no world: each takes plain values, so each
//! is unit-tested below. Where two panels format the same thing in slightly
//! different words, both forms are kept (and named), so no panel's text
//! changes.

use std::fmt::Display;
use std::time::SystemTime;

/// The first letter upper-cased ("ironworks" → "Ironworks"); "" stays "".
pub fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// The plural "s" for a count: "" for exactly one, else "s".
pub fn s<N: PartialEq + From<u8>>(n: N) -> &'static str {
    if n == N::from(1) {
        ""
    } else {
        "s"
    }
}

/// A count with its noun: "1 car", "3 cars".
pub fn plural<N: PartialEq + From<u8> + Display>(n: N, one: &str, many: &str) -> String {
    let word = if n == N::from(1) { one } else { many };
    format!("{n} {word}")
}

/// A compact number: 1234 → "1.2k", 2.5e6 → "2.5M".
pub fn compact(x: f32) -> String {
    let a = x.abs();
    if a >= 1e6 {
        format!("{:.1}M", x / 1e6)
    } else if a >= 1e4 {
        format!("{:.0}k", x / 1e3)
    } else if a >= 1e3 {
        format!("{:.1}k", x / 1e3)
    } else if a >= 10.0 || a == 0.0 {
        format!("{x:.0}")
    } else {
        format!("{x:.1}")
    }
}

/// [`compact`] with a "+" before a gain: "+1.2k", "-40", "0".
pub fn signed(v: f32) -> String {
    if v > 0.0 {
        format!("+{}", compact(v))
    } else {
        compact(v)
    }
}

/// Money with a unit: "$950", "$12k", "$3.4M", "$1.2B" (the land and
/// estate lines). Not sign-aware: a negative sum prints as plain dollars.
pub fn money(v: f64) -> String {
    if v >= 1e9 {
        format!("${:.1}B", v / 1e9)
    } else if v >= 1e6 {
        format!("${:.1}M", v / 1e6)
    } else if v >= 1e3 {
        format!("${:.0}k", v / 1e3)
    } else {
        format!("${v:.0}")
    }
}

/// The 1-based day a tick falls on, as the top bar counts days.
pub fn day(tick: u64) -> u64 {
    tick / 1440 + 1
}

/// The time of day of a tick: "07:05".
pub fn hhmm(tick: u64) -> String {
    format!("{:02}:{:02}", (tick % 1440) / 60, tick % 60)
}

/// A tick as "day 3 14:05".
pub fn day_time(tick: u64) -> String {
    format!("day {} {}", day(tick), hhmm(tick))
}

/// Game minutes as "12 min" or "2 h 05".
pub fn hours_min(m: u64) -> String {
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {:02}", m / 60, m % 60)
    }
}

/// Game minutes as "12 min" or "2 h 05 min" (the clamp's wording).
pub fn hours_min_long(m: u64) -> String {
    if m >= 60 {
        format!("{} h {:02} min", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}

/// Traffic seconds as "45 s" or "3 min".
pub fn secs_or_min(secs: f32) -> String {
    if secs < 90.0 {
        format!("{secs:.0} s")
    } else {
        format!("{:.0} min", secs / 60.0)
    }
}

/// Minutes as "12 min", or "under a minute" below one.
pub fn minutes_or_under(m: f32) -> String {
    if m < 1.0 {
        "under a minute".into()
    } else {
        format!("{m:.0} min")
    }
}

/// Minutes as "12 min", or "-" for none.
pub fn minutes_or_dash(t: Option<u64>) -> String {
    t.map_or("-".to_string(), |m| format!("{m} min"))
}

/// Days in words: "under a day", "a day", "12 days", "over 999 days".
pub fn days_text(d: f32) -> String {
    if d < 1.0 {
        "under a day".into()
    } else if d < 1.5 {
        "a day".into()
    } else if d > 999.0 {
        "over 999 days".into()
    } else {
        format!("{d:.0} days")
    }
}

/// At most `n` characters: a longer text is cut to `n - 1` and ends in "…".
pub fn elide(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>())
    }
}

/// How long before `now` a file was written, in words: "just now",
/// "5 min ago", "3 h ago", "2 d ago"; "" when unknown or in the future.
pub fn age_since(modified: Option<SystemTime>, now: SystemTime) -> String {
    let Some(secs) = modified.and_then(|m| now.duration_since(m).ok()).map(|d| d.as_secs()) else { return String::new() };
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn capitalise_handles_empty_and_multibyte() {
        assert_eq!(capitalise(""), "");
        assert_eq!(capitalise("ironworks"), "Ironworks");
        assert_eq!(capitalise("Already"), "Already");
        assert_eq!(capitalise("éclair"), "Éclair");
        // A letter whose capital is two letters.
        assert_eq!(capitalise("ßtraße"), "SStraße");
        assert_eq!(capitalise("ǆ"), "Ǆ");
        assert_eq!(capitalise("1st"), "1st");
        // No CJK here: `theme`'s font test wants a glyph for every literal.
        assert_eq!(capitalise("ñandú"), "Ñandú");
    }

    #[test]
    fn plurals() {
        assert_eq!(s(1u32), "");
        assert_eq!(s(0usize), "s");
        assert_eq!(s(2u64), "s");
        assert_eq!(s(-1i32), "s");
        assert_eq!(plural(1u32, "lane", "lanes"), "1 lane");
        assert_eq!(plural(0u32, "lane", "lanes"), "0 lanes");
        assert_eq!(plural(12usize, "bus", "buses"), "12 buses");
    }

    #[test]
    fn compact_at_band_edges() {
        assert_eq!(compact(0.0), "0");
        assert_eq!(compact(0.25), "0.2");
        assert_eq!(compact(9.94), "9.9");
        assert_eq!(compact(10.0), "10");
        assert_eq!(compact(999.0), "999");
        assert_eq!(compact(1000.0), "1.0k");
        assert_eq!(compact(1234.0), "1.2k");
        assert_eq!(compact(9999.0), "10.0k");
        assert_eq!(compact(10_000.0), "10k");
        assert_eq!(compact(999_000.0), "999k");
        assert_eq!(compact(1e6), "1.0M");
        assert_eq!(compact(2.5e6), "2.5M");
        assert_eq!(compact(-1234.0), "-1.2k");
        assert_eq!(compact(-5.0), "-5.0");
    }

    #[test]
    fn signed_marks_gains() {
        assert_eq!(signed(1234.0), "+1.2k");
        assert_eq!(signed(-40.0), "-40");
        assert_eq!(signed(0.0), "0");
    }

    #[test]
    fn money_units() {
        assert_eq!(money(0.0), "$0");
        assert_eq!(money(950.0), "$950");
        assert_eq!(money(12_300.0), "$12k");
        assert_eq!(money(3.4e6), "$3.4M");
        assert_eq!(money(1.25e9), "$1.2B");
        assert_eq!(money(-5e6), "$-5000000");
    }

    #[test]
    fn days_and_times() {
        assert_eq!(day(0), 1);
        assert_eq!(day(1439), 1);
        assert_eq!(day(1440), 2);
        assert_eq!(hhmm(0), "00:00");
        assert_eq!(hhmm(1440 + 7 * 60 + 5), "07:05");
        assert_eq!(day_time(2 * 1440 + 14 * 60 + 5), "day 3 14:05");
    }

    #[test]
    fn durations_keep_their_wordings() {
        assert_eq!(hours_min(0), "0 min");
        assert_eq!(hours_min(59), "59 min");
        assert_eq!(hours_min(60), "1 h 00");
        assert_eq!(hours_min(125), "2 h 05");
        assert_eq!(hours_min_long(59), "59 min");
        assert_eq!(hours_min_long(125), "2 h 05 min");
        assert_eq!(secs_or_min(45.0), "45 s");
        assert_eq!(secs_or_min(89.4), "89 s");
        assert_eq!(secs_or_min(90.0), "2 min");
        assert_eq!(secs_or_min(180.0), "3 min");
        assert_eq!(minutes_or_under(0.5), "under a minute");
        assert_eq!(minutes_or_under(12.4), "12 min");
        assert_eq!(minutes_or_dash(None), "-");
        assert_eq!(minutes_or_dash(Some(7)), "7 min");
        assert_eq!(days_text(0.4), "under a day");
        assert_eq!(days_text(1.2), "a day");
        assert_eq!(days_text(12.0), "12 days");
        assert_eq!(days_text(1000.0), "over 999 days");
    }

    #[test]
    fn elide_cuts_on_characters() {
        assert_eq!(elide("", 0), "");
        assert_eq!(elide("", 5), "");
        assert_eq!(elide("abc", 0), "…");
        assert_eq!(elide("abc", 3), "abc");
        assert_eq!(elide("abcdef", 4), "abc…");
        assert_eq!(elide("Ünïcödé façade", 5), "Ünïc…");
        assert_eq!(elide("üüüü", 2), "ü…");
    }

    #[test]
    fn age_in_words() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let ago = |s: u64| Some(now - Duration::from_secs(s));
        assert_eq!(age_since(None, now), "");
        assert_eq!(age_since(Some(now + Duration::from_secs(5)), now), "");
        assert_eq!(age_since(ago(0), now), "just now");
        assert_eq!(age_since(ago(59), now), "just now");
        assert_eq!(age_since(ago(60), now), "1 min ago");
        assert_eq!(age_since(ago(3599), now), "59 min ago");
        assert_eq!(age_since(ago(3600), now), "1 h ago");
        assert_eq!(age_since(ago(86_400 * 3), now), "3 d ago");
    }
}
