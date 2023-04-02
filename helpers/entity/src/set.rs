use core::hash::{BuildHasher, Hash};
use core::marker::PhantomData;
use core::ops::Index;

use std::collections::hash_map::RandomState;
use std::fmt;

use serde::de::{Deserialize, Deserializer, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeSeq, Serializer};

use indexmap::set::IndexSet;
use indexmap::Equivalent;

use crate::id::EntityIds;
use crate::{EntityId, EntityVec};

#[derive(Clone)]
pub struct EntitySet<I, V: Hash, RS: BuildHasher = RandomState> {
    set: IndexSet<V, RS>,
    ids: PhantomData<I>,
}

impl<I, V, RS> EntitySet<I, V, RS>
where
    I: EntityId,
    V: Hash + Eq,
    RS: BuildHasher,
{
    pub fn with_hasher(h: RS) -> Self {
        Self {
            set: IndexSet::with_hasher(h),
            ids: PhantomData,
        }
    }

    pub fn with_capacity_and_hasher(n: usize, h: RS) -> Self {
        Self {
            set: IndexSet::with_capacity_and_hasher(n, h),
            ids: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn clear(&mut self) {
        self.set.clear()
    }

    pub fn insert(&mut self, v: V) -> (I, bool) {
        let (i, f) = self.set.insert_full(v);
        (I::from_idx(i), f)
    }

    pub fn insert_new(&mut self, v: V) -> I {
        let (i, f) = self.insert(v);
        assert!(f);
        i
    }

    pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<I>
    where
        Q: Hash + Equivalent<V>,
    {
        let (i, _) = self.set.get_full(key)?;
        Some(I::from_idx(i))
    }

    pub fn contains<Q: ?Sized>(&self, key: &Q) -> bool
    where
        Q: Hash + Equivalent<V>,
    {
        self.set.contains(key)
    }

    pub fn ids(&self) -> EntityIds<I> {
        EntityIds::new(self.len())
    }

    pub fn iter(&self) -> Iter<'_, I, V> {
        Iter {
            vals: self.set.iter(),
            pos: 0,
            ids: PhantomData,
        }
    }

    pub fn values(&self) -> indexmap::set::Iter<'_, V> {
        self.set.iter()
    }

    pub fn into_values(self) -> indexmap::set::IntoIter<V> {
        self.set.into_iter()
    }

    pub fn into_vec(self) -> EntityVec<I, V> {
        self.into_values().collect()
    }

    pub fn get_or_insert(
        &mut self,
        key: &(impl ToOwned<Owned = V> + Hash + Equivalent<V> + ?Sized),
    ) -> I {
        match self.get(key) {
            Some(i) => i,
            None => self.insert(key.to_owned()).0,
        }
    }
}

impl<I, V> EntitySet<I, V>
where
    I: EntityId,
    V: Hash,
{
    pub fn new() -> Self {
        Self {
            set: IndexSet::new(),
            ids: PhantomData,
        }
    }

    pub fn with_capacity(n: usize) -> Self {
        Self {
            set: IndexSet::with_capacity(n),
            ids: PhantomData,
        }
    }
}

impl<I, V> Default for EntitySet<I, V>
where
    I: EntityId,
    V: Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<I, V, RS> fmt::Debug for EntitySet<I, V, RS>
where
    I: EntityId,
    V: Hash + Eq + fmt::Debug,
    RS: BuildHasher,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_map().entries(self).finish()
    }
}

impl<I, V, RS, RS2> PartialEq<EntitySet<I, V, RS2>> for EntitySet<I, V, RS>
where
    I: EntityId,
    V: Hash + Eq,
    RS: BuildHasher,
    RS2: BuildHasher,
{
    fn eq(&self, other: &EntitySet<I, V, RS2>) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<I, V, RS> Eq for EntitySet<I, V, RS>
where
    I: EntityId,
    V: Hash + Eq,
    RS: BuildHasher,
{
}

impl<I, V, RS> IntoIterator for EntitySet<I, V, RS>
where
    I: EntityId,
    V: Hash + Eq,
    RS: BuildHasher,
{
    type Item = (I, V);
    type IntoIter = IntoIter<I, V>;
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            vals: self.set.into_iter(),
            pos: 0,
            ids: PhantomData,
        }
    }
}

