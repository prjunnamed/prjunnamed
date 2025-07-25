use std::{
    borrow::Cow,
    fmt::{Debug, Display},
    ops::{Index, IndexMut},
    hash::Hash,
    slice::SliceIndex,
};

use crate::{Const, Design, Trit};

/// A net is a driver in the design; either a constant (a [`Trit`]) or a reference to a single position from
/// the output of a [`Cell`].
///
/// [`Cell`]: crate::Cell
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Net {
    pub(crate) index: u32,
}

impl Net {
    pub const UNDEF: Net = Net { index: u32::MAX };
    pub const ZERO: Net = Net { index: 0 };
    pub const ONE: Net = Net { index: 1 };

    const FIRST_CELL: u32 = 2; // ZERO, ONE, then cells

    pub fn as_const(self) -> Option<Trit> {
        if self == Self::UNDEF {
            Some(Trit::Undef)
        } else if self == Self::ZERO {
            Some(Trit::Zero)
        } else if self == Self::ONE {
            Some(Trit::One)
        } else {
            None
        }
    }

    pub(crate) fn from_cell_index(cell_index: usize) -> Net {
        assert!(cell_index <= u32::MAX as usize - 3);
        Net { index: cell_index as u32 + Net::FIRST_CELL }
    }

    pub(crate) fn as_cell_index(self) -> Result<usize, Trit> {
        if self.index >= Self::FIRST_CELL && self != Self::UNDEF {
            Ok((self.index - Self::FIRST_CELL) as usize)
        } else {
            Err(self.as_const().unwrap())
        }
    }

    pub fn is_const(self) -> bool {
        self.as_const().is_some()
    }

    pub fn is_cell(self) -> bool {
        self.as_const().is_none()
    }

    pub fn visit(self, mut f: impl FnMut(Net)) {
        f(self)
    }

    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        f(self)
    }

    pub fn repeat(self, count: usize) -> Value {
        Value::from_iter(std::iter::repeat_n(self, count))
    }
}

impl From<bool> for Net {
    fn from(value: bool) -> Self {
        match value {
            false => Net::ZERO,
            true => Net::ONE,
        }
    }
}

impl From<Trit> for Net {
    fn from(value: Trit) -> Self {
        match value {
            Trit::Undef => Self::UNDEF,
            Trit::Zero => Self::ZERO,
            Trit::One => Self::ONE,
        }
    }
}

impl From<&Net> for Net {
    fn from(net: &Net) -> Self {
        *net
    }
}

impl TryFrom<Value> for Net {
    type Error = ();

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        value.as_net().ok_or(())
    }
}

impl Debug for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Net { index: 0 } => write!(f, "Net::ZERO"),
            Net { index: 1 } => write!(f, "Net::ONE"),
            Net { index: u32::MAX } => write!(f, "Net::UNDEF"),
            _ => {
                let cell_index = self.index.checked_sub(Net::FIRST_CELL).unwrap();
                write!(f, "Net::from_cell({cell_index})")
            }
        }
    }
}

impl Display for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Net { index: 0 } => write!(f, "0"),
            Net { index: 1 } => write!(f, "1"),
            Net { index: u32::MAX } => write!(f, "X"),
            _ => {
                let cell_index = self.index.checked_sub(Net::FIRST_CELL).unwrap();
                write!(f, "%_{cell_index}")
            }
        }
    }
}

#[derive(Clone)]
enum ValueRepr {
    None,
    Some(Net),
    Many(Vec<Net>),
}

impl ValueRepr {
    fn as_slice(&self) -> &[Net] {
        match self {
            ValueRepr::None => &[],
            ValueRepr::Some(net) => std::slice::from_ref(net),
            ValueRepr::Many(nets) => nets.as_slice(),
        }
    }

    fn as_slice_mut(&mut self) -> &mut [Net] {
        match self {
            ValueRepr::None => &mut [],
            ValueRepr::Some(net) => std::slice::from_mut(net),
            ValueRepr::Many(nets) => nets.as_mut_slice(),
        }
    }

