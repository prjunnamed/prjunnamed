use core::hash::Hash;
use core::marker::PhantomData;
use core::ops::{Index, IndexMut};

use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

use crate::{EntityId, EntityVec};

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EntityPartVec<I, V> {
    vals: Vec<Option<V>>,
    ids: PhantomData<I>,
}

impl<I: EntityId, V> EntityPartVec<I, V> {
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

    pub fn get(&self, id: I) -> Option<&V> {
        let idx = id.to_idx();
        self.vals.get(idx).and_then(|x| x.as_ref())
    }

    pub fn get_mut(&mut self, id: I) -> Option<&mut V> {
        let idx = id.to_idx();
        self.vals.get_mut(idx).and_then(|x| x.as_mut())
    }

    pub fn contains_id(&self, id: I) -> bool {
        let idx = id.to_idx();
        self.vals.get(idx).map_or(false, |x| x.is_some())
    }

    pub fn clear(&mut self) {
        self.vals.clear()
    }

    pub fn insert(&mut self, id: I, val: V) -> Option<V> {
        let idx = id.to_idx();
        if idx >= self.vals.len() {
            self.vals.resize_with(idx + 1, Default::default);
        }
        std::mem::replace(&mut self.vals[idx], Some(val))
    }

    pub fn remove(&mut self, id: I) -> Option<V> {
        let idx = id.to_idx();
        let res = self.vals.get_mut(idx)?.take();
        while let Some(None) = self.vals.last() {
            self.vals.pop();
        }
        res
    }

    pub fn ids(&self) -> Ids<'_, I, V> {
        Ids { vals: self.iter() }
    }

    pub fn into_ids(self) -> IntoIds<I, V> {
        IntoIds {
            vals: self.into_iter(),
        }
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

    pub fn values(&self) -> Values<'_, I, V> {
        Values { vals: self.iter() }
    }

    pub fn values_mut(&mut self) -> ValuesMut<'_, I, V> {
        ValuesMut {
            vals: self.iter_mut(),
        }
    }

    pub fn into_values(self) -> IntoValues<I, V> {
        IntoValues {
            vals: self.into_iter(),
        }
    }

    pub fn into_full(mut self) -> EntityVec<I, V> {
        while matches!(self.vals.last(), Some(None)) {
            self.vals.pop();
        }
        self.vals.into_iter().map(Option::unwrap).collect()
    }

    pub fn try_into_full(mut self) -> Result<EntityVec<I, V>, I> {
        while matches!(self.vals.last(), Some(None)) {
            self.vals.pop();
        }
        let mut res = EntityVec::new();
        for val in self.vals {
            if let Some(val) = val {
                res.push(val);
            } else {
                return Err(res.next_id());
            }
        }
        Ok(res)
    }
}

impl<I: EntityId, V> Default for EntityPartVec<I, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I, V> fmt::Debug for EntityPartVec<I, V>
where
    I: EntityId,
    V: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_map().entries(self).finish()
    }
}

