//! Saving a random stream: RON cannot encode ChaCha's 128-bit counter, so
//! a stream is kept as its seed, stream id and word position, and picks up
//! after a load exactly where it stopped. `one` is the world's own stream
//! (`World::rng`); `opt` is for the lazily made per-system
//! streams (`Option<SimRng>`): saves from before they were kept load as
//! `None` and are seeded afresh on first use.

use crate::SimRng;
use rand::SeedableRng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
struct Saved {
    seed: [u8; 32],
    stream: u64,
    pos_hi: u64,
    pos_lo: u64,
}

impl Saved {
    fn of(rng: &SimRng) -> Self {
        let pos = rng.get_word_pos();
        Saved { seed: rng.get_seed(), stream: rng.get_stream(), pos_hi: (pos >> 64) as u64, pos_lo: pos as u64 }
    }

    fn rng(&self) -> SimRng {
        let mut rng = SimRng::from_seed(self.seed);
        rng.set_stream(self.stream);
        rng.set_word_pos(((self.pos_hi as u128) << 64) | self.pos_lo as u128);
        rng
    }
}

/// For a `SimRng` field (the world's own stream).
pub mod one {
    use super::*;

    pub fn serialize<S: Serializer>(rng: &SimRng, s: S) -> Result<S::Ok, S::Error> {
        Saved::of(rng).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SimRng, D::Error> {
        Ok(Saved::deserialize(d)?.rng())
    }
}

/// For `Option<SimRng>` fields (with `#[serde(default)]`).
pub mod opt {
    use super::*;

    pub fn serialize<S: Serializer>(rng: &Option<SimRng>, s: S) -> Result<S::Ok, S::Error> {
        rng.as_ref().map(Saved::of).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SimRng>, D::Error> {
        Ok(Option::<Saved>::deserialize(d)?.map(|s| s.rng()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

    #[derive(Serialize, Deserialize, Default)]
    #[serde(default)]
    struct Holder {
        #[serde(with = "opt")]
        rng: Option<SimRng>,
    }

    #[test]
    fn a_saved_stream_carries_on_where_it_stopped() {
        let mut h = Holder { rng: Some(crate::rng_for(7, 3)) };
        let _: u64 = h.rng.as_mut().unwrap().random();
        let text = ron::to_string(&h).unwrap();
        let mut back: Holder = ron::from_str(&text).unwrap();
        let (a, b): (u64, u64) = (h.rng.as_mut().unwrap().random(), back.rng.as_mut().unwrap().random());
        assert_eq!(a, b);
        let old: Holder = ron::from_str("()").unwrap();
        assert!(old.rng.is_none(), "a save without it seeds afresh");
        let none: Holder = ron::from_str(&ron::to_string(&Holder::default()).unwrap()).unwrap();
        assert!(none.rng.is_none());
    }
}