    fn push(&mut self, new_net: Net) {
        match self {
            ValueRepr::None => *self = ValueRepr::Some(new_net),
            ValueRepr::Some(net) => *self = ValueRepr::Many(vec![*net, new_net]),
            ValueRepr::Many(nets) => {
                nets.push(new_net);
            }
        }
    }
}

impl PartialEq for ValueRepr {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ValueRepr::Some(lft), ValueRepr::Some(rgt)) => lft.eq(rgt),
            _ => self.as_slice().eq(other.as_slice()),
        }
    }
}

impl Eq for ValueRepr {}

impl PartialOrd for ValueRepr {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (ValueRepr::Some(lft), ValueRepr::Some(rgt)) => lft.partial_cmp(rgt),
            _ => self.as_slice().partial_cmp(other.as_slice()),
        }
    }
}

impl Ord for ValueRepr {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (ValueRepr::Some(lft), ValueRepr::Some(rgt)) => lft.cmp(rgt),
            _ => self.as_slice().cmp(other.as_slice()),
        }
    }
}

impl Hash for ValueRepr {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

/// A value is a (possibly empty) sequence of [`Net`]s.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Value(ValueRepr);

impl Value {
    /// Creates an empty value.
    pub fn new() -> Self {
        Value(ValueRepr::None)
    }

    /// Creates an all-`0` value of given width.
    pub fn zero(width: usize) -> Self {
        Net::ZERO.repeat(width)
    }

    /// Creates an all-`1` value of given width.
    pub fn ones(width: usize) -> Self {
        Net::ONE.repeat(width)
    }

    /// Creates an all-`X` value of given width.
    pub fn undef(width: usize) -> Self {
        Net::UNDEF.repeat(width)
    }

    /// Creates a reference to `count` outputs of cell at position `cell_index` in their natural order.
    pub(crate) fn from_cell_range(cell_index: usize, count: usize) -> Value {
        Value::from_iter((cell_index..(cell_index + count)).map(Net::from_cell_index))
    }

    pub fn len(&self) -> usize {
        self.0.as_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.as_slice().is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Net> + ExactSizeIterator + '_ {
        self.0.as_slice().iter().copied()
    }

    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut Net> + ExactSizeIterator + '_ {
        self.0.as_slice_mut().iter_mut()
    }

    pub fn push(&mut self, new_net: impl Into<Net>) {
        self.0.push(new_net.into())
    }

    pub fn is_undef(&self) -> bool {
        self.iter().all(|net| net == Net::UNDEF)
    }

    pub fn is_zero(&self) -> bool {
        self.iter().all(|net| net == Net::ZERO)
    }

    pub fn is_ones(&self) -> bool {
        self.iter().all(|net| net == Net::ONE)
    }

    pub fn lsb(&self) -> Net {
        self[0]
    }

    pub fn msb(&self) -> Net {
        self[self.len() - 1]
    }

    pub fn has_undef(&self) -> bool {
        self.iter().any(|net| net == Net::UNDEF)
    }

    pub fn as_const(&self) -> Option<Const> {
        let nets = self.0.as_slice();
        if nets.iter().all(|net| net.is_const()) {
            Some(Const::from_iter(nets.iter().map(|net| net.as_const().unwrap())))
        } else {
            None
        }
    }

    pub fn as_net(&self) -> Option<Net> {
        if self.len() == 1 { Some(self[0]) } else { None }
    }

    pub fn unwrap_net(&self) -> Net {
        self.as_net().unwrap()
    }

