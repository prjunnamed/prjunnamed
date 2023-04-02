use std::fmt::{Debug, Display};

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct F64BitEq(pub f64);

impl PartialEq for F64BitEq {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for F64BitEq {}

impl PartialOrd for F64BitEq {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for F64BitEq {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.to_bits().cmp(&other.0.to_bits())
    }
}

impl core::hash::Hash for F64BitEq {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl Debug for F64BitEq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:#}")
    }
}

impl Display for F64BitEq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "f64'h{b:016x}", b = self.0.to_bits())
        } else if self.0 == 0.0 && self.0.is_sign_negative() {
            // https://github.com/Alexhuszagh/rust-lexical/issues/94
            write!(f, "-0.0")
        } else {
            let s = lexical::to_string(self.0);
            write!(f, "{s}")
        }
    }
}
