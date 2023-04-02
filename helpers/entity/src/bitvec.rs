use core::hash::Hash;
use core::marker::PhantomData;
use core::ops::Index;

use std::fmt;

use serde::de::{Deserialize, Deserializer, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeSeq, Serializer};

use crate::id::EntityIds;
use crate::EntityId;

use bitvec::order::Lsb0;
use bitvec::vec::BitVec;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EntityBitVec<I> {
    vals: BitVec,
    ids: PhantomData<I>,
}

impl<I: EntityId> EntityBitVec<I> {
    pub fn new() -> Self {
        Self {
            vals: BitVec::new(),
            ids: PhantomData,
        }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            vals: BitVec::with_capacity(cap),
            ids: PhantomData,
        }
    }

    pub fn repeat(bit: bool, len: usize) -> Self {
        Self {
            vals: BitVec::repeat(bit, len),
            ids: PhantomData,
        }
    }

    pub fn push(&mut self, val: bool) -> I {
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

    pub fn values(&self) -> bitvec::slice::Iter<'_, usize, Lsb0> {
        self.vals.iter()
    }

    pub fn values_mut(&mut self) -> bitvec::slice::IterMut<'_, usize, Lsb0> {
        self.vals.iter_mut()
    }

    pub fn into_values(self) -> bitvec::vec::IntoIter {
        self.vals.into_iter()
    }

    pub fn iter(&self) -> Iter<'_, I> {
        Iter {
            vals: self.vals.iter(),
            pos: 0,
            ids: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, I> {
        IterMut {
            vals: self.vals.iter_mut(),
            pos: 0,
            ids: PhantomData,
        }
    }

    pub fn first_id(&self) -> Option<I> {
        if self.is_empty() {
            None
        } else {
            Some(I::from_idx(0))
        }
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

    pub fn pop(&mut self) -> Option<(I, bool)> {
        self.vals.pop().map(|x| (self.next_id(), x))
    }

    pub fn set(&mut self, index: I, value: bool) {
        self.vals.set(index.to_idx(), value);
    }
}

impl<I: EntityId> Default for EntityBitVec<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I> fmt::Debug for EntityBitVec<I>
where
    I: EntityId,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_map().entries(self).finish()
    }
}

impl<I: EntityId> IntoIterator for EntityBitVec<I> {
    type Item = (I, bool);
    type IntoIter = IntoIter<I>;
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            vals: self.vals.into_iter(),
            pos: 0,
            ids: PhantomData,
        }
    }
}

impl<'a, I: EntityId> IntoIterator for &'a EntityBitVec<I> {
    type Item = (I, <bitvec::slice::Iter<'a, usize, Lsb0> as Iterator>::Item);
    type IntoIter = Iter<'a, I>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, I: EntityId> IntoIterator for &'a mut EntityBitVec<I> {
    type Item = (
        I,
        <bitvec::slice::IterMut<'a, usize, Lsb0> as Iterator>::Item,
    );
    type IntoIter = IterMut<'a, I>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<I: EntityId> Index<I> for EntityBitVec<I> {
    type Output = <BitVec as Index<usize>>::Output;
    fn index(&self, index: I) -> &Self::Output {
        &self.vals[index.to_idx()]
    }
}

#[derive(Clone, Debug)]
pub struct Iter<'a, I> {
    vals: bitvec::slice::Iter<'a, usize, Lsb0>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<'a, I: EntityId> Iterator for Iter<'a, I> {
    type Item = (I, <bitvec::slice::Iter<'a, usize, Lsb0> as Iterator>::Item);
    fn next(&mut self) -> Option<Self::Item> {
        let val = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, val))
    }
}

impl<'a, I: EntityId> DoubleEndedIterator for Iter<'a, I> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let val = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), val))
    }
}

impl<'a, I: EntityId> ExactSizeIterator for Iter<'a, I> {
    fn len(&self) -> usize {
        self.vals.len()
    }
}

#[derive(Debug)]
pub struct IterMut<'a, I> {
    vals: bitvec::slice::IterMut<'a, usize, Lsb0>,
    pos: usize,
    ids: PhantomData<I>,
}

impl<'a, I: EntityId> Iterator for IterMut<'a, I> {
    type Item = (
        I,
        <bitvec::slice::IterMut<'a, usize, Lsb0> as Iterator>::Item,
    );
    fn next(&mut self) -> Option<Self::Item> {
        let val = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, val))
    }
}

impl<'a, I: EntityId> DoubleEndedIterator for IterMut<'a, I> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let val = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), val))
    }
}

impl<'a, I: EntityId> ExactSizeIterator for IterMut<'a, I> {
    fn len(&self) -> usize {
        self.vals.len()
    }
}

#[derive(Clone, Debug)]
pub struct IntoIter<I> {
    vals: bitvec::vec::IntoIter,
    pos: usize,
    ids: PhantomData<I>,
}

impl<I: EntityId> Iterator for IntoIter<I> {
    type Item = (I, bool);
    fn next(&mut self) -> Option<(I, bool)> {
        let val = self.vals.next()?;
        let id = I::from_idx(self.pos);
        self.pos += 1;
        Some((id, val))
    }
}

impl<I: EntityId> DoubleEndedIterator for IntoIter<I> {
    fn next_back(&mut self) -> Option<(I, bool)> {
        let val = self.vals.next_back()?;
        Some((I::from_idx(self.pos + self.vals.len()), val))
    }
}

impl<I: EntityId> ExactSizeIterator for IntoIter<I> {
    fn len(&self) -> usize {
        self.vals.len()
    }
}

impl<I: EntityId> FromIterator<bool> for EntityBitVec<I> {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = bool>,
    {
        Self {
            vals: BitVec::from_iter(iter),
            ids: PhantomData,
        }
    }
}

impl<I: EntityId> Serialize for EntityBitVec<I> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for v in self.values() {
            seq.serialize_element(&*v)?;
        }
        seq.end()
    }
}

struct DeserializeVisitor<I> {
    marker: PhantomData<fn() -> EntityBitVec<I>>,
}

impl<I> DeserializeVisitor<I> {
    fn new() -> Self {
        DeserializeVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, I> Visitor<'de> for DeserializeVisitor<I>
where
    I: EntityId,
{
    type Value = EntityBitVec<I>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("entity vector")
    }

    fn visit_seq<S>(self, mut access: S) -> Result<Self::Value, S::Error>
    where
        S: SeqAccess<'de>,
    {
        let mut res = EntityBitVec::with_capacity(access.size_hint().unwrap_or(0));

        while let Some(value) = access.next_element()? {
            res.push(value);
        }

        Ok(res)
    }
}

impl<'de, I> Deserialize<'de> for EntityBitVec<I>
where
    I: EntityId,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(DeserializeVisitor::new())
    }
}
