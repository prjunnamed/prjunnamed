use core::hash::{BuildHasher, Hash};
use core::marker::PhantomData;
use core::ops::{Index, IndexMut};

use std::collections::hash_map::RandomState;
use std::fmt;

use serde::de::{Deserialize, Deserializer, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeSeq, Serializer};

use indexmap::map::IndexMap;
use indexmap::Equivalent;

use crate::id::EntityIds;
use crate::{EntityId, EntityVec};

#[derive(Clone)]
pub struct EntityMap<I, K: Hash, V, RS: BuildHasher = RandomState> {
    map: IndexMap<K, V, RS>,
    ids: PhantomData<I>,
}

impl<I, K, V, RS> EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq,
    RS: BuildHasher,
{
    pub fn with_hasher(h: RS) -> Self {
        Self {
            map: IndexMap::with_hasher(h),
            ids: PhantomData,
        }
    }

    pub fn with_capacity_and_hasher(n: usize, h: RS) -> Self {
        Self {
            map: IndexMap::with_capacity_and_hasher(n, h),
            ids: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn clear(&mut self) {
        self.map.clear()
    }

    pub fn insert(&mut self, k: K, v: V) -> (I, Option<V>) {
        let (i, v) = self.map.insert_full(k, v);
        (I::from_idx(i), v)
    }

    pub fn insert_new(&mut self, k: K, v: V) -> I {
        let (i, f) = self.insert(k, v);
        assert!(f.is_none());
        i
    }

    pub fn key(&self, id: I) -> &K {
        self.map.get_index(id.to_idx()).unwrap().0
    }

    pub fn get<Q: ?Sized>(&self, key: &Q) -> Option<(I, &V)>
    where
        Q: Hash + Equivalent<K>,
    {
        let (i, _, v) = self.map.get_full(key)?;
        Some((I::from_idx(i), v))
    }

    pub fn get_mut<Q: ?Sized>(&mut self, key: &Q) -> Option<(I, &mut V)>
    where
        Q: Hash + Equivalent<K>,
    {
        let (i, _, v) = self.map.get_full_mut(key)?;
        Some((I::from_idx(i), v))
    }

    pub fn get_full<Q: ?Sized>(&self, key: &Q) -> Option<(I, &K, &V)>
    where
        Q: Hash + Equivalent<K>,
    {
        let (i, k, v) = self.map.get_full(key)?;
        Some((I::from_idx(i), k, v))
    }

    pub fn get_full_mut<Q: ?Sized>(&mut self, key: &Q) -> Option<(I, &K, &mut V)>
    where
        Q: Hash + Equivalent<K>,
    {
        let (i, k, v) = self.map.get_full_mut(key)?;
        Some((I::from_idx(i), k, v))
    }

    pub fn contains_key<Q: ?Sized>(&self, key: &Q) -> bool
    where
        Q: Hash + Equivalent<K>,
    {
        self.map.contains_key(key)
    }

    pub fn ids(&self) -> EntityIds<I> {
        EntityIds::new(self.len())
    }

    pub fn iter(&self) -> Iter<'_, I, K, V> {
        Iter {
            vals: self.map.iter(),
            pos: 0,
            ids: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, I, K, V> {
        IterMut {
            vals: self.map.iter_mut(),
            pos: 0,
            ids: PhantomData,
        }
    }

    pub fn keys(&self) -> indexmap::map::Keys<'_, K, V> {
        self.map.keys()
    }

    pub fn into_keys(self) -> indexmap::map::IntoKeys<K, V> {
        self.map.into_keys()
    }

    pub fn values(&self) -> indexmap::map::Values<'_, K, V> {
        self.map.values()
    }

    pub fn values_mut(&mut self) -> indexmap::map::ValuesMut<'_, K, V> {
        self.map.values_mut()
    }

    pub fn into_values(self) -> indexmap::map::IntoValues<K, V> {
        self.map.into_values()
    }

    pub fn into_vec(self) -> EntityVec<I, (K, V)> {
        self.into_iter().map(|(_, k, v)| (k, v)).collect()
    }
}

impl<I, K, V> EntityMap<I, K, V>
where
    I: EntityId,
    K: Hash,
{
    pub fn new() -> Self {
        Self {
            map: IndexMap::new(),
            ids: PhantomData,
        }
    }

    pub fn with_capacity(n: usize) -> Self {
        Self {
            map: IndexMap::with_capacity(n),
            ids: PhantomData,
        }
    }
}

impl<I, K, V> Default for EntityMap<I, K, V>
where
    I: EntityId,
    K: Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<I, K, V, RS> fmt::Debug for EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq + fmt::Debug,
    V: fmt::Debug,
    RS: BuildHasher,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_map()
            .entries(self.iter().map(|(i, k, v)| (i, (k, v))))
            .finish()
    }
}

impl<I, K, V, RS, RS2> PartialEq<EntityMap<I, K, V, RS2>> for EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq,
    V: PartialEq,
    RS: BuildHasher,
    RS2: BuildHasher,
{
    fn eq(&self, other: &EntityMap<I, K, V, RS2>) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<I, K, V, RS> Eq for EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq,
    V: Eq,
    RS: BuildHasher,
{
}

impl<I, K, V, RS> IntoIterator for EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq,
    RS: BuildHasher,
{
    type Item = (I, K, V);
    type IntoIter = IntoIter<I, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            vals: self.map.into_iter(),
            pos: 0,
            ids: PhantomData,
        }
    }
}

