use crate::model::{
    bits::Bits,
    cells::{self, CellKind, CellValSlot},
    float::F64BitEq,
    CellId, CellPlane, CellRef, CellRefMut, CellType, ModuleRefMut, StrId,
};
use delegate::delegate;

impl ModuleRefMut<'_> {
    pub fn add_cell(&mut self, contents: impl Into<CellKind>) -> CellId {
        let mut cell = self.add_void();
        cell.set_contents(contents);
        cell.id()
    }
}

macro_rules! cell_getter_copy {
    ($name:ident, $typ:ident) => {
        impl CellRef<'_> {
            pub fn $name(&self) -> Option<cells::$typ> {
                if let &CellKind::$typ(x) = self.contents() {
                    Some(x)
                } else {
                    None
                }
            }
        }

        impl CellRefMut<'_> {
            pub fn $name(&self) -> Option<cells::$typ> {
                self.as_ref().$name()
            }
        }
    };
}

macro_rules! cell_getter_ref {
    ($name:ident, $typ:ident) => {
        impl<'a> CellRef<'a> {
            pub fn $name(self) -> Option<&'a cells::$typ> {
                if let CellKind::$typ(x) = self.contents() {
                    Some(x)
                } else {
                    None
                }
            }
        }
        impl CellRefMut<'_> {
            pub fn $name(&self) -> Option<&cells::$typ> {
                self.as_ref().$name()
            }
        }
    };
}

cell_getter_copy!(get_param, Param);
cell_getter_copy!(get_port_in, PortIn);
cell_getter_copy!(get_port_out, PortOut);
cell_getter_copy!(get_port_bus, PortBus);
cell_getter_copy!(get_slice, Slice);
cell_getter_copy!(get_ext, Ext);
cell_getter_copy!(get_buf, Buf);
cell_getter_copy!(get_bitop, BitOp);
cell_getter_copy!(get_unary_xor, UnaryXor);
cell_getter_copy!(get_cmp, Cmp);
cell_getter_copy!(get_addsub, AddSub);
cell_getter_copy!(get_mul, Mul);
cell_getter_copy!(get_shift, Shift);
cell_getter_copy!(get_instout, InstanceOutput);
cell_getter_copy!(get_bus, Bus);
cell_getter_copy!(get_bus_joiner, BusJoiner);
cell_getter_copy!(get_bus_driver, BusDriver);
cell_getter_copy!(get_blackbox_buf, BlackboxBuf);
cell_getter_ref!(get_swizzle, Swizzle);
cell_getter_ref!(get_bus_swizzle, BusSwizzle);
cell_getter_ref!(get_mux, Mux);
cell_getter_ref!(get_switch, Switch);
cell_getter_ref!(get_register, Register);
cell_getter_ref!(get_instance, Instance);
cell_getter_ref!(get_uinstance, UnresolvedInstance);
cell_getter_ref!(get_wire, Wire);

impl<'a> CellRef<'a> {
    pub fn is_const(&self) -> bool {
        matches!(
            self.contents(),
            CellKind::ConstBits(_)
                | CellKind::ConstInt(_)
                | CellKind::ConstFloat(_)
                | CellKind::ConstString(_)
        )
    }

    pub fn is_swizzle(&self) -> bool {
        matches!(
            self.contents(),
            CellKind::Swizzle(_) | CellKind::Slice(_) | CellKind::Ext(_)
        )
    }

    pub fn is_comb(&self) -> bool {
        matches!(
            self.contents(),
            CellKind::Buf(_)
                | CellKind::BitOp(_)
                | CellKind::UnaryXor(_)
                | CellKind::Mux(_)
                | CellKind::Switch(_)
                | CellKind::Cmp(_)
                | CellKind::AddSub(_)
                | CellKind::Mul(_)
                | CellKind::Shift(_)
        )
    }