    pub fn concat<'a>(&self, other: impl Into<Cow<'a, Value>>) -> Self {
        Value::from_iter(self.iter().chain(other.into().iter()))
    }

    pub fn repeat(&self, count: usize) -> Self {
        Value::from_iter((0..count).flat_map(|_| self))
    }

    pub fn slice(&self, range: impl std::ops::RangeBounds<usize>) -> Value {
        Value::from(&self[(range.start_bound().cloned(), range.end_bound().cloned())])
    }

    pub fn zext(&self, width: usize) -> Self {
        assert!(width >= self.len());
        self.concat(Value::zero(width - self.len()))
    }

    pub fn sext(&self, width: usize) -> Self {
        assert!(!self.is_empty());
        assert!(width >= self.len());
        Value::from_iter(self.iter().chain(std::iter::repeat_n(self.msb(), width - self.len())))
    }

    fn shift_count(value: &Const, stride: u32) -> usize {
        let stride = stride as usize;
        let mut result: usize = 0;
        for (index, trit) in value.iter().enumerate() {
            if trit == Trit::One {
                if index >= usize::BITS as usize {
                    return usize::MAX;
                } else {
                    result |= 1 << index;
                }
            }
        }
        result.checked_mul(stride).unwrap_or(usize::MAX)
    }

    pub fn shl<'a>(&self, other: impl Into<Cow<'a, Const>>, stride: u32) -> Value {
        let other = other.into();
        if other.has_undef() {
            return Value::undef(self.len());
        }
        let shcnt = Self::shift_count(&other, stride);
        if shcnt >= self.len() {
            return Value::zero(self.len());
        }
        Value::zero(shcnt).concat(Value::from(&self[..self.len() - shcnt]))
    }

    pub fn ushr<'a>(&self, other: impl Into<Cow<'a, Const>>, stride: u32) -> Value {
        let other = other.into();
        if other.has_undef() {
            return Value::undef(self.len());
        }
        let shcnt = Self::shift_count(&other, stride);
        if shcnt >= self.len() {
            return Value::zero(self.len());
        }
        Value::from(&self[shcnt..]).zext(self.len())
    }

    pub fn sshr<'a>(&self, other: impl Into<Cow<'a, Const>>, stride: u32) -> Value {
        let other = other.into();
        if other.has_undef() {
            return Value::undef(self.len());
        }
        let shcnt = Self::shift_count(&other, stride);
        if shcnt >= self.len() {
            return Value::from(self.msb()).sext(self.len());
        }
        Value::from(&self[shcnt..]).sext(self.len())
    }

    pub fn xshr<'a>(&self, other: impl Into<Cow<'a, Const>>, stride: u32) -> Value {
        let other = other.into();
        if other.has_undef() {
            return Value::undef(self.len());
        }
        let shcnt = Self::shift_count(&other, stride);
        if shcnt >= self.len() {
            return Value::undef(self.len());
        }
        Value::from(&self[shcnt..]).concat(Value::undef(shcnt))
    }

    pub fn visit(&self, mut f: impl FnMut(Net)) {
        for net in self.iter() {
            f(net)
        }
    }

    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        for net in self.iter_mut() {
            f(net)
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::new()
    }
}

impl Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Value::from_iter([")?;
        for (index, net) in self.iter().enumerate() {
            if index != 0 {
                write!(f, ", ")?;
            }
            write!(f, "{net:?}")?;
        }
        write!(f, "])")?;
        Ok(())
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            write!(f, "[]")
        } else if self.len() == 1 {
            write!(f, "{}", self[0])
        } else {
            write!(f, "[")?;
            for net in self.iter().rev() {
                write!(f, " {net}")?;
            }
            write!(f, " ]")
        }
    }
}

impl<I: SliceIndex<[Net]>> Index<I> for Value {
    type Output = I::Output;

    fn index(&self, index: I) -> &Self::Output {
        &self.0.as_slice()[index]
    }
}

impl<I: SliceIndex<[Net]>> IndexMut<I> for Value {
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.0.as_slice_mut()[index]
    }
}

impl Extend<Net> for Value {
    fn extend<T: IntoIterator<Item = Net>>(&mut self, iter: T) {
        for net in iter {
            self.push(net);
        }
    }
}

impl From<&Value> for Value {
    fn from(value: &Value) -> Self {
        value.clone()
    }
}