impl<'a, I, K, V, RS> IntoIterator for &'a EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq,
    RS: BuildHasher,
{
    type Item = (I, &'a K, &'a V);
    type IntoIter = Iter<'a, I, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, I, K, V, RS> IntoIterator for &'a mut EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq,
    RS: BuildHasher,
{
    type Item = (I, &'a K, &'a mut V);
    type IntoIter = IterMut<'a, I, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<I, K, V, RS> Index<I> for EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq,
    RS: BuildHasher,
{
    type Output = V;
    fn index(&self, index: I) -> &V {
        self.map.index(index.to_idx())
    }
}

impl<I, K, V, RS> IndexMut<I> for EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq,
    RS: BuildHasher,
{
    fn index_mut(&mut self, index: I) -> &mut V {
        self.map.index_mut(index.to_idx())
    }
}

#[derive(Clone, Debug)]
pub struct Iter<'a, I, K: Hash, V> {
    vals: indexmap::map::Iter<'a, K, V>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<'a, I, K, V> Iterator for Iter<'a, I, K, V>
where
    I: EntityId,
    K: Hash,
{
    type Item = (I, &'a K, &'a V);
    fn next(&mut self) -> Option<(I, &'a K, &'a V)> {
        let (key, val) = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, key, val))
    }
}

impl<'a, I, K, V> DoubleEndedIterator for Iter<'a, I, K, V>
where
    I: EntityId,
    K: Hash,
{
    fn next_back(&mut self) -> Option<(I, &'a K, &'a V)> {
        let (key, val) = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), key, val))
    }
}

impl<'a, I, K, V> ExactSizeIterator for Iter<'a, I, K, V>
where
    I: EntityId,
    K: Hash,
{
    fn len(&self) -> usize {
        self.vals.len()
    }
}

pub struct IterMut<'a, I, K: Hash, V> {
    vals: indexmap::map::IterMut<'a, K, V>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<'a, I, K, V> Iterator for IterMut<'a, I, K, V>
where
    I: EntityId,
    K: Hash,
{
    type Item = (I, &'a K, &'a mut V);
    fn next(&mut self) -> Option<(I, &'a K, &'a mut V)> {
        let (key, val) = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, key, val))
    }
}

impl<'a, I, K, V> DoubleEndedIterator for IterMut<'a, I, K, V>
where
    I: EntityId,
    K: Hash,
{
    fn next_back(&mut self) -> Option<(I, &'a K, &'a mut V)> {
        let (key, val) = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), key, val))
    }
}

impl<'a, I, K, V> ExactSizeIterator for IterMut<'a, I, K, V>
where
    I: EntityId,
    K: Hash,
{
    fn len(&self) -> usize {
        self.vals.len()
    }
}

#[derive(Debug)]
pub struct IntoIter<I, K: Hash, V> {
    vals: indexmap::map::IntoIter<K, V>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<I, K, V> Iterator for IntoIter<I, K, V>
where
    I: EntityId,
    K: Hash,
{
    type Item = (I, K, V);
    fn next(&mut self) -> Option<(I, K, V)> {
        let (key, val) = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, key, val))
    }
}

impl<I, K, V> DoubleEndedIterator for IntoIter<I, K, V>
where
    I: EntityId,
    K: Hash,
{
    fn next_back(&mut self) -> Option<(I, K, V)> {
        let (key, val) = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), key, val))
    }
}

impl<I, K, V> ExactSizeIterator for IntoIter<I, K, V>
where
    I: EntityId,
    K: Hash,
{
    fn len(&self) -> usize {
        self.vals.len()
    }
}

impl<I, K, V, RS> FromIterator<(K, V)> for EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Hash + Eq,
    RS: BuildHasher + Default,
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (K, V)>,
    {
        Self {
            map: IndexMap::from_iter(iter),
            ids: PhantomData,
        }
    }
}

impl<I, K, V, RS> Serialize for EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Serialize + Hash + Eq,
    V: Serialize,
    RS: BuildHasher,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for (_, k, v) in self {
            seq.serialize_element(&(k, v))?;
        }
        seq.end()
    }
}

#[allow(clippy::type_complexity)]
struct DeserializeVisitor<I, K: Hash, V, RS: BuildHasher> {
    marker: PhantomData<fn() -> EntityMap<I, K, V, RS>>,
}

impl<I, K, V, RS> DeserializeVisitor<I, K, V, RS>
where
    K: Hash,
    RS: BuildHasher,
{
    fn new() -> Self {
        DeserializeVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, I, K, V, RS> Visitor<'de> for DeserializeVisitor<I, K, V, RS>
where
    I: EntityId,
    K: Deserialize<'de> + Hash + Eq,
    V: Deserialize<'de>,
    RS: Default + BuildHasher,
{
    type Value = EntityMap<I, K, V, RS>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("entity map")
    }

    fn visit_seq<S>(self, mut access: S) -> Result<Self::Value, S::Error>
    where
        S: SeqAccess<'de>,
    {
        let mut res = EntityMap::with_capacity_and_hasher(
            access.size_hint().unwrap_or(0),
            Default::default(),
        );

        while let Some((key, value)) = access.next_element()? {
            res.insert(key, value);
        }

        Ok(res)
    }
}

impl<'de, I, K, V, RS> Deserialize<'de> for EntityMap<I, K, V, RS>
where
    I: EntityId,
    K: Deserialize<'de> + Hash + Eq,
    V: Deserialize<'de>,
    RS: Default + BuildHasher,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(DeserializeVisitor::new())
    }
}
