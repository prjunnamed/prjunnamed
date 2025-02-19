use std::{
    borrow::Cow,
    fmt::{Debug, Display},
    ops::{Index, IndexMut},
    slice::SliceIndex,
    str::FromStr,
};

use crate::Net;

/// An extended binary value.
///
/// In addition to the usual `0` and `1`, the `X` value (also known as "undef") means that either `0` and `1`
/// may be encountered. The undef value is used in both optimization and simulation, and its exact semantics
/// depends on the context where it is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Trit {
    Undef = -1,
    Zero = 0,
    One = 1,
}

impl Trit {
    pub fn from_char(chr: char) -> Result<Self, ()> {
        match chr {
            '0' => Ok(Trit::Zero),
            '1' => Ok(Trit::One),
            'X' => Ok(Trit::Undef),
            _ => Err(()),
        }
    }

    pub fn lit(chr: char) -> Self {
        Self::from_char(chr).unwrap()
    }

    pub fn combine(lft: Trit, rgt: Trit) -> Option<Trit> {
        match (lft, rgt) {
            (Trit::Undef, trit) | (trit, Trit::Undef) => Some(trit),
            _ if lft == rgt => Some(lft),
            _ => None,
        }
    }

    pub fn mux<'a, 'b>(self, arg1: impl Into<Cow<'a, Const>>, arg2: impl Into<Cow<'b, Const>>) -> Const {
        let arg1 = arg1.into();
        let arg2 = arg2.into();
        match self {
            Trit::One => arg1.into_owned(),
            Trit::Zero => arg2.into_owned(),
            Trit::Undef => {
                Const::from_iter(arg1.iter().zip(arg2.iter()).map(|(x, y)| if x == y { x } else { Trit::Undef }))
            }
        }
    }
}

impl Display for Trit {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Trit::Undef => write!(f, "X"),
            Trit::Zero => write!(f, "0"),
            Trit::One => write!(f, "1"),
        }
    }
}

impl FromStr for Trit {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "X" => Ok(Trit::Undef),
            "0" => Ok(Trit::Zero),
            "1" => Ok(Trit::One),
            _ => Err(()),
        }
    }
}

impl From<bool> for Trit {
    fn from(value: bool) -> Self {
        match value {
            false => Trit::Zero,
            true => Trit::One,
        }
    }
}

impl TryFrom<Net> for Trit {
    type Error = ();

    fn try_from(value: Net) -> Result<Self, Self::Error> {
        if value == Net::UNDEF {
            Ok(Trit::Undef)
        } else if value == Net::ZERO {
            Ok(Trit::Zero)
        } else if value == Net::ONE {
            Ok(Trit::One)
        } else {
            Err(())
        }
    }
}

impl std::ops::Not for Trit {
    type Output = Trit;

    fn not(self) -> Self::Output {
        match self {
            Trit::One => Trit::Zero,
            Trit::Zero => Trit::One,
            Trit::Undef => Trit::Undef,
        }
    }
}

impl std::ops::BitAnd<Trit> for Trit {
    type Output = Trit;

    fn bitand(self, rhs: Trit) -> Self::Output {
        match (self, rhs) {
            (Trit::Zero, _) => Trit::Zero,
            (_, Trit::Zero) => Trit::Zero,
            (Trit::Undef, _) => Trit::Undef,
            (_, Trit::Undef) => Trit::Undef,
            (Trit::One, Trit::One) => Trit::One,
        }
    }
}

impl std::ops::BitOr<Trit> for Trit {
    type Output = Trit;

    fn bitor(self, rhs: Trit) -> Self::Output {
        match (self, rhs) {
            (Trit::One, _) => Trit::One,
            (_, Trit::One) => Trit::One,
            (Trit::Undef, _) => Trit::Undef,
            (_, Trit::Undef) => Trit::Undef,
            (Trit::Zero, Trit::Zero) => Trit::Zero,
        }
    }
}

impl std::ops::BitXor<Trit> for Trit {
    type Output = Trit;

    fn bitxor(self, rhs: Trit) -> Self::Output {
        match (self, rhs) {
            (Trit::Undef, _) => Trit::Undef,
            (_, Trit::Undef) => Trit::Undef,
            (Trit::One, Trit::One) => Trit::Zero,
            (Trit::One, Trit::Zero) => Trit::One,
            (Trit::Zero, Trit::One) => Trit::One,
            (Trit::Zero, Trit::Zero) => Trit::Zero,
        }
    }
}

/// A constant is a (possibly empty) sequence of [`Trit`]s.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Const {
    trits: Vec<Trit>,
}

