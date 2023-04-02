use smallvec::SmallVec;

/// A single bit value.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Bit {
    _0,
    _1,
    X,
}

/// A bitvec value.  Bits are indexed starting from LSB.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Bits {
    pub bits: SmallVec<[Bit; 16]>,
}

impl std::fmt::Display for Bits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::fmt::Write;
        write!(f, "{n}'b", n = self.bits.len())?;
        for b in self.bits.iter().rev() {
            f.write_char(match b {
                Bit::_0 => '0',
                Bit::_1 => '1',
                Bit::X => 'x',
            })?;
        }
        Ok(())
    }
}

impl Bits {
    pub fn width(&self) -> u32 {
        self.bits.len() as u32
    }
}
