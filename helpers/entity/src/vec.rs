use core::hash::Hash;
use core::marker::PhantomData;
use core::ops::{Index, IndexMut};

use std::fmt;

use serde::de::{Deserialize, Deserializer, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeSeq, Serializer};

use crate::id::EntityIds;
use crate::EntityId;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EntityVec<I, V> {
    vals: Vec<V>,
    ids: PhantomData<I>,
}

impl<I: EntityId, V> EntityVec<I, V> {
    pub fn new() -> Self {
        Self {
            vals: Vec::new(),
            ids: PhantomData,
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            vals: Vec::with_capacity(cap),
            ids: PhantomData,
        }
    }

    pub fn get(&self, idx: I) -> Option<&V> {
        self.vals.get(idx.to_idx())
    }

    pub fn get_mut(&mut self, idx: I) -> Option<&mut V> {
        self.vals.get_mut(idx.to_idx())
    }

    /// Gets a given item of the vector, without checking index validity.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `idx` is a valid index within this vector.
    pub unsafe fn get_unchecked(&self, idx: I) -> &V {
        self.vals.get_unchecked(idx.to_idx())
    }

    /// Gets a given item of the vector, without checking index validity.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `idx` is a valid index within this vector.
    pub unsafe fn get_unchecked_mut(&mut self, idx: I) -> &mut V {
        self.vals.get_unchecked_mut(idx.to_idx())
    }

    pub fn push(&mut self, val: V) -> I {
        let res = I::from_idx(self.vals.len());
        self.vals.push(val);
        res
    }

    pub fn len(&self) -> usize {
        self.vals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vals.is_empty()
    }

    pub fn clear(&mut self) {
        self.vals.clear()
    }

    pub fn ids(&self) -> EntityIds<I> {
        EntityIds::new(self.vals.len())
    }

    pub fn values(&self) -> core::slice::Iter<'_, V> {
        self.vals.iter()
    }

    pub fn values_mut(&mut self) -> core::slice::IterMut<'_, V> {
        self.vals.iter_mut()
    }

    pub fn into_values(self) -> std::vec::IntoIter<V> {
        self.vals.into_iter()
    }

    pub fn iter(&self) -> Iter<'_, I, V> {
        Iter {
            vals: self.vals.iter(),
            pos: 0,
            ids: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, I, V> {
        IterMut {
            vals: self.vals.iter_mut(),
            pos: 0,
            ids: PhantomData,
        }
    }

    pub fn map<'a, NV>(&'a self, mut f: impl FnMut(I, &'a V) -> NV) -> EntityVec<I, NV> {
        self.iter().map(|(i, v)| f(i, v)).collect()
    }

    pub fn map_values<'a, NV>(&'a self, f: impl FnMut(&'a V) -> NV) -> EntityVec<I, NV> {
        self.values().map(f).collect()
    }

    pub fn into_map_values<NV>(self, f: impl FnMut(V) -> NV) -> EntityVec<I, NV> {
        self.into_values().map(f).collect()
    }

    pub fn first(&self) -> Option<&V> {
        self.vals.first()
    }

    pub fn first_mut(&mut self) -> Option<&mut V> {
        self.vals.first_mut()
    }

    pub fn first_id(&self) -> Option<I> {
        if self.is_empty() {
            None
        } else {
            Some(I::from_idx(0))
        }
    }

    pub fn last(&self) -> Option<&V> {
        self.vals.last()
    }

    pub fn last_mut(&mut self) -> Option<&mut V> {
        self.vals.last_mut()
    }

    pub fn last_id(&self) -> Option<I> {
        if self.is_empty() {
            None
        } else {
            Some(I::from_idx(self.len() - 1))
        }
    }

    pub fn next_id(&self) -> I {
        I::from_idx(self.len())
    }

    pub fn binary_search(&self, x: &V) -> Result<I, I>
    where
        V: Ord,
    {
        match self.vals.binary_search(x) {
            Ok(x) => Ok(I::from_idx(x)),
            Err(x) => Err(I::from_idx(x)),
        }
    }

    pub fn pop(&mut self) -> Option<(I, V)> {
        self.vals.pop().map(|x| (self.next_id(), x))
    }
}

impl<I: EntityId, V> Default for EntityVec<I, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I, V> fmt::Debug for EntityVec<I, V>
where
    I: EntityId,
    V: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_map().entries(self).finish()
    }
}