impl Const {
    /// Creates an empty constant.
    pub const fn new() -> Self {
        Self { trits: vec![] }
    }

    /// Creates an all-`0` constant of given width.
    pub fn zero(width: usize) -> Self {
        Self::from_iter(std::iter::repeat_n(Trit::Zero, width))
    }

    /// Creates an all-`1` constant of given width.
    pub fn ones(width: usize) -> Self {
        Self::from_iter(std::iter::repeat_n(Trit::One, width))
    }

    /// Creates an all-`X` constant of given width.
    pub fn undef(width: usize) -> Self {
        Self::from_iter(std::iter::repeat_n(Trit::Undef, width))
    }

    /// Creates a constant of given width that has a `1` at position `hot_at` and `0` elsewhere.
    pub fn one_hot(width: usize, hot_at: usize) -> Self {
        assert!(hot_at < width);
        Self::from_iter((0..width).map(|index| if index == hot_at { Trit::One } else { Trit::Zero }))
    }

    /// Expands the constant by a single position with the given value.
    pub fn push(&mut self, trit: impl Into<Trit>) {
        self.trits.push(trit.into());
    }

    pub fn from_uint(val: u128, bits: usize) -> Self {
        let mut trits = vec![];
        if bits < 128 {
            assert!(val < (1 << bits));
        }
        for i in 0..bits {
            let trit = if i < u128::BITS as usize && ((val >> i) & 1) != 0 { Trit::One } else { Trit::Zero };
            trits.push(trit);
        }
        Self { trits }
    }

    pub fn lit(value: &str) -> Self {
        value.parse().unwrap()
    }

    pub fn len(&self) -> usize {
        self.trits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.trits.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = Trit> + ExactSizeIterator + '_ {
        self.trits.iter().copied()
    }

    pub fn lsb(&self) -> Trit {
        self.trits[0]
    }

    pub fn msb(&self) -> Trit {
        self.trits[self.len() - 1]
    }

    pub fn is_zero(&self) -> bool {
        self.trits.iter().all(|&trit| trit == Trit::Zero)
    }

    pub fn is_ones(&self) -> bool {
        self.trits.iter().all(|&trit| trit == Trit::One)
    }

    pub fn is_undef(&self) -> bool {
        self.trits.iter().all(|&trit| trit == Trit::Undef)
    }

    pub fn has_undef(&self) -> bool {
        self.trits.iter().any(|&trit| trit == Trit::Undef)
    }

    pub fn as_power_of_two(&self) -> Option<u32> {
        let mut result = None;
        for (offset, trit) in self.trits.iter().copied().enumerate() {
            if trit == Trit::Undef {
                return None;
            }
            if trit == Trit::One {
                if result.is_some() {
                    return None;
                }
                result = Some(offset as u32);
            }
        }
        result
    }