impl<I: EntityId, V> IntoIterator for EntityPartVec<I, V> {
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

impl<'a, I: EntityId, V> IntoIterator for &'a EntityPartVec<I, V> {
    type Item = (I, &'a V);
    type IntoIter = Iter<'a, I, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, I: EntityId, V> IntoIterator for &'a mut EntityPartVec<I, V> {
    type Item = (I, &'a mut V);
    type IntoIter = IterMut<'a, I, V>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<I: EntityId, V> Index<I> for EntityPartVec<I, V> {
    type Output = V;
    fn index(&self, index: I) -> &V {
        self.vals[index.to_idx()].as_ref().unwrap()
    }
}

impl<I: EntityId, V> IndexMut<I> for EntityPartVec<I, V> {
    fn index_mut(&mut self, index: I) -> &mut V {
        self.vals[index.to_idx()].as_mut().unwrap()
    }
}

#[derive(Clone, Debug)]
pub struct Iter<'a, I, V> {
    vals: core::slice::Iter<'a, Option<V>>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<'a, I: EntityId, V> Iterator for Iter<'a, I, V> {
    type Item = (I, &'a V);
    fn next(&mut self) -> Option<(I, &'a V)> {
        loop {
            let id = I::from_idx(self.pos);
            let val = self.vals.next()?;
            self.pos += 1;
            if let Some(val) = val {
                return Some((id, val));
            }
        }
    }
}

impl<'a, I: EntityId, V> DoubleEndedIterator for Iter<'a, I, V> {
    fn next_back(&mut self) -> Option<(I, &'a V)> {
        loop {
            if let Some(val) = self.vals.next_back()? {
                return Some((I::from_idx(self.pos + self.vals.len()), val));
            }
        }
    }
}

#[derive(Debug)]
pub struct IterMut<'a, I, V> {
    vals: core::slice::IterMut<'a, Option<V>>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<'a, I: EntityId, V> Iterator for IterMut<'a, I, V> {
    type Item = (I, &'a mut V);
    fn next(&mut self) -> Option<(I, &'a mut V)> {
        loop {
            let id = I::from_idx(self.pos);
            let val = self.vals.next()?;
            self.pos += 1;
            if let Some(val) = val {
                return Some((id, val));
            }
        }
    }
}

impl<'a, I: EntityId, V> DoubleEndedIterator for IterMut<'a, I, V> {
    fn next_back(&mut self) -> Option<(I, &'a mut V)> {
        loop {
            if let Some(val) = self.vals.next_back()? {
                return Some((I::from_idx(self.pos + self.vals.len()), val));
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct IntoIter<I, V> {
    vals: std::vec::IntoIter<Option<V>>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<I: EntityId, V> Iterator for IntoIter<I, V> {
    type Item = (I, V);
    fn next(&mut self) -> Option<(I, V)> {
        loop {
            let id = I::from_idx(self.pos);
            let val = self.vals.next()?;
            self.pos += 1;
            if let Some(val) = val {
                return Some((id, val));
            }
        }
    }
}

impl<I: EntityId, V> DoubleEndedIterator for IntoIter<I, V> {
    fn next_back(&mut self) -> Option<(I, V)> {
        loop {
            if let Some(val) = self.vals.next_back()? {
                return Some((I::from_idx(self.pos + self.vals.len()), val));
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ids<'a, I, V> {
    vals: Iter<'a, I, V>,
}

impl<'a, I: EntityId, V> Iterator for Ids<'a, I, V> {
    type Item = I;
    fn next(&mut self) -> Option<I> {
        self.vals.next().map(|x| x.0)
    }
}

impl<'a, I: EntityId, V> DoubleEndedIterator for Ids<'a, I, V> {
    fn next_back(&mut self) -> Option<I> {
        self.vals.next_back().map(|x| x.0)
    }
}

#[derive(Clone, Debug)]
pub struct IntoIds<I, V> {
    vals: IntoIter<I, V>,
}

impl<I: EntityId, V> Iterator for IntoIds<I, V> {
    type Item = I;
    fn next(&mut self) -> Option<I> {
        self.vals.next().map(|x| x.0)
    }
}

impl<I: EntityId, V> DoubleEndedIterator for IntoIds<I, V> {
    fn next_back(&mut self) -> Option<I> {
        self.vals.next_back().map(|x| x.0)
    }
}

#[derive(Clone, Debug)]
pub struct Values<'a, I, V> {
    vals: Iter<'a, I, V>,
}

impl<'a, I: EntityId, V> Iterator for Values<'a, I, V> {
    type Item = &'a V;
    fn next(&mut self) -> Option<&'a V> {
        self.vals.next().map(|x| x.1)
    }
}

impl<'a, I: EntityId, V> DoubleEndedIterator for Values<'a, I, V> {
    fn next_back(&mut self) -> Option<&'a V> {
        self.vals.next_back().map(|x| x.1)
    }
}

#[derive(Debug)]
pub struct ValuesMut<'a, I, V> {
    vals: IterMut<'a, I, V>,
}

impl<'a, I: EntityId, V> Iterator for ValuesMut<'a, I, V> {
    type Item = &'a mut V;
    fn next(&mut self) -> Option<&'a mut V> {
        self.vals.next().map(|x| x.1)
    }
}

impl<'a, I: EntityId, V> DoubleEndedIterator for ValuesMut<'a, I, V> {
    fn next_back(&mut self) -> Option<&'a mut V> {
        self.vals.next_back().map(|x| x.1)
    }
}

#[derive(Clone, Debug)]
pub struct IntoValues<I, V> {
    vals: IntoIter<I, V>,
}

impl<I: EntityId, V> Iterator for IntoValues<I, V> {
    type Item = V;
    fn next(&mut self) -> Option<V> {
        self.vals.next().map(|x| x.1)
    }
}

impl<I: EntityId, V> DoubleEndedIterator for IntoValues<I, V> {
    fn next_back(&mut self) -> Option<V> {
        self.vals.next_back().map(|x| x.1)
    }
}

impl<I: EntityId, V: Serialize> Serialize for EntityPartVec<I, V> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.iter().count()))?;
        for (k, v) in self {
            map.serialize_entry(&k, v)?;
        }
        map.end()
    }
}

impl<I: EntityId, V> FromIterator<(I, V)> for EntityPartVec<I, V> {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (I, V)>,
    {
        let mut res = Self::new();
        for (k, v) in iter {
            res.insert(k, v);
        }
        res
    }
}

struct DeserializeVisitor<I, V> {
    marker: PhantomData<fn() -> EntityPartVec<I, V>>,
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
    type Value = EntityPartVec<I, V>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("entity partial vector")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = EntityPartVec::with_capacity(access.size_hint().unwrap_or(0));

        while let Some((id, value)) = access.next_entry()? {
            map.insert(id, value);
        }

        Ok(map)
    }
}

impl<'de, I, V> Deserialize<'de> for EntityPartVec<I, V>
where
    I: EntityId,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(DeserializeVisitor::new())
    }
}
