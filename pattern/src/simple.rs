use prjunnamed_netlist::{Cell, Const, Net, Trit, Value};

use crate::{DesignDyn, NetOrValue, Pattern};

/// Matches anything. Captures the value being matched.
pub struct PAny;

impl PAny {
    pub fn new() -> PAny {
        PAny
    }
}

impl<T: Clone> Pattern<T> for PAny {
    type Capture = (T,);

    fn execute(&self, _design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
        Some((target.clone(),))
    }
}

/// Captures the value being matched, even if the child pattern captures
/// something else.
///
/// For example, `[PBind@v [PPow2@e]]`.
pub struct PBind<P>(P);

impl<P> PBind<P> {
    pub fn new(pat: P) -> PBind<P> {
        PBind(pat)
    }
}

impl<T: Clone, P: Pattern<T>> Pattern<T> for PBind<P> {
    type Capture = (T, P::Capture);

    fn execute(&self, design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
        self.0.execute(design, target).and_then(|capture| Some((target.clone(), capture)))
    }
}

/// Matches `Net`s and `Value`s that are constants.
pub struct PConst;

impl PConst {
    pub fn new() -> PConst {
        PConst
    }
}

impl Pattern<Net> for PConst {
    type Capture = (Trit,);

    fn execute(&self, _design: &dyn DesignDyn, target: &Net) -> Option<Self::Capture> {
        Net::as_const(*target).map(|value| (value,))
    }
}

impl Pattern<Value> for PConst {
    type Capture = (Const,);

    fn execute(&self, _design: &dyn DesignDyn, target: &Value) -> Option<Self::Capture> {
        Value::as_const(&target).map(|value| (value,))
    }
}

pub struct PZero;

impl PZero {
    pub fn new() -> PZero {
        PZero
    }
}

impl Pattern<u32> for PZero {
    type Capture = ((),);

    fn execute(&self, _design: &dyn DesignDyn, target: &u32) -> Option<Self::Capture> {
        if *target == 0 {
            Some(((),))
        } else {
            None
        }
    }
}

impl<T: NetOrValue> Pattern<T> for PZero {
    type Capture = ((),);

    fn execute(&self, _design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
        match target.as_const() {
            Some(value) if value.iter().all(|net| net == Trit::Zero) => Some(((),)),
            _ => None,
        }
    }
}

pub struct POnes;

impl POnes {
    pub fn new() -> POnes {
        POnes
    }
}

impl<T: NetOrValue> Pattern<T> for POnes {
    type Capture = ((),);

    fn execute(&self, _design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
        match target.as_const() {
            Some(value) if value.is_empty() => None,
            Some(value) if value.iter().all(|net| net == Trit::One) => Some(((),)),
            _ => None,
        }
    }
}

pub struct PUndef;

impl PUndef {
    pub fn new() -> PUndef {
        PUndef
    }
}

impl<T: NetOrValue> Pattern<T> for PUndef {
    type Capture = ((),);

    fn execute(&self, _design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
        match target.as_const() {
            Some(value) if value.is_empty() => None,
            Some(value) if value.iter().all(|net| net == Trit::Undef) => Some(((),)),
            _ => None,
        }
    }
}

pub struct PHasX;

impl PHasX {
    pub fn new() -> PHasX {
        PHasX
    }
}

impl<T: NetOrValue> Pattern<T> for PHasX {
    type Capture = ((),);

    fn execute(&self, _design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
        match target.as_const() {
            Some(value) if value.has_undef() => Some(((),)),
            _ => None,
        }
    }
}

/// Matches constants that are powers of two (have exactly one bit set).
/// Captures the exponent, i.e. log2 of the value.
pub struct PPow2;

impl PPow2 {
    pub fn new() -> PPow2 {
        PPow2
    }
}

impl<T: NetOrValue> Pattern<T> for PPow2 {
    type Capture = (u32,);

    fn execute(&self, _design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
        target.as_const().and_then(|value| value.as_power_of_two().map(|exp| (exp,)))
    }
}

pub type POneHot = PPow2;

pub struct PInput(&'static str);

impl PInput {
    pub fn new(name: &'static str) -> PInput {
        PInput(name)
    }
}

impl<T: NetOrValue> Pattern<T> for PInput {
    type Capture = (T,);

    fn execute(&self, design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
        if let Some(net) = target.iter().next() {
            if let Ok((cell_ref, 0)) = design.find_cell(net) {
                if let Cell::Input(name, _size) = &*cell_ref.get() {
                    if target.as_value() == cell_ref.output() && name == self.0 {
                        return Some((target.clone(),));
                    }
                }
            }
        }
        None
    }
}

pub struct PZExt<P>(P);

impl<P> PZExt<P> {
    pub fn new(pat: P) -> PZExt<P> {
        PZExt(pat)
    }
}

impl<P: Pattern<Value>> Pattern<Value> for PZExt<P> {
    // The amount of extension bits can be found using: [PZExt@y [PAny@a]] => y.len() - a.len()
    type Capture = (Value, P::Capture);

    fn execute(&self, design: &dyn DesignDyn, target: &Value) -> Option<Self::Capture> {
        let zext_count = target.iter().rev().take_while(|net| *net == Net::ZERO).count();
        self.0.execute(design, &target.slice(..target.len() - zext_count)).map(|capture| (target.clone(), capture))
    }
}

pub struct PSExt<P>(P);

impl<P> PSExt<P> {
    pub fn new(pat: P) -> PSExt<P> {
        PSExt(pat)
    }
}

impl<P: Pattern<Value>> Pattern<Value> for PSExt<P> {
    // The amount of extension bits can be found using: [PZExt@y [PAny@a]] => y.len() - a.len()
    type Capture = (Value, P::Capture);

    fn execute(&self, design: &dyn DesignDyn, target: &Value) -> Option<Self::Capture> {
        if target.is_empty() {
            return None;
        }
        let sext_count = target.iter().rev().take_while(|net| *net == target.msb()).count() - 1;
        self.0.execute(design, &target.slice(..target.len() - sext_count)).map(|capture| (target.clone(), capture))
    }
}
