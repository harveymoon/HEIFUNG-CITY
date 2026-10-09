use super::*;

fn ids(n: usize) -> Vec<CitizenId> {
    let mut m: slotmap::SlotMap<CitizenId, ()> = slotmap::SlotMap::with_key();
    (0..n).map(|_| m.insert(())).collect()
}

#[test]
fn ties_and_notes_survive_a_save_compactly() {
    let people = ids(3);
    let mut b: slotmap::SlotMap<BuildingId, ()> = slotmap::SlotMap::with_key();
    let bar = b.insert(());
    let mut c = Circle::default();
    let t = c.tie_or_new(people[1], 5, Met::Venue, Some(bar));
    t.affinity = 0.4;
    t.partner = true;
    c.note(TalkNote { tick: 9, with: people[2], topic: Topic::Rent, outcome: Outcome::Laughed, place: Some(bar), setting: Setting::Venue });
    let text = ron::ser::to_string_pretty(&c, ron::ser::PrettyConfig::default()).unwrap();
    assert!(!text.contains("idx"), "ids are compact: {text}");
    let back: Circle = ron::from_str(&text).unwrap();
    assert_eq!(back, c);
}

#[test]
fn labels_follow_affinity_and_familiarity() {
    let who = ids(1)[0];
    let mut t = Tie::new(who, 0, Met::Venue, None);
    assert_eq!(t.bond(), Bond::Stranger);
    t.familiarity = 0.1;
    assert_eq!(t.bond(), Bond::Acquaintance);
    t.affinity = 0.35;
    t.familiarity = 0.3;
    assert_eq!(t.bond(), Bond::Friend);
    t.affinity = 0.7;
    t.familiarity = 0.6;
    assert_eq!(t.bond(), Bond::CloseFriend);
    t.partner = true;
    assert_eq!(t.bond(), Bond::Partner);
    t.partner = false;
    t.affinity = -0.3;
    assert_eq!(t.bond(), Bond::Rival);
    t.affinity = -0.6;
    assert_eq!(t.bond(), Bond::Enemy);
}

#[test]
fn the_weakest_tie_goes_past_the_cap_never_a_partner() {
    let people = ids(MAX_TIES + 3);
    let mut c = Circle::default();
    {
        let p = c.tie_or_new(people[0], 0, Met::Venue, None);
        p.partner = true;
    }
    for (k, &w) in people.iter().enumerate().skip(1) {
        let t = c.tie_or_new(w, k as u64, Met::Venue, None);
        t.familiarity = 0.1 + k as f32 * 0.01;
    }
    assert_eq!(c.ties.len(), MAX_TIES);
    assert!(c.tie(people[0]).is_some(), "the partner stays");
    assert!(c.tie(people[1]).is_none(), "the faintest went");
    assert!(c.tie(*people.last().unwrap()).is_some());
}

#[test]
fn like_minds_of_an_age_get_on_better_than_opposites() {
    // Two near-identical temperaments against two opposite ones.
    let seeds: Vec<u64> = (0..4000u64).collect();
    let t = |s: u64| crate::places::traits(s);
    let mut pairs_close = Vec::new();
    let mut pairs_far = Vec::new();
    for &a in seeds.iter().take(300) {
        for &b in seeds.iter().skip(300).take(300) {
            let (ta, tb) = (t(a), t(b));
            let gap: f32 = (0..5).map(|k| (ta[k] - tb[k]).abs()).sum();
            if gap < 0.6 {
                pairs_close.push(compatibility((a, 30, Tier::Mid), (b, 32, Tier::Mid)));
            } else if gap > 2.6 {
                pairs_far.push(compatibility((a, 25, Tier::Low), (b, 70, Tier::High)));
            }
        }
    }
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
    assert!(pairs_close.len() > 20 && pairs_far.len() > 20, "{} {}", pairs_close.len(), pairs_far.len());
    assert!(mean(&pairs_close) > 0.3, "like minds: {}", mean(&pairs_close));
    assert!(mean(&pairs_far) < -0.3, "opposites: {}", mean(&pairs_far));
    // Symmetric.
    assert_eq!(compatibility((5, 30, Tier::Mid), (9, 40, Tier::Low)), compatibility((9, 40, Tier::Low), (5, 30, Tier::Mid)));
}

#[test]
fn interests_follow_temperament_and_age() {
    let young_wild = (0..3000u64).find(|&s| {
        let t = crate::places::traits(s);
        t[2] > 0.8 && t[0] > 0.6
    });
    let s = young_wild.unwrap();
    let i = interests(s, 22, Tier::Low);
    let o = interests(s, 75, Tier::Low);
    assert!(i[GroupKind::Band.index()] > o[GroupKind::Band.index()]);
    assert_eq!(interests(s, 12, Tier::Low)[GroupKind::Startup.index()], 0.0, "no children's startups");
}