    pub fn concat<'a>(&self, other: impl Into<Cow<'a, Const>>) -> Self {
        Self::from_iter(self.iter().chain(other.into().iter()))
    }

    pub fn repeat(&self, count: usize) -> Self {
        Const::from_iter((0..count).flat_map(|_| self))
    }

    pub fn slice(&self, range: impl std::ops::RangeBounds<usize>) -> Const {
        Const::from_iter(self[(range.start_bound().cloned(), range.end_bound().cloned())].iter().copied())
    }

    pub fn combine<'a, 'b>(lft: impl Into<Cow<'a, Const>>, rgt: impl Into<Cow<'b, Const>>) -> Option<Const> {
        let (lft, rgt) = (lft.into(), rgt.into());
        assert_eq!(lft.len(), rgt.len());
        let mut trits = vec![];
        for (lft, rgt) in std::iter::zip(lft.iter(), rgt.iter()) {
            trits.push(Trit::combine(lft, rgt)?);
        }
        Some(Const { trits })
    }

    pub fn not(&self) -> Const {
        Const::from_iter(self.iter().map(|x| !x))
    }

    pub fn and<'a>(&self, other: impl Into<Cow<'a, Const>>) -> Const {
        let other = other.into();
        assert_eq!(self.len(), other.len());
        Const::from_iter(self.iter().zip(other.iter()).map(|(x, y)| x & y))
    }

    pub fn or<'a>(&self, other: impl Into<Cow<'a, Const>>) -> Const {
        let other = other.into();
        assert_eq!(self.len(), other.len());
        Const::from_iter(self.iter().zip(other.iter()).map(|(x, y)| x | y))
    }

    pub fn xor<'a>(&self, other: impl Into<Cow<'a, Const>>) -> Const {
        let other = other.into();
        assert_eq!(self.len(), other.len());
        Const::from_iter(self.iter().zip(other.iter()).map(|(x, y)| x ^ y))
    }

    pub fn adc<'a>(&self, other: impl Into<Cow<'a, Const>>, ci: Trit) -> Const {
        let other = other.into();
        assert_eq!(self.len(), other.len());
        let mut sum = vec![];
        let mut carry = ci;
        for (a, b) in self.iter().zip(other.iter()) {
            let (y, co) = match (a, b, carry) {
                (Trit::Undef, _, _) => (Trit::Undef, Trit::Undef),
                (_, Trit::Undef, _) => (Trit::Undef, Trit::Undef),
                (_, _, Trit::Undef) => (Trit::Undef, Trit::Undef),
                (Trit::Zero, Trit::Zero, s) => (s, Trit::Zero),
                (Trit::Zero, s, Trit::Zero) => (s, Trit::Zero),
                (s, Trit::Zero, Trit::Zero) => (s, Trit::Zero),
                (Trit::One, Trit::One, s) => (s, Trit::One),
                (Trit::One, s, Trit::One) => (s, Trit::One),
                (s, Trit::One, Trit::One) => (s, Trit::One),
            };
            carry = co;
            sum.push(y);
        }
        sum.push(carry);
        Const::from_iter(sum)
    }

    pub fn eq<'a>(&self, other: impl Into<Cow<'a, Const>>) -> Trit {
        let other = other.into();
        assert_eq!(self.len(), other.len());
        let mut undef = false;
        for (x, y) in self.iter().zip(other.iter()) {
            if x == Trit::Undef || y == Trit::Undef {
                undef = true;
            } else if x != y {
                return Trit::Zero;
            }
        }
        if undef {
            Trit::Undef
        } else {
            Trit::One
        }
    }

    pub fn ult<'a>(&self, other: impl Into<Cow<'a, Const>>) -> Trit {
        let other = other.into();
        assert_eq!(self.len(), other.len());
        if self.has_undef() || other.has_undef() {
            Trit::Undef
        } else {
            for (x, y) in self.iter().zip(other.iter()).rev() {
                if x != y {
                    return Trit::from(x < y);
                }
            }
            Trit::Zero
        }
    }

    pub fn slt<'a>(&self, other: impl Into<Cow<'a, Const>>) -> Trit {
        let other = other.into();
        assert_eq!(self.len(), other.len());
        if self.has_undef() || other.has_undef() {
            Trit::Undef
        } else if self.msb() != other.msb() {
            Trit::from(self.msb() > other.msb())
        } else {
            for (x, y) in self.iter().zip(other.iter()).rev() {
                if x != y {
                    return Trit::from(x < y);
                }
            }
            Trit::Zero
        }
    }

    pub fn mul<'a>(&self, other: impl Into<Cow<'a, Const>>) -> Const {
        let other = other.into();
        assert_eq!(self.len(), other.len());
        if self.has_undef() || other.has_undef() {
            Const::undef(self.len())
        } else {
            let mut res = Const::zero(self.len());
            for (i, bit) in other.iter().enumerate() {
                if bit == Trit::One {
                    res = res.adc(Const::zero(i).concat(self), Trit::Zero);
                } else {
                    res.trits.push(Trit::Zero);
                }
            }
            res.slice(..self.len())
        }
    }
}

impl Debug for Const {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Const::from_iter([")?;
        for (index, trit) in self.trits.iter().enumerate() {
            if index != 0 {
                write!(f, ", ")?;
            }
            write!(f, "{:?}", trit)?;
        }
        write!(f, "])")?;
        Ok(())
    }
}

impl Display for Const {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for trit in self.trits.iter().rev() {
            write!(f, "{}", trit)?;
        }
        Ok(())
    }
}

impl FromStr for Const {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut trits = vec![];
        for char in s.chars().rev() {
            trits.push(Trit::from_char(char)?)
        }
        Ok(Const { trits })
    }
}

impl TryFrom<&Const> for u64 {
    type Error = ();

    fn try_from(value: &Const) -> Result<Self, Self::Error> {
        if value.has_undef() {
            return Err(());
        }
        let mut result = 0;
        for (index, trit) in value.iter().enumerate() {
            if trit == Trit::One {
                if index >= 64 {
                    return Err(());
                }
                result |= 1 << index;
            }
        }
        Ok(result)
    }
}

impl TryFrom<&Const> for i64 {
    type Error = ();

    fn try_from(value: &Const) -> Result<Self, Self::Error> {
        if value.has_undef() {
            return Err(());
        }
        let mut width = value.len();
        while width > 1 && value[width - 1] == value[width - 2] {
            width -= 1;
        }
        if width > Self::BITS as usize {
            return Err(());
        }
        let mut result = 0;
        for (index, trit) in value.iter().enumerate() {
            if trit == Trit::One {
                if index == width - 1 {
                    result |= -1 << index;
                } else {
                    result |= 1 << index;
                }
            }
        }
        Ok(result)
    }
}

