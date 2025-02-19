use prjunnamed_netlist::{Cell, Net, Value};

use crate::{DesignDyn, NetOrValue, Pattern};

pub struct PAdc<P1, P2, P3>(P1, P2, P3);

impl<P1, P2, P3> PAdc<P1, P2, P3> {
    pub fn new(pat1: P1, pat2: P2, pat3: P3) -> PAdc<P1, P2, P3> {
        PAdc(pat1, pat2, pat3)
    }
}

impl<P1: Pattern<Value>, P2: Pattern<Value>, P3: Pattern<Net>> Pattern<Value> for PAdc<P1, P2, P3> {
    type Capture = (Value, P1::Capture, P2::Capture, P3::Capture);

    #[inline]
    fn execute(&self, design: &dyn DesignDyn, target: &Value) -> Option<Self::Capture> {
        if target.is_empty() {
            return None;
        }
        let (cap1, cap2, cap3);
        if let Ok((cell_ref, _offset)) = design.find_cell(target.iter().next().unwrap()) {
            if let Cell::Adc(arg1, arg2, arg3) = &*cell_ref.get() {
                if *target == cell_ref.output() {
                    cap1 = arg1.clone();
                    cap2 = arg2.clone();
                    cap3 = *arg3;
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            return None;
        }
        self.0.execute(design, &cap1).and_then(|cap1| {
            self.1.execute(design, &cap2).and_then(|cap2| {
                self.2.execute(design, &cap3).and_then(|cap3| Some((target.clone(), cap1, cap2, cap3)))
            })
        })
    }
}

macro_rules! compare_patterns {
    {} => {};

    { $name:ident(_,_) => $cstr:ident; $($rest:tt)* } => {
        pub struct $name<P1, P2>(P1, P2);

        impl<P1, P2> $name<P1, P2> {
            pub fn new(pat1: P1, pat2: P2) -> $name<P1, P2> {
                $name(pat1, pat2)
            }
        }

        impl<T: NetOrValue, P1: Pattern<Value>, P2: Pattern<Value>> Pattern<T> for $name<P1, P2> {
            type Capture = (Net, P1::Capture, P2::Capture);

            #[inline]
            fn execute(&self, design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
                if target.len() != 1 { return None }
                let target = target.iter().next().unwrap();
                let (cap1, cap2);
                if let Ok((cell_ref, 0)) = design.find_cell(target) {
                    if let Cell::$cstr(arg1, arg2) = &*cell_ref.get() {
                        cap1 = arg1.clone();
                        cap2 = arg2.clone();
                    } else {
                        return None
                    }
                } else {
                    return None
                }
                self.0.execute(design, &cap1).and_then(|cap1|
                    self.1.execute(design, &cap2).and_then(|cap2|
                        Some((target, cap1, cap2))))
            }
        }

        compare_patterns!{ $($rest)* }
    }
}

macro_rules! arithmetic_patterns {
    {} => {};

    { $name:ident(_,_) => $cstr:ident; $($rest:tt)* } => {
        pub struct $name<P1, P2>(P1, P2);

        impl<P1, P2> $name<P1, P2> {
            pub fn new(pat1: P1, pat2: P2) -> $name<P1, P2> {
                $name(pat1, pat2)
            }
        }

        impl<P1: Pattern<Value>, P2: Pattern<Value>> Pattern<Value> for $name<P1, P2> {
            type Capture = (Value, P1::Capture, P2::Capture);

            #[inline]
            fn execute(&self, design: &dyn DesignDyn, target: &Value) -> Option<Self::Capture> {
                if target.is_empty() { return None }
                let (cap1, cap2);
                if let Ok((cell_ref, 0)) = design.find_cell(target.iter().next().unwrap()) {
                    if let Cell::$cstr(arg1, arg2) = &*cell_ref.get() {
                        if *target == cell_ref.output() {
                            cap1 = arg1.clone();
                            cap2 = arg2.clone();
                        } else {
                            return None
                        }
                    } else {
                        return None
                    }
                } else {
                    return None
                }
                self.0.execute(design, &cap1).and_then(|cap1|
                    self.1.execute(design, &cap2).and_then(|cap2|
                        Some((target.clone(), cap1, cap2))))
            }
        }

        arithmetic_patterns!{ $($rest)* }
    }
}

compare_patterns! {
    PEq(_,_) => Eq;
    PULt(_,_) => ULt;
    PSLt(_,_) => SLt;
}

arithmetic_patterns! {
    PMul(_,_) => Mul;
    PUDiv(_,_) => UDiv;
    PUMod(_,_) => UMod;
    PSDivTrunc(_,_) => SDivTrunc;
    PSDivFloor(_,_) => SDivFloor;
    PSModTrunc(_,_) => SModTrunc;
    PSModFloor(_,_) => SModFloor;
}