impl From<Net> for Value {
    fn from(net: Net) -> Self {
        Value(ValueRepr::Some(net))
    }
}

impl From<&Net> for Value {
    fn from(net: &Net) -> Self {
        Value::from(*net)
    }
}

impl From<Trit> for Value {
    fn from(trit: Trit) -> Self {
        Value(ValueRepr::Some(trit.into()))
    }
}

impl From<&[Net]> for Value {
    fn from(nets: &[Net]) -> Self {
        Value::from_iter(nets.iter().cloned())
    }
}

impl From<Vec<Net>> for Value {
    fn from(nets: Vec<Net>) -> Self {
        Value::from(&nets[..])
    }
}

impl From<&Const> for Value {
    fn from(value: &Const) -> Self {
        Value::from_iter(value.into_iter().map(Net::from))
    }
}

impl From<Const> for Value {
    fn from(value: Const) -> Self {
        Value::from(&value)
    }
}

impl From<Value> for Cow<'_, Value> {
    fn from(value: Value) -> Self {
        Cow::Owned(value)
    }
}

impl From<Net> for Cow<'_, Value> {
    fn from(value: Net) -> Self {
        Cow::Owned(Value::from(value))
    }
}

impl From<Trit> for Cow<'_, Value> {
    fn from(value: Trit) -> Self {
        Cow::Owned(Value::from(Net::from(value)))
    }
}

impl From<&Const> for Cow<'_, Value> {
    fn from(value: &Const) -> Self {
        Cow::Owned(Value::from(value))
    }
}

impl From<Const> for Cow<'_, Value> {
    fn from(value: Const) -> Self {
        Cow::Owned(Value::from(value))
    }
}

impl<'a> From<&'a Value> for Cow<'a, Value> {
    fn from(value: &'a Value) -> Self {
        Cow::Borrowed(value)
    }
}

impl FromIterator<Net> for Value {
    fn from_iter<T: IntoIterator<Item = Net>>(iter: T) -> Self {
        let mut iter = iter.into_iter();
        match iter.size_hint() {
            (_, Some(0 | 1)) => {
                let mut value = match iter.next() {
                    None => Value::new(),
                    Some(net) => Value::from(net),
                };
                while let Some(net) = iter.next() {
                    value.push(net);
                }
                value
            }
            _ => Value(ValueRepr::Many(iter.collect())),
        }
    }
}

impl<'a> IntoIterator for &'a Value {
    type Item = Net;
    type IntoIter = std::iter::Cloned<std::slice::Iter<'a, Net>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.as_slice().iter().cloned()
    }
}

pub struct ValueIntoIter {
    repr: ValueRepr,
    index: usize,
}

impl Iterator for ValueIntoIter {
    type Item = Net;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.repr.as_slice().get(self.index).cloned();
        if item.is_some() {
            self.index += 1;
        }
        item
    }
}

impl IntoIterator for Value {
    type Item = Net;
    type IntoIter = ValueIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        ValueIntoIter { repr: self.0, index: 0 }
    }
}

/// A control net is a [`Net`] that can be negated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControlNet {
    Pos(Net),
    Neg(Net),
}

impl ControlNet {
    pub const UNDEF: ControlNet = ControlNet::Pos(Net::UNDEF);
    pub const ZERO: ControlNet = ControlNet::Pos(Net::ZERO);
    pub const ONE: ControlNet = ControlNet::Pos(Net::ONE);

    pub fn from_net_invert(net: Net, invert: bool) -> Self {
        match invert {
            false => ControlNet::Pos(net),
            true => ControlNet::Neg(net),
        }
    }

    pub fn net(self) -> Net {
        match self {
            Self::Pos(net) => net,
            Self::Neg(net) => net,
        }
    }

    pub fn is_positive(self) -> bool {
        matches!(self, Self::Pos(_))
    }

    pub fn is_negative(self) -> bool {
        matches!(self, Self::Neg(_))
    }