impl<I: SliceIndex<[Trit]>> Index<I> for Const {
    type Output = I::Output;

    fn index(&self, index: I) -> &Self::Output {
        &self.trits[index]
    }
}

impl<I: SliceIndex<[Trit]>> IndexMut<I> for Const {
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        &mut self.trits[index]
    }
}

impl Extend<Trit> for Const {
    fn extend<T: IntoIterator<Item = Trit>>(&mut self, iter: T) {
        for trit in iter {
            self.trits.push(trit);
        }
    }
}

impl From<Trit> for Const {
    fn from(trit: Trit) -> Self {
        Const { trits: vec![trit] }
    }
}

impl From<Vec<Trit>> for Const {
    fn from(trits: Vec<Trit>) -> Self {
        Const { trits }
    }
}

impl From<Trit> for Cow<'_, Const> {
    fn from(value: Trit) -> Self {
        Cow::Owned(Const::from(value))
    }
}

impl From<Const> for Cow<'_, Const> {
    fn from(value: Const) -> Self {
        Cow::Owned(value)
    }
}

impl<'a> From<&'a Const> for Cow<'a, Const> {
    fn from(value: &'a Const) -> Self {
        Cow::Borrowed(value)
    }
}

impl FromIterator<Trit> for Const {
    fn from_iter<T: IntoIterator<Item = Trit>>(iter: T) -> Self {
        Const { trits: iter.into_iter().collect() }
    }
}

impl IntoIterator for &Const {
    type Item = Trit;
    type IntoIter = std::vec::IntoIter<Trit>;

    fn into_iter(self) -> Self::IntoIter {
        self.trits.clone().into_iter()
    }
}

impl IntoIterator for Const {
    type Item = Trit;
    type IntoIter = std::vec::IntoIter<Trit>;

    fn into_iter(self) -> Self::IntoIter {
        self.trits.into_iter()
    }
}

#[cfg(test)]
mod test {
    use crate::{Const, Trit};

    #[test]
    fn test_not() {
        for (a, y) in [("", ""), ("01", "10"), ("X10", "X01")] {
            assert_eq!(Const::lit(a).not(), Const::lit(y));
        }
    }

    #[test]
    fn test_and() {
        for (a, b, y) in [("", "", ""), ("1010", "1100", "1000"), ("X0X0", "XX00", "X000"), ("X1X1", "XX11", "XXX1")] {
            assert_eq!(Const::lit(a).and(Const::lit(b)), Const::lit(y));
        }
    }

    #[test]
    fn test_or() {
        for (a, b, y) in [("", "", ""), ("1010", "1100", "1110"), ("X0X0", "XX00", "XXX0"), ("X1X1", "XX11", "X111")] {
            assert_eq!(Const::lit(a).or(Const::lit(b)), Const::lit(y));
        }
    }

    #[test]
    fn test_xor() {
        for (a, b, y) in [("", "", ""), ("1010", "1100", "0110"), ("X0X0", "XX00", "XXX0"), ("X1X1", "XX11", "XXX0")] {
            assert_eq!(Const::lit(a).xor(Const::lit(b)), Const::lit(y));
        }
    }

    #[test]
    fn test_mux() {
        for (s, a, b, y) in [
            ('0', "", "", ""),
            ('0', "XXX101010", "X10XX1100", "X10XX1100"),
            ('1', "XXX101010", "X10XX1100", "XXX101010"),
            ('X', "XXX101010", "X10XX1100", "XXXXX1XX0"),
        ] {
            assert_eq!(Trit::lit(s).mux(Const::lit(a), Const::lit(b)), Const::lit(y));
        }
    }

    #[test]
    fn test_adc() {
        for (a, b, c, y) in [
            ("", "", '0', "0"),
            ("", "", '1', "1"),
            ("10101010", "11001100", '0', "101110110"),
            ("1101", "1111", '1', "11101"),
            ("1010X010", "11001100", '0', "XXXXXX110"),
        ] {
            assert_eq!(Const::lit(a).adc(Const::lit(b), Trit::lit(c)), Const::lit(y));
        }
    }

    #[test]
    fn test_mul() {
        for (a, b, y) in [("", "", ""), ("0011", "0011", "1001"), ("0X11", "0011", "XXXX"), ("1101", "1101", "1001")] {
            assert_eq!(Const::lit(a).mul(Const::lit(b)), Const::lit(y));
        }
    }
}
