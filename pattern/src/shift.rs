use prjunnamed_netlist::{Cell, Value};

use crate::{DesignDyn, Pattern};

macro_rules! shift_patterns {
    {} => {};

    { $name:ident(_,_,_) => $cstr:ident; $($rest:tt)* } => {
        pub struct $name<P1, P2, P3>(P1, P2, P3);

        impl<P1, P2, P3> $name<P1, P2, P3> {
            pub fn new(pat1: P1, pat2: P2, pat3: P3) -> $name<P1, P2, P3> {
                $name(pat1, pat2, pat3)
            }
        }

        impl<P1: Pattern<Value>, P2: Pattern<Value>, P3: Pattern<u32>> Pattern<Value> for $name<P1, P2, P3> {
            type Capture = (Value, P1::Capture, P2::Capture, P3::Capture);

            #[inline]
            fn execute(&self, design: &dyn DesignDyn, target: &Value) -> Option<Self::Capture> {
                if target.is_empty() {
                    return None;
                }
                let (cap1, cap2, cap3);
                if let Ok((cell_ref, _offset)) = design.find_cell(target.iter().next().unwrap()) {
                    if let Cell::$cstr(arg1, arg2, arg3) = &*cell_ref.get() {
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
                self.0.execute(design, &cap1).and_then(|cap1|
                    self.1.execute(design, &cap2).and_then(|cap2|
                        self.2.execute(design, &cap3).and_then(|cap3|
                            Some((target.clone(), cap1, cap2, cap3)))))
            }
        }

        shift_patterns!{ $($rest)* }
    };
}

shift_patterns! {
    PShl(_,_,_) => Shl;
    PUShr(_,_,_) => UShr;
    PSShr(_,_,_) => SShr;
    PXShr(_,_,_) => XShr;
}