    pub fn is_active(self) -> Option<bool> {
        match self {
            Self::Pos(net) if net == Net::ZERO => Some(false),
            Self::Neg(net) if net == Net::ONE => Some(false),
            Self::Pos(net) if net == Net::ONE => Some(true),
            Self::Neg(net) if net == Net::ZERO => Some(true),
            _ => None,
        }
    }

    pub fn is_always(self, active: bool) -> bool {
        self.is_active() == Some(active)
    }

    pub fn is_const(self) -> bool {
        self.net().as_const().is_some()
    }

    pub fn canonicalize(self) -> Self {
        match self {
            Self::Neg(net) if net == Net::UNDEF => Self::Pos(net),
            Self::Neg(net) if net == Net::ZERO => Self::Pos(Net::ONE),
            Self::Neg(net) if net == Net::ONE => Self::Pos(Net::ZERO),
            _ => self,
        }
    }

    pub fn into_pos(self, design: &Design) -> Net {
        match self {
            ControlNet::Pos(net) => net,
            ControlNet::Neg(net) => {
                if let Some(trit) = net.as_const() {
                    Net::from(!trit)
                } else {
                    design.add_not(net).unwrap_net()
                }
            }
        }
    }

    pub fn into_neg(self, design: &Design) -> Net {
        match self {
            ControlNet::Pos(net) => {
                if let Some(trit) = net.as_const() {
                    Net::from(!trit)
                } else {
                    design.add_not(net).unwrap_net()
                }
            }
            ControlNet::Neg(net) => net,
        }
    }

    pub fn visit(self, f: impl FnMut(Net)) {
        match self {
            ControlNet::Pos(net) => net.visit(f),
            ControlNet::Neg(net) => net.visit(f),
        }
    }

    pub fn visit_mut(&mut self, f: impl FnMut(&mut Net)) {
        match self {
            ControlNet::Pos(net) => net.visit_mut(f),
            ControlNet::Neg(net) => net.visit_mut(f),
        }
    }
}

impl std::ops::Not for ControlNet {
    type Output = ControlNet;

    fn not(self) -> Self::Output {
        match self {
            ControlNet::Pos(net) => ControlNet::Neg(net),
            ControlNet::Neg(net) => ControlNet::Pos(net),
        }
        .canonicalize()
    }
}

impl From<Net> for ControlNet {
    fn from(net: Net) -> Self {
        ControlNet::Pos(net)
    }
}

impl Display for ControlNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlNet::Pos(net) => write!(f, "{net}"),
            ControlNet::Neg(net) => write!(f, "!{net}"),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{Net, Trit, Value};

    #[test]
    fn test_net() {
        assert_eq!(Net::from(Trit::Zero), Net::ZERO);
        assert_eq!(Net::from(Trit::One), Net::ONE);
        assert_eq!(Net::from(Trit::Undef), Net::UNDEF);
        assert_eq!(Net::from_cell_index(3), Net { index: 5 });
    }

    #[test]
    fn test_from_bool() {
        assert_eq!(Net::from(false), Net::ZERO);
        assert_eq!(Net::from(true), Net::ONE);
    }

    #[test]
    fn test_from_trit() {
        assert_eq!(Net::from(Trit::Zero), Net::ZERO);
        assert_eq!(Net::from(Trit::One), Net::ONE);
        assert_eq!(Net::from(Trit::Undef), Net::UNDEF);
    }

    #[test]
    fn test_net_debug() {
        assert_eq!(format!("{:?}", Net::ZERO), "Net::ZERO");
        assert_eq!(format!("{:?}", Net::ONE), "Net::ONE");
        assert_eq!(format!("{:?}", Net::UNDEF), "Net::UNDEF");
        assert_eq!(format!("{:?}", Net::from_cell_index(0)), "Net::from_cell(0)");
    }

    #[test]
    fn test_value() {
        let v01 = Value::from_iter([Net::ONE, Net::ZERO]);
        assert_eq!(v01.into_iter().collect::<Vec<_>>(), vec![Net::ONE, Net::ZERO]);
    }
}
