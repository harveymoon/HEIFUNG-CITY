//! [talk] Prompt building (their state in, secrets out), the caps, the
//! offline stand-in, the daily budget and the memory. No network.

use std::time::Duration;

use super::talk::*;
use super::tests::grown;
use super::{system_prompt, user_prompt, CommitteeBackend, CommitteeSettings, Courier, Task, TurnReply, TurnRequest};
use nd_sim::{CitizenId, World};

const SECRET: &str = "sk-ant-api03-TESTSECRET-do-not-leak-0123456789";

fn worker(w: &World) -> CitizenId {
    w.citizens.iter().find(|(_, c)| c.home.is_some() && c.work.is_some()).map(|(id, _)| id).expect("a working resident")
}

/// A model that answers every line with the same words, on the next poll.
#[derive(Default)]
struct Parrot {
    ready: Vec<u64>,
}

impl CommitteeBackend for Parrot {
    fn submit(&mut self, id: u64, _req: TurnRequest) {
        self.ready.push(id);
    }
    fn poll(&mut self) -> Vec<(u64, Result<TurnReply, String>)> {
        self.ready.drain(..).map(|id| (id, Ok(TurnReply { text: "Kira: \"The buses never come.\"".into(), json: None }))).collect()
    }
    fn name(&self) -> String {
        "parrot".into()
    }
}

#[test]
fn the_prompt_holds_their_real_state() {
    let mut w = grown().clone();
    let id = worker(&w);
    let tick = w.clock.tick;
    w.citizens[id].remember(tick, "Saw a dog chase a drone at the plaza.");
    let req = talk_request(&w, id, "Why don't you take the bus?", &[], 300).unwrap();
    let (sys, user) = (system_prompt(&req), user_prompt(&req));
    let c = &w.citizens[id];
    assert!(sys.contains(&c.name), "{sys}");
    assert!(sys.contains(&format!("Age {}", c.age)), "{sys}");
    let home = &w.buildings[c.home.unwrap()].name;
    let work = &w.buildings[c.work.unwrap()].name;
    assert!(sys.contains(home.as_str()) && sys.contains(work.as_str()), "{sys}");
    assert!(sys.contains("Saw a dog chase a drone at the plaza."), "memories: {sys}");
    assert!(sys.contains(&format!("Mood {:.0}%", c.mood * 100.0)), "{sys}");
    assert!(sys.contains("Home to work is") && sys.contains("a car"), "the commute: {sys}");
    assert!(sys.contains("Favourite places:") && sys.contains("never to go back"), "{sys}");
    assert!(sys.contains("fictional") && sys.contains("never mention real people"), "grounded: {sys}");
    assert!(sys.contains("You know only what is written below"), "{sys}");
    assert!(user.contains("Why don't you take the bus?"), "{user}");
    assert_eq!(req.task.key(), "talk");
    assert!(!req.task.wants_json());
}

#[test]
fn the_prompt_never_holds_the_key() {
    let mut w = grown().clone();
    let id = worker(&w);
    let settings = CommitteeSettings { claude_api_key: Some(SECRET.into()), ..Default::default() };
    let mut courier = Courier::mock();
    let mut t = Talker::default();
    t.send(&mut w, id, "How are you?", &mut courier, &settings, false).unwrap();
    let req = talk_request(&w, id, "And the rent?", w.talks.lines(id), settings.talk_max_tokens).unwrap();
    for text in [system_prompt(&req), user_prompt(&req), serde_json::to_string(&req).unwrap(), format!("{req:?}"), format!("{settings:?}"), ron::to_string(&w.talks).unwrap()] {
        assert!(!text.contains(SECRET) && !text.contains("TESTSECRET"), "the key leaked: {text}");
    }
    // The mayor cannot paste it into a chat either.
    let before = w.talks.lines(id).len();
    assert!(t.send(&mut w, id, &format!("my key is {SECRET}"), &mut courier, &settings, false).is_err());
    assert!(t.send(&mut w, id, "try sk-ant-something", &mut courier, &settings, false).is_err());
    let err = t.send(&mut w, id, &format!("x {SECRET}"), &mut courier, &settings, false).unwrap_err();
    assert!(!err.contains("TESTSECRET"), "the error does not echo it: {err}");
    assert_eq!(w.talks.lines(id).len(), before, "nothing was sent or kept");
}

