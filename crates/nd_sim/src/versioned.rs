//! [refactor/app] A value with a change counter, for the renderer.
//!
//! `Versioned<T>` derefs to `T`; every mutable access (`DerefMut`, so
//! `get_mut`, `values_mut`, `insert`, indexing for writing, ...) bumps
//! `version`. The count is conservative: a mutable borrow that changes
//! nothing still bumps it. What it promises is the other way round: while
//! `version()` stays put, the value has not changed, so a reader that
//! hashed or built something from it can skip the work.
//!
//! It is not saved: a save holds the bare value (serde transparent). Every
//! new or loaded value starts its count at a fresh base (2^32 apart), so a
//! reader keyed on an old value's version does not mistake a replaced one
//! for it.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

/// The next value's starting version.
static NEXT_BASE: AtomicU64 = AtomicU64::new(1 << 32);

#[derive(Debug)]
pub struct Versioned<T> {
    value: T,
    version: u64,
}

impl<T> Versioned<T> {
    pub fn new(value: T) -> Self {
        Self { value, version: NEXT_BASE.fetch_add(1 << 32, Ordering::Relaxed) }
    }

    /// Bumped on every mutable access since the value was made or loaded.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Replace the whole value (counts as a change).
    pub fn set(&mut self, value: T) {
        **self = value;
    }
}

/// A clone starts from a fresh base too: it may go its own way.
impl<T: Clone> Clone for Versioned<T> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone())
    }
}

impl<T: Default> Default for Versioned<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> From<T> for Versioned<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T> Deref for Versioned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for Versioned<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.version = self.version.wrapping_add(1);
        &mut self.value
    }
}

impl<T: PartialEq> PartialEq for Versioned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Serialize> Serialize for Versioned<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(s)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Versioned<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        T::deserialize(d).map(Self::new)
    }
}

impl<'a, T> IntoIterator for &'a Versioned<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (&self.value).into_iter()
    }
}

impl<'a, T> IntoIterator for &'a mut Versioned<T>
where
    &'a mut T: IntoIterator,
{
    type Item = <&'a mut T as IntoIterator>::Item;
    type IntoIter = <&'a mut T as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (&mut **self).into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_keep_the_version_and_writes_bump_it() {
        let mut v: Versioned<Vec<u32>> = Versioned::new(vec![1, 2]);
        let v0 = v.version();
        let _ = v.len() + v.iter().count();
        for _ in &v {}
        assert_eq!(v.version(), v0);
        v.push(3);
        assert_eq!(v.version(), v0 + 1);
        for x in &mut v {
            *x += 1;
        }
        assert_eq!(v.version(), v0 + 2);
        v.set(vec![]);
        assert_eq!(v.version(), v0 + 3);
        assert_ne!(Versioned::new(vec![1u32]).version(), v0, "a new value starts from a fresh base");
    }

    #[test]
    fn saves_hold_the_bare_value() {
        let v = Versioned::new(vec![1u8, 2]);
        let s = ron::to_string(&v).unwrap();
        assert_eq!(s, ron::to_string(&vec![1u8, 2]).unwrap());
        let back: Versioned<Vec<u8>> = ron::from_str(&s).unwrap();
        assert_eq!(*back, vec![1, 2]);
        assert_ne!(back.version(), v.version(), "a loaded value starts from a fresh base");
    }
}