impl<I: EntityId, V> IntoIterator for EntityVec<I, V> {
    type Item = (I, V);
    type IntoIter = IntoIter<I, V>;
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            vals: self.vals.into_iter(),
            pos: 0,
            ids: PhantomData,
        }
    }
}

impl<'a, I: EntityId, V> IntoIterator for &'a EntityVec<I, V> {
    type Item = (I, &'a V);
    type IntoIter = Iter<'a, I, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, I: EntityId, V> IntoIterator for &'a mut EntityVec<I, V> {
    type Item = (I, &'a mut V);
    type IntoIter = IterMut<'a, I, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<I: EntityId, V> Index<I> for EntityVec<I, V> {
    type Output = V;
    fn index(&self, index: I) -> &V {
        &self.vals[index.to_idx()]
    }
}

impl<I: EntityId, V> IndexMut<I> for EntityVec<I, V> {
    fn index_mut(&mut self, index: I) -> &mut V {
        &mut self.vals[index.to_idx()]
    }
}

#[derive(Clone, Debug)]
pub struct Iter<'a, I, V> {
    vals: core::slice::Iter<'a, V>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<'a, I: EntityId, V> Iterator for Iter<'a, I, V> {
    type Item = (I, &'a V);
    fn next(&mut self) -> Option<(I, &'a V)> {
        let val = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, val))
    }
}

impl<'a, I: EntityId, V> DoubleEndedIterator for Iter<'a, I, V> {
    fn next_back(&mut self) -> Option<(I, &'a V)> {
        let val = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), val))
    }
}

impl<'a, I: EntityId, V> ExactSizeIterator for Iter<'a, I, V> {
    fn len(&self) -> usize {
        self.vals.len()
    }
}

#[derive(Debug)]
pub struct IterMut<'a, I, V> {
    vals: core::slice::IterMut<'a, V>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<'a, I: EntityId, V> Iterator for IterMut<'a, I, V> {
    type Item = (I, &'a mut V);
    fn next(&mut self) -> Option<(I, &'a mut V)> {
        let val = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, val))
    }
}

impl<'a, I: EntityId, V> DoubleEndedIterator for IterMut<'a, I, V> {
    fn next_back(&mut self) -> Option<(I, &'a mut V)> {
        let val = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), val))
    }
}

impl<'a, I: EntityId, V> ExactSizeIterator for IterMut<'a, I, V> {
    fn len(&self) -> usize {
        self.vals.len()
    }
}

#[derive(Clone, Debug)]
pub struct IntoIter<I, V> {
    vals: std::vec::IntoIter<V>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<I: EntityId, V> Iterator for IntoIter<I, V> {
    type Item = (I, V);
    fn next(&mut self) -> Option<(I, V)> {
        let val = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, val))
    }
}

impl<I: EntityId, V> DoubleEndedIterator for IntoIter<I, V> {
    fn next_back(&mut self) -> Option<(I, V)> {
        let val = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), val))
    }
}

impl<I: EntityId, V> ExactSizeIterator for IntoIter<I, V> {
    fn len(&self) -> usize {
        self.vals.len()
    }
}

impl<I: EntityId, V> FromIterator<V> for EntityVec<I, V> {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = V>,
    {
        Self {
            vals: Vec::from_iter(iter),
            ids: PhantomData,
        }
    }
}

impl<I: EntityId, V: Serialize> Serialize for EntityVec<I, V> {
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

struct DeserializeVisitor<I, V> {
    marker: PhantomData<fn() -> EntityVec<I, V>>,
}

impl<I, V> DeserializeVisitor<I, V> {
    fn new() -> Self {
        DeserializeVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, I, V> Visitor<'de> for DeserializeVisitor<I, V>
where
    I: EntityId,
    V: Deserialize<'de>,
{
    type Value = EntityVec<I, V>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("entity vector")
    }

    fn visit_seq<S>(self, mut access: S) -> Result<Self::Value, S::Error>
    where
        S: SeqAccess<'de>,
    {
        let mut res = EntityVec::with_capacity(access.size_hint().unwrap_or(0));

        while let Some(value) = access.next_element()? {
            res.push(value);
        }

        Ok(res)
    }
}

impl<'de, I, V> Deserialize<'de> for EntityVec<I, V>
where
    I: EntityId,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(DeserializeVisitor::new())
    }
}