#[test]
fn offline_the_stand_in_answers_at_once_from_the_card() {
    let mut w = grown().clone();
    let id = worker(&w);
    let mut courier = Courier::mock();
    let mut t = Talker::default();
    let settings = CommitteeSettings::default();
    assert_eq!(t.send(&mut w, id, "Why don't you take the bus?", &mut courier, &settings, false), Ok(Sent::StandIn));
    assert!(!t.typing(id), "no wait offline");
    assert_eq!(courier.calls, 0, "no request at all");
    let l = w.talks.lines(id);
    assert_eq!(l.len(), 2);
    assert!(l[0].mayor && !l[1].mayor && l[1].stand_in, "{l:?}");
    assert!(!l[1].text.is_empty());
    assert!(t.send(&mut w, id, "   ", &mut courier, &settings, false).is_err(), "empty lines are refused");
    assert!(t.send(&mut w, id, &"a".repeat(MAX_SAID_CHARS + 1), &mut courier, &settings, false).is_err());
    // The same words from the same state: the same answer.
    let card = TalkCard::of(&w, id).unwrap();
    assert_eq!(mock_chat(&card, "hi", 7, true), mock_chat(&card, "hi", 7, true));
}

#[test]
fn the_stand_in_gives_the_real_reason_for_no_bus() {
    let card = TalkCard { name: "Kira Vale".into(), archetype: "Survivor".into(), work: Some("Block 9".into()), commute_best: Some("walk".into()), no_bus: Some("no_stop_at_start".into()), ..Default::default() };
    let r = mock_chat(&card, "Why didn't you take a bus, metro or ride a bike?", 3, false);
    assert!(r.contains("no bus stop within a walk of home"), "{r}");
    assert!(r.contains("don't have a bike"), "{r}");
    assert_eq!(Topic::of("Hi, how are you doing?"), Topic::Feeling);
    assert_eq!(Topic::of("Is the rent OK?"), Topic::Money);
    assert_eq!(Topic::of("Where do you like to go after work?"), Topic::Places);
}

#[test]
fn a_model_answers_on_a_later_poll_within_the_daily_budget() {
    let mut w = grown().clone();
    let id = worker(&w);
    let other = w.citizens.keys().find(|k| *k != id).unwrap();
    let mut courier = Courier::new(Box::new(Parrot::default()), Duration::from_secs(60));
    let settings = CommitteeSettings { talk_daily_budget: 1, ..Default::default() };
    let mut t = Talker::default();
    assert_eq!(t.send(&mut w, id, "Hello there", &mut courier, &settings, false), Ok(Sent::Model));
    assert!(t.typing(id), "typing until the reply comes");
    assert!(t.send(&mut w, id, "Again?", &mut courier, &settings, false).is_err(), "one line at a time");
    for d in courier.poll() {
        assert!(t.take(&mut w, &d));
    }
    assert!(!t.typing(id));
    let l = w.talks.lines(id);
    assert_eq!(l.last().unwrap().text, "The buses never come.", "the name and quotes are cleaned off");
    assert!(!l.last().unwrap().stand_in);
    assert_eq!(t.used_today(), 1);
    // The budget is spent: the stand-in answers, no call.
    let calls = courier.calls;
    assert_eq!(t.send(&mut w, other, "Hello", &mut courier, &settings, false), Ok(Sent::OverBudget));
    assert_eq!(courier.calls, calls);
    assert_eq!(w.talks.lines(other).len(), 2);
}

