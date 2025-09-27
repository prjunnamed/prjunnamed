use prjunnamed_netlist::{Cell, Const, FlipFlop, Net, Value};

use crate::{DesignDyn, NetOrValue, Pattern};

macro_rules! bitwise_patterns {
    {} => {};

    { $name:ident(_) => $cstr:ident; $($rest:tt)* } => {
        pub struct $name<P>(P);

        impl<P> $name<P> {
            pub fn new(pat: P) -> $name<P> {
                $name(pat)
            }
        }

        impl<T: NetOrValue, P: Pattern<T>> Pattern<T> for $name<P> {
            type Capture = (T, P::Capture);

            #[inline]
            fn execute(&self, design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
                let mut cap = None;
                for net in target.iter() {
                    if let Ok((cell_ref, offset)) = design.find_cell(net) {
                        if let Cell::$cstr(arg) = &*cell_ref.get() {
                            assert!(T::accumulate(&mut cap, arg[offset]));
                        } else {
                            return None
                        }
                    } else {
                        return None
                    }
                }
                self.0.execute(design, &cap.unwrap()).and_then(|cap|
                    Some((target.clone(), cap)))
            }
        }

        bitwise_patterns!{ $($rest)* }
    };

    { $name:ident(_,_) => $cstr:ident; $($rest:tt)* } => {
        pub struct $name<P1, P2>(P1, P2);

        impl<P1, P2> $name<P1, P2> {
            pub fn new(pat1: P1, pat2: P2) -> $name<P1, P2> {
                $name(pat1, pat2)
            }
        }

        impl<T: NetOrValue, P1: Pattern<T>, P2: Pattern<T>> Pattern<T> for $name<P1, P2> {
            type Capture = (T, P1::Capture, P2::Capture);

            #[inline]
            fn execute(&self, design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
                let mut cap1 = None;
                let mut cap2 = None;
                for net in target.iter() {
                    if let Ok((cell_ref, offset)) = design.find_cell(net) {
                        if let Cell::$cstr(arg1, arg2) = &*cell_ref.get() {
                            assert!(T::accumulate(&mut cap1, arg1[offset]));
                            assert!(T::accumulate(&mut cap2, arg2[offset]));
                        } else {
                            return None
                        }
                    } else {
                        return None
                    }
                }
                self.0.execute(design, &cap1.unwrap()).and_then(|cap1|
                    self.1.execute(design, &cap2.unwrap()).and_then(|cap2|
                        Some((target.clone(), cap1, cap2))))
            }
        }

        bitwise_patterns!{ $($rest)* }
    }
}

bitwise_patterns! {
    PBuf(_) => Buf;
    PNot(_) => Not;

    PAnd(_, _) => And;
    POr(_, _) => Or;
    PXor(_, _) => Xor;
}

pub struct PMux<P1, P2, P3>(P1, P2, P3);

impl<P1, P2, P3> PMux<P1, P2, P3> {
    pub fn new(pat1: P1, pat2: P2, pat3: P3) -> PMux<P1, P2, P3> {
        PMux(pat1, pat2, pat3)
    }
}

impl<T: NetOrValue, P1: Pattern<Net>, P2: Pattern<T>, P3: Pattern<T>> Pattern<T> for PMux<P1, P2, P3> {
    type Capture = (T, P1::Capture, P2::Capture, P3::Capture);

    #[inline]
    fn execute(&self, design: &dyn DesignDyn, target: &T) -> Option<Self::Capture> {
        let mut cap1 = None;
        let mut cap2 = None;
        let mut cap3 = None;
        for net in target.iter() {
            if let Ok((cell_ref, offset)) = design.find_cell(net) {
                if let Cell::Mux(arg1, arg2, arg3) = &*cell_ref.get() {
                    if !NetOrValue::accumulate(&mut cap1, *arg1) {
                        return None;
                    }
                    assert!(T::accumulate(&mut cap2, arg2[offset]));
                    assert!(T::accumulate(&mut cap3, arg3[offset]));
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        self.0.execute(design, &cap1.unwrap()).and_then(|cap1| {
            self.1.execute(design, &cap2.unwrap()).and_then(|cap2| {
                self.2.execute(design, &cap3.unwrap()).and_then(|cap3| Some((target.clone(), cap1, cap2, cap3)))
            })
        })
    }
}

pub struct PDff<P>(P);

impl<P> PDff<P> {
    pub fn new(pat: P) -> PDff<P> {
        PDff(pat)
    }
}

impl<P: Pattern<Net>> Pattern<Net> for PDff<P> {
    type Capture = (FlipFlop, P::Capture);

    #[inline]
    fn execute(&self, design: &dyn DesignDyn, target: &Net) -> Option<Self::Capture> {
        if let Ok((cell_ref, offset)) = design.find_cell(*target) {
            if let Cell::Dff(flip_flop) = &*cell_ref.get() {
                let flip_flop = FlipFlop {
                    data: flip_flop.data[offset].into(),
                    load_data: flip_flop.load_data[offset].into(),
                    clear_value: flip_flop.clear_value[offset].into(),
                    reset_value: flip_flop.reset_value[offset].into(),
                    init_value: flip_flop.init_value[offset].into(),
                    ..*flip_flop
                };
                return self.0.execute(design, &flip_flop.data[0]).and_then(|capture| Some((flip_flop, capture)));
            }
        }
        None
    }
}

impl<P: Pattern<Value>> Pattern<Value> for PDff<P> {
    type Capture = (FlipFlop, P::Capture);

    #[inline]
    fn execute(&self, design: &dyn DesignDyn, target: &Value) -> Option<Self::Capture> {
        let mut target_flip_flop = None::<FlipFlop>;
        for target_net in target.iter() {
            if let Ok((cell_ref, offset)) = design.find_cell(target_net) {
                if let Cell::Dff(flip_flop) = &*cell_ref.get() {
                    if let Some(ref mut capture) = target_flip_flop {
                        if capture.clock.canonicalize() == flip_flop.clock.canonicalize()
                            && capture.clear.canonicalize() == flip_flop.clear.canonicalize()
                            && capture.reset.canonicalize() == flip_flop.reset.canonicalize()
                            && capture.enable.canonicalize() == flip_flop.enable.canonicalize()
                            && capture.reset_over_enable == flip_flop.reset_over_enable
                        {
                            capture.data.extend(Value::from(flip_flop.data[offset]));
                            capture.clear_value.extend(Const::from(flip_flop.clear_value[offset]));
                            capture.reset_value.extend(Const::from(flip_flop.reset_value[offset]));
                            capture.init_value.extend(Const::from(flip_flop.init_value[offset]));
                        }
                    } else {
                        target_flip_flop = Some(FlipFlop {
                            data: flip_flop.data[offset].into(),
                            load_data: flip_flop.load_data[offset].into(),
                            clear_value: flip_flop.clear_value[offset].into(),
                            reset_value: flip_flop.reset_value[offset].into(),
                            init_value: flip_flop.init_value[offset].into(),
                            ..*flip_flop
                        })
                    }
                }
            }
        }
        target_flip_flop.and_then(|target_flip_flop| {
            self.0.execute(design, &target_flip_flop.data).and_then(|capture| Some((target_flip_flop, capture)))
        })
    }
}
