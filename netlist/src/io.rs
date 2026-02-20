use std::borrow::Cow;
use std::fmt::{Debug, Display};
use std::ops::{Index, IndexMut, Range};
use std::slice::SliceIndex;

#[derive(Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct IoNet {
    pub index: u32,
}

impl IoNet {
    pub const FLOATING: IoNet = IoNet { index: u32::MAX };

    pub fn is_floating(self) -> bool {
        self.index == u32::MAX
    }
}

impl Debug for IoNet {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            IoNet { index: u32::MAX } => write!(f, "IoNet::FLOATING"),
            IoNet { index } => write!(f, "IoNet {{ index: {index} }}"),
        }
    }
}

impl Display for IoNet {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            IoNet { index: u32::MAX } => write!(f, "#_"),
            IoNet { index } => write!(f, "#{index}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct IoValue {
    nets: Vec<IoNet>,
}

impl IoValue {
    pub const fn new() -> Self {
        IoValue { nets: vec![] }
    }

    pub fn from_range(range: Range<u32>) -> Self {
        Self { nets: range.map(|i| IoNet { index: i }).collect() }
    }

    pub fn floating(width: usize) -> Self {
        Self { nets: vec![IoNet::FLOATING; width] }
    }

    pub fn len(&self) -> usize {
        self.nets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nets.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = IoNet> + ExactSizeIterator + '_ {
        self.nets.iter().copied()
    }

    pub fn concat<'a>(&self, other: impl Into<Cow<'a, IoValue>>) -> Self {
        Self::from_iter(self.iter().chain(other.into().iter()))
    }

    pub fn slice(&self, range: impl std::ops::RangeBounds<usize>) -> IoValue {
        IoValue::from_iter(self[(range.start_bound().cloned(), range.end_bound().cloned())].iter().copied())
    }
}

impl From<IoNet> for IoValue {
    fn from(value: IoNet) -> Self {
        Self { nets: vec![value] }
    }
}

impl From<IoNet> for Cow<'_, IoValue> {
    fn from(value: IoNet) -> Self {
        Cow::Owned(IoValue::from(value))
    }
}

impl FromIterator<IoNet> for IoValue {
    fn from_iter<T: IntoIterator<Item = IoNet>>(iter: T) -> Self {
        IoValue { nets: iter.into_iter().collect() }
    }
}

impl<I: SliceIndex<[IoNet]>> Index<I> for IoValue {
    type Output = I::Output;

    fn index(&self, index: I) -> &Self::Output {
        &self.nets[index]
    }
}

impl<I: SliceIndex<[IoNet]>> IndexMut<I> for IoValue {
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.nets[index]
    }
}

impl IntoIterator for &IoValue {
    type Item = IoNet;
    type IntoIter = std::vec::IntoIter<IoNet>;

    fn into_iter(self) -> Self::IntoIter {
        self.nets.clone().into_iter()
    }
}

impl IntoIterator for IoValue {
    type Item = IoNet;
    type IntoIter = std::vec::IntoIter<IoNet>;

    fn into_iter(self) -> Self::IntoIter {
        self.nets.into_iter()
    }
}

impl Extend<IoNet> for IoValue {
    fn extend<T: IntoIterator<Item = IoNet>>(&mut self, iter: T) {
        self.nets.extend(iter);
    }
}