#[test]
fn a_long_chat_keeps_its_caps() {
    let mut w = grown().clone();
    let id = worker(&w);
    let mut courier = Courier::mock();
    let mut t = Talker::default();
    let settings = CommitteeSettings::default();
    for i in 0..30 {
        t.send(&mut w, id, &format!("question {i}"), &mut courier, &settings, false).unwrap();
    }
    assert_eq!(w.talks.lines(id).len(), nd_sim::talk::LINES_PER_PERSON);
    let req = talk_request(&w, id, "last", w.talks.lines(id), 300).unwrap();
    let Task::Chat { history, .. } = &req.task else { panic!() };
    assert_eq!(history.len(), HISTORY_IN_PROMPT);
    assert_eq!(history.last().unwrap().text, w.talks.lines(id).last().unwrap().text);
}

#[test]
fn a_chat_leaves_one_memory_a_day_only_when_asked() {
    let mut w = grown().clone();
    let id = worker(&w);
    let mut courier = Courier::mock();
    let mut t = Talker::default();
    let settings = CommitteeSettings::default();
    let mems = |w: &World| w.citizens[id].memories.iter().filter(|m| m.text.starts_with(MEMORY_PREFIX)).count();
    let before = mems(&w);
    t.send(&mut w, id, "How's the commute?", &mut courier, &settings, false).unwrap();
    assert_eq!(mems(&w), before, "no memory unless asked");
    t.send(&mut w, id, "How's the commute?", &mut courier, &settings, true).unwrap();
    assert_eq!(mems(&w), before + 1);
    assert!(w.citizens[id].memories.last().unwrap().text.contains("getting around"));
    t.send(&mut w, id, "And the rent?", &mut courier, &settings, true).unwrap();
    assert_eq!(mems(&w), before + 1, "one a day");
}

#[test]
fn a_chat_saves_and_loads_with_the_city() {
    let mut w = grown().clone();
    let id = worker(&w);
    let mut t = Talker::default();
    t.send(&mut w, id, "Hello", &mut Courier::mock(), &CommitteeSettings::default(), false).unwrap();
    let text = ron::to_string(&w.talks).unwrap();
    let back: nd_sim::talk::TalkLog = ron::from_str(&text).unwrap();
    assert_eq!(back.lines(id), w.talks.lines(id));
}

/// [r7-citizen-story] Harvey towed Yuki's car and they "seemed
/// unbothered": the chat's card now says the car is in the pound and what
/// it costs, among what is wrong right now, and the tow stays in view even
/// when the last memories are all benches; the stand-in brings it up.
#[test]
fn a_fresh_tow_is_on_their_mind_when_the_mayor_talks_to_them() {
    use nd_sim::vehicle::{CarPlace, ParkKind, ParkSpot};
    let mut w = grown().clone();
    let id = w.citizens.iter().find(|(_, c)| c.car.is_some() && c.home.is_some()).map(|(id, _)| id).expect("a car owner");
    let vid = w.citizens[id].car.unwrap();
    let pound = w.buildings.keys().next().unwrap();
    let tick = w.clock.tick;
    w.vehicles.cars[vid].place = CarPlace::Parked(ParkSpot { kind: ParkKind::Pound, building: Some(pound), since: tick - 120, fee: 80.0, ..Default::default() });
    w.citizens[id].remember(tick - 120, "The car's in the pound at Hooked on Towing. 95 to get it out now, 15 more each day it stays.");
    for k in 0..12 {
        w.citizens[id].remember(tick - 100 + k, "Sat on a bench in Umbra Plaza.");
    }
    let req = talk_request(&w, id, "How are you doing?", &[], 300).unwrap();
    let sys = system_prompt(&req);
    assert!(sys.contains("What is wrong for you right now") && sys.contains("Your car was towed"), "{sys}");
    assert!(sys.contains("to get it out now"), "the cost: {sys}");
    assert!(sys.contains("Things that mattered lately") && sys.contains("The car's in the pound at Hooked"), "the tow stays in view: {sys}");
    assert!(sys.contains("is on your mind"), "{sys}");
    let Task::Chat { who, said, .. } = &req.task else { panic!("a chat") };
    let reply = mock_chat(who, said, 1, true);
    assert!(reply.contains("My car was towed"), "the stand-in reacts: {reply}");
}