impl<'a, I, V, RS> IntoIterator for &'a EntitySet<I, V, RS>
where
    I: EntityId,
    V: Hash + Eq,
    RS: BuildHasher,
{
    type Item = (I, &'a V);
    type IntoIter = Iter<'a, I, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<I, V, RS> Index<I> for EntitySet<I, V, RS>
where
    I: EntityId,
    V: Hash + Eq,
    RS: BuildHasher,
{
    type Output = V;
    fn index(&self, index: I) -> &V {
        &self.set[index.to_idx()]
    }
}

#[derive(Clone, Debug)]
pub struct Iter<'a, I, V: Hash> {
    vals: indexmap::set::Iter<'a, V>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<'a, I, V> Iterator for Iter<'a, I, V>
where
    I: EntityId,
    V: Hash,
{
    type Item = (I, &'a V);
    fn next(&mut self) -> Option<(I, &'a V)> {
        let val = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, val))
    }
}

impl<'a, I, V> DoubleEndedIterator for Iter<'a, I, V>
where
    I: EntityId,
    V: Hash,
{
    fn next_back(&mut self) -> Option<(I, &'a V)> {
        let val = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), val))
    }
}

impl<'a, I, V> ExactSizeIterator for Iter<'a, I, V>
where
    I: EntityId,
    V: Hash,
{
    fn len(&self) -> usize {
        self.vals.len()
    }
}

#[derive(Debug)]
pub struct IntoIter<I, V: Hash> {
    vals: indexmap::set::IntoIter<V>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<I, V> Iterator for IntoIter<I, V>
where
    I: EntityId,
    V: Hash,
{
    type Item = (I, V);
    fn next(&mut self) -> Option<(I, V)> {
        let val = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, val))
    }
}

impl<I, V> DoubleEndedIterator for IntoIter<I, V>
where
    I: EntityId,
    V: Hash,
{
    fn next_back(&mut self) -> Option<(I, V)> {
        let val = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), val))
    }
}

impl<I, V> ExactSizeIterator for IntoIter<I, V>
where
    I: EntityId,
    V: Hash,
{
    fn len(&self) -> usize {
        self.vals.len()
    }
}

impl<I, V, RS> FromIterator<V> for EntitySet<I, V, RS>
where
    I: EntityId,
    V: Hash + Eq,
    RS: BuildHasher + Default,
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = V>,
    {
        Self {
            set: IndexSet::from_iter(iter),
            ids: PhantomData,
        }
    }
}

impl<I, V, RS> Serialize for EntitySet<I, V, RS>
where
    I: EntityId,
    V: Serialize + Hash + Eq,
    RS: BuildHasher,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for v in self.values() {
            seq.serialize_element(v)?;
        }
        seq.end()
    }
}

#[allow(clippy::type_complexity)]
struct DeserializeVisitor<I, V: Hash, RS: BuildHasher> {
    marker: PhantomData<fn() -> EntitySet<I, V, RS>>,
}

impl<I, V, RS> DeserializeVisitor<I, V, RS>
where
    V: Hash,
    RS: BuildHasher,
{
    fn new() -> Self {
        DeserializeVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, I, V, RS> Visitor<'de> for DeserializeVisitor<I, V, RS>
where
    I: EntityId,
    V: Deserialize<'de> + Hash + Eq,
    RS: Default + BuildHasher,
{
    type Value = EntitySet<I, V, RS>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("entity set")
    }

    fn visit_seq<S>(self, mut access: S) -> Result<Self::Value, S::Error>
    where
        S: SeqAccess<'de>,
    {
        let mut res = EntitySet::with_capacity_and_hasher(
            access.size_hint().unwrap_or(0),
            Default::default(),
        );

        while let Some(value) = access.next_element()? {
            res.insert(value);
        }

        Ok(res)
    }
}

impl<'de, I, V, RS> Deserialize<'de> for EntitySet<I, V, RS>
where
    I: EntityId,
    V: Deserialize<'de> + Hash + Eq,
    RS: Default + BuildHasher,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(DeserializeVisitor::new())
    }
}