    pub fn typ(&self) -> CellType {
        match self.contents() {
            CellKind::Void => CellType::Void,
            CellKind::Param(p) => match p.typ {
                cells::ParamType::BitVec(w) => CellType::BitVec(w, false),
                cells::ParamType::BitVecAny => CellType::BitVecAny,
                cells::ParamType::String => CellType::String,
                cells::ParamType::Int => CellType::Int,
                cells::ParamType::Float => CellType::Float,
            },
            CellKind::PortIn(p) => {
                if let Some(width) = p.width {
                    CellType::BitVec(width, false)
                } else {
                    CellType::BitVecAny
                }
            }
            CellKind::PortBus(p) => {
                if let Some(width) = p.width {
                    CellType::BitVec(width, true)
                } else {
                    CellType::BitVecAny
                }
            }
            CellKind::PortOut(p) => {
                if let Some(width) = p.width {
                    CellType::Out(width)
                } else {
                    CellType::OutAny
                }
            }
            CellKind::ConstBits(val) => CellType::BitVec(val.width(), false),
            CellKind::ConstInt(_) => CellType::Int,
            CellKind::ConstFloat(_) => CellType::Float,
            CellKind::ConstString(_) => CellType::String,
            CellKind::Swizzle(s) => CellType::BitVec(s.width, false),
            CellKind::BusSwizzle(s) => CellType::BitVec(s.width, true),
            CellKind::Slice(s) => CellType::BitVec(s.width, false),
            CellKind::Ext(e) => CellType::BitVec(e.width, false),
            CellKind::Buf(b) => CellType::BitVec(b.width, false),
            CellKind::BitOp(b) => CellType::BitVec(b.width, false),
            CellKind::UnaryXor(_) => CellType::BitVec(1, false),
            CellKind::Mux(m) => CellType::BitVec(m.width, false),
            CellKind::Switch(s) => CellType::BitVec(s.width, false),
            CellKind::Cmp(_) => CellType::BitVec(1, false),
            CellKind::AddSub(v) => CellType::BitVec(v.width, false),
            CellKind::Mul(m) => CellType::BitVec(m.width, false),
            CellKind::Shift(s) => CellType::BitVec(s.width, false),
            CellKind::Register(r) => CellType::BitVec(r.width, false),
            CellKind::Instance(_) => CellType::Void,
            CellKind::UnresolvedInstance(_) => CellType::Void,
            CellKind::InstanceOutput(o) => CellType::BitVec(o.width, false),
            CellKind::Bus(b) => CellType::BitVec(b.width, true),
            CellKind::BusJoiner(_) => CellType::Void,
            CellKind::BusDriver(_) => CellType::Void,
            CellKind::BlackboxBuf(b) => CellType::BitVec(b.width, false),
            CellKind::Wire(_) => CellType::Void,
        }
    }

    /// Determines which plane a cell is on.
    pub fn plane(self) -> CellPlane {
        if self.is_const() || matches!(self.contents(), CellKind::Param(_)) {
            CellPlane::Param
        } else if matches!(self.contents(), CellKind::Wire(_)) {
            CellPlane::Debug
        } else {
            self.flags_plane()
        }
    }

    pub fn get_const_int(&self) -> Option<i32> {
        if let &CellKind::ConstInt(v) = self.contents() {
            Some(v)
        } else {
            None
        }
    }

    pub fn get_const_float(&self) -> Option<F64BitEq> {
        if let &CellKind::ConstFloat(v) = self.contents() {
            Some(v)
        } else {
            None
        }
    }

    pub fn get_const_str(&self) -> Option<StrId> {
        if let &CellKind::ConstString(v) = self.contents() {
            Some(v)
        } else {
            None
        }
    }

    pub fn for_each_val(self, f: impl FnMut(CellId, CellValSlot)) {
        self.contents().for_each_val(f);
    }

    pub fn get_const_bits(self) -> Option<&'a Bits> {
        if let CellKind::ConstBits(b) = self.contents() {
            Some(b)
        } else {
            None
        }
    }
}

impl CellRefMut<'_> {
    delegate! {
        to self.as_ref() {
            pub fn get_const_bits(&self) -> Option<&Bits>;
            pub fn is_const(&self) -> bool;
            pub fn is_swizzle(&self) -> bool;
            pub fn is_comb(&self) -> bool;
            pub fn typ(self) -> CellType;
            pub fn plane(self) -> CellPlane;
            pub fn get_const_int(&self) -> Option<i32>;
            pub fn get_const_float(&self) -> Option<F64BitEq>;
            pub fn get_const_str(&self) -> Option<StrId>;
            pub fn for_each_val(self, f: impl FnMut(CellId, CellValSlot));
        }
    }

    pub fn remove(&mut self) {
        self.set_contents(CellKind::Void);
        self.set_annnotations(vec![]);
        self.clear_flags();
    }

    pub fn replace_uses_with_if(&mut self, val: CellId, mut f: impl FnMut(CellValSlot) -> bool) {
        if val == self.id() {
            return;
        }
        let uses: Vec<_> = self.uses().filter(|&(_, slot)| f(slot)).collect();
        for (cid, slot) in uses {
            self.sibling_mut(cid).replace_val(slot, val);
        }
    }

    pub fn replace_uses_with(&mut self, val: CellId) {
        self.replace_uses_with_if(val, |_| true);
    }

    pub fn replace_plain_uses_with(&mut self, val: CellId) {
        self.replace_uses_with_if(val, CellValSlot::is_plain);
    }
}
