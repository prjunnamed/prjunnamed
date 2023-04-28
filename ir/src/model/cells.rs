use prjunnamed_entity::EntityVec;
use smallvec::SmallVec;

use super::{
    annotations::HierName, bits::Bits, float::F64BitEq, CellId, ModuleId, ParamId, PortBusId,
    PortInId, PortOutId, StrId,
};

#[cfg(doc)]
use super::{annotations::CellAnnotation, ModuleRef};

/// The main contents of a cell.
#[derive(Debug, Clone, Default)]
pub enum CellKind {
    #[default]
    /// A cell that does nothing, aka a tombstone.  It has two purposes:
    ///
    /// - is left behind in place of removed cells (to be removed later by the GC pass)
    /// - is used as a placeholder for not-yet-filled cells when creating a cyclic structure
    ///
    /// No flags nor annotations are valid on this cell.  It has no value and it is not valid to reference it.
    Void,
    Param(Param),
    PortIn(PortIn),
    PortOut(PortOut),
    PortBus(PortBus),
    /// A constant of bitvec type.  No flags nor annotations are valid on this cell.  Always belongs to the constant plane.
    ConstBits(Bits),
    /// A constant of integer type.  No flags nor annotations are valid on this cell.  Always belongs to the constant plane.
    ConstInt(i32),
    /// A constant of float type.  No flags nor annotations are valid on this cell.  Always belongs to the constant plane.
    ConstFloat(F64BitEq),
    /// A constant of string type.  No flags nor annotations are valid on this cell.  Always belongs to the constant plane.
    ConstString(StrId),
    Swizzle(Swizzle),
    BusSwizzle(BusSwizzle),
    Slice(Slice),
    Ext(Ext),
    Buf(Buf),
    BitOp(BitOp),
    UnaryXor(UnaryXor),
    Mux(Mux),
    Switch(Switch),
    Cmp(Cmp),
    AddSub(AddSub),
    Mul(Mul),
    Shift(Shift),
    // XXX rotate
    // XXX bitscan
    // XXX popcnt
    // XXX demux
    // XXX special fine cells?
    Register(Register),
    // XXX memory
    Instance(Instance),
    UnresolvedInstance(UnresolvedInstance),
    InstanceOutput(InstanceOutput),
    Bus(Bus),
    BusJoiner(BusJoiner),
    BusDriver(BusDriver),
    // XXX clockgate
    BlackboxBuf(BlackboxBuf),
    Wire(Wire),
}

macro_rules! impl_from_typ {
    ($kind: ident, $typ: ty) => {
        impl From<$typ> for CellKind {
            fn from(val: $typ) -> CellKind {
                CellKind::$kind(val)
            }
        }
    };
}

macro_rules! impl_from {
    ($kind: ident) => {
        impl_from_typ!($kind, $kind);
    };
}

impl_from!(Param);
impl_from!(PortIn);
impl_from!(PortOut);
impl_from!(PortBus);
impl_from_typ!(ConstBits, Bits);
impl_from_typ!(ConstInt, i32);
impl_from_typ!(ConstFloat, F64BitEq);
impl_from_typ!(ConstString, StrId);
impl_from!(Swizzle);
impl_from!(BusSwizzle);
impl_from!(Slice);
impl_from!(Ext);
impl_from!(Buf);
impl_from!(BitOp);
impl_from!(UnaryXor);
impl_from!(Mux);
impl_from!(Switch);
impl_from!(Cmp);
impl_from!(AddSub);
impl_from!(Mul);
impl_from!(Shift);
impl_from!(Register);
impl_from!(Instance);
impl_from!(UnresolvedInstance);
impl_from!(InstanceOutput);
impl_from!(Bus);
impl_from!(BusJoiner);
impl_from!(BusDriver);
impl_from!(BlackboxBuf);
impl_from!(Wire);

impl CellKind {
    pub fn for_each_val(&self, mut f: impl FnMut(CellId, CellValSlot)) {
        match self {
            CellKind::Void => (),
            CellKind::Param(_) => (),
            CellKind::PortIn(_) => (),
            CellKind::PortOut(port) => {
                if let Some(v) = port.val {
                    f(v, CellValSlot::PortOut);
                }
            }
            CellKind::PortBus(_) => (),
            CellKind::ConstBits(_) => (),
            CellKind::ConstInt(_) => (),
            CellKind::ConstFloat(_) => (),
            CellKind::ConstString(_) => (),
            CellKind::Swizzle(swizzle) => {
                for (i, chunk) in swizzle.chunks.iter().enumerate() {
                    match *chunk {
                        SwizzleChunk::Const(_) => (),
                        SwizzleChunk::Value { val, .. } => f(val, CellValSlot::SwizzleChunk(i)),
                    }
                }
            }
            CellKind::BusSwizzle(sw) => {
                for (i, chunk) in sw.chunks.iter().enumerate() {
                    f(chunk.val, CellValSlot::BusSwizzleChunk(i));
                }
            }
            CellKind::Slice(s) => f(s.val, CellValSlot::Slice),
            CellKind::Ext(e) => f(e.val, CellValSlot::Ext),
            CellKind::Buf(b) => f(b.val, CellValSlot::Buf),
            CellKind::BitOp(b) => {
                f(b.val_a, CellValSlot::BitOpA);
                f(b.val_b, CellValSlot::BitOpB);
            }
            CellKind::UnaryXor(v) => {
                f(v.val, CellValSlot::UnaryXor);
            }
            CellKind::Mux(v) => {
                f(v.val_sel, CellValSlot::MuxSel);
                for (i, &val) in v.vals.iter().enumerate() {
                    f(val, CellValSlot::MuxInput(i));
                }
            }
            CellKind::Switch(v) => {
                f(v.val_sel, CellValSlot::SwitchSel);
                for (i, case) in v.cases.iter().enumerate() {
                    f(case.val, CellValSlot::SwitchInput(i));
                }
                f(v.default, CellValSlot::SwitchDefault);
            }
            CellKind::Cmp(v) => {
                f(v.val_a, CellValSlot::CmpA);
                f(v.val_b, CellValSlot::CmpB);
            }
            CellKind::AddSub(v) => {
                f(v.val_a, CellValSlot::AddSubA);
                f(v.val_b, CellValSlot::AddSubB);
                f(v.val_inv, CellValSlot::AddSubInv);
                f(v.val_carry, CellValSlot::AddSubCarry);
            }
            CellKind::Mul(v) => {
                f(v.val_a, CellValSlot::MulA);
                f(v.val_b, CellValSlot::MulB);
            }
            CellKind::Shift(s) => {
                f(s.val, CellValSlot::ShiftInput);
                f(s.val_shamt, CellValSlot::ShiftAmount);
            }
            CellKind::Register(r) => {
                f(r.init, CellValSlot::RegisterInit);
                for (i, rule) in r.async_trigs.iter().enumerate() {
                    f(rule.cond, CellValSlot::RegisterAsyncCond(i));
                    f(rule.data, CellValSlot::RegisterAsyncData(i));
                }
                if let Some(ref sync) = r.clock_trig {
                    f(sync.clk, CellValSlot::RegisterClock);
                    for (i, rule) in sync.rules.iter().enumerate() {
                        f(rule.cond, CellValSlot::RegisterSyncCond(i));
                        f(rule.data, CellValSlot::RegisterSyncData(i));
                    }
                }
            }
            CellKind::Instance(inst) => {
                for (i, &v) in &inst.params {
                    f(v, CellValSlot::InstanceParam(i));
                }
                for (i, &v) in &inst.ports_in {
                    f(v, CellValSlot::InstancePortIn(i));
                }
                for (i, &v) in &inst.ports_bus {
                    f(v, CellValSlot::InstancePortBus(i));
                }
            }
            CellKind::UnresolvedInstance(inst) => {
                for (i, &(_, v)) in inst.params.iter().enumerate() {
                    f(v, CellValSlot::UnresolvedInstanceParam(i));
                }
                for (i, &(_, v)) in inst.ports_in.iter().enumerate() {
                    f(v, CellValSlot::UnresolvedInstancePortIn(i));
                }
                for (i, &(_, v)) in inst.ports_bus.iter().enumerate() {
                    f(v, CellValSlot::UnresolvedInstancePortBus(i));
                }
            }
            CellKind::InstanceOutput(_) => (),
            CellKind::Bus(_) => (),
            CellKind::BusJoiner(b) => {
                f(b.bus_a, CellValSlot::BusJoinerA);
                f(b.bus_b, CellValSlot::BusJoinerB);
            }
            CellKind::BusDriver(b) => {
                f(b.bus, CellValSlot::BusDriverBus);
                f(b.val, CellValSlot::BusDriverData);
                f(b.cond, CellValSlot::BusDriverCond);
            }
            CellKind::BlackboxBuf(b) => f(b.val, CellValSlot::BlackboxBuf),
            CellKind::Wire(w) => f(w.val, CellValSlot::Wire),
        }
    }

    pub fn replace_val(&mut self, slot: CellValSlot, val: CellId) -> CellId {
        let slot = match slot {
            CellValSlot::PortOut => {
                let CellKind::PortOut(port) = self else { panic!("expected port out") };
                port.val.as_mut().unwrap()
            }
            CellValSlot::SwizzleChunk(i) => {
                let CellKind::Swizzle(swz) = self else { panic!("expected swizzle") };
                let SwizzleChunk::Value { val, ..} = &mut swz.chunks[i] else { panic!("expected value chunk") };
                val
            }
            CellValSlot::BusSwizzleChunk(i) => {
                let CellKind::BusSwizzle(swz) = self else { panic!("expected swizzle") };
                &mut swz.chunks[i].val
            }
            CellValSlot::Slice => {
                let CellKind::Slice(slice) = self else { panic!("expected slice") };
                &mut slice.val
            }
            CellValSlot::Ext => {
                let CellKind::Ext(ext) = self else { panic!("expected ext") };
                &mut ext.val
            }
            CellValSlot::Buf => {
                let CellKind::Buf(buf) = self else { panic!("expected buf") };
                &mut buf.val
            }
            CellValSlot::BitOpA => {
                let CellKind::BitOp(bitop) = self else { panic!("expected bitop") };
                &mut bitop.val_a
            }
            CellValSlot::BitOpB => {
                let CellKind::BitOp(bitop) = self else { panic!("expected bitop") };
                &mut bitop.val_b
            }
            CellValSlot::UnaryXor => {
                let CellKind::UnaryXor(uxor) = self else { panic!("expected uxor") };
                &mut uxor.val
            }
            CellValSlot::MuxSel => {
                let CellKind::Mux(mux) = self else { panic!("expected mux") };
                &mut mux.val_sel
            }
            CellValSlot::MuxInput(i) => {
                let CellKind::Mux(mux) = self else { panic!("expected mux") };
                &mut mux.vals[i]
            }
            CellValSlot::SwitchSel => {
                let CellKind::Switch(switch) = self else { panic!("expected switch") };
                &mut switch.val_sel
            }
            CellValSlot::SwitchInput(i) => {
                let CellKind::Switch(switch) = self else { panic!("expected switch") };
                &mut switch.cases[i].val
            }
            CellValSlot::SwitchDefault => {
                let CellKind::Switch(switch) = self else { panic!("expected switch") };
                &mut switch.default
            }
            CellValSlot::CmpA => {
                let CellKind::Cmp(cmp) = self else { panic!("expected cmp") };
                &mut cmp.val_a
            }
            CellValSlot::CmpB => {
                let CellKind::Cmp(cmp) = self else { panic!("expected cmp") };
                &mut cmp.val_b
            }
            CellValSlot::AddSubA => {
                let CellKind::AddSub(addsub) = self else { panic!("expected addsub") };
                &mut addsub.val_a
            }
            CellValSlot::AddSubB => {
                let CellKind::AddSub(addsub) = self else { panic!("expected addsub") };
                &mut addsub.val_b
            }
            CellValSlot::AddSubInv => {
                let CellKind::AddSub(addsub) = self else { panic!("expected addsub") };
                &mut addsub.val_inv
            }
            CellValSlot::AddSubCarry => {
                let CellKind::AddSub(addsub) = self else { panic!("expected addsub") };
                &mut addsub.val_carry
            }
            CellValSlot::MulA => {
                let CellKind::Mul(mul) = self else { panic!("expected mul") };
                &mut mul.val_a
            }
            CellValSlot::MulB => {
                let CellKind::Mul(mul) = self else { panic!("expected mul") };
                &mut mul.val_b
            }
            CellValSlot::ShiftInput => {
                let CellKind::Shift(shift) = self else { panic!("expected shift") };
                &mut shift.val
            }
            CellValSlot::ShiftAmount => {
                let CellKind::Shift(shift) = self else { panic!("expected shift") };
                &mut shift.val_shamt
            }
            CellValSlot::RegisterInit => {
                let CellKind::Register(reg) = self else { panic!("expected register") };
                &mut reg.init
            }
            CellValSlot::RegisterAsyncCond(i) => {
                let CellKind::Register(reg) = self else { panic!("expected register") };
                &mut reg.async_trigs[i].cond
            }
            CellValSlot::RegisterAsyncData(i) => {
                let CellKind::Register(reg) = self else { panic!("expected register") };
                &mut reg.async_trigs[i].data
            }
            CellValSlot::RegisterClock => {
                let CellKind::Register(reg) = self else { panic!("expected register") };
                &mut reg.clock_trig.as_mut().unwrap().clk
            }
            CellValSlot::RegisterSyncCond(i) => {
                let CellKind::Register(reg) = self else { panic!("expected register") };
                &mut reg.clock_trig.as_mut().unwrap().rules[i].cond
            }
            CellValSlot::RegisterSyncData(i) => {
                let CellKind::Register(reg) = self else { panic!("expected register") };
                &mut reg.clock_trig.as_mut().unwrap().rules[i].data
            }
            CellValSlot::InstanceParam(i) => {
                let CellKind::Instance(inst) = self else { panic!("expected instance") };
                &mut inst.params[i]
            }
            CellValSlot::InstancePortIn(i) => {
                let CellKind::Instance(inst) = self else { panic!("expected instance") };
                &mut inst.ports_in[i]
            }
            CellValSlot::InstancePortBus(i) => {
                let CellKind::Instance(inst) = self else { panic!("expected instance") };
                &mut inst.ports_bus[i]
            }
            CellValSlot::UnresolvedInstanceParam(i) => {
                let CellKind::UnresolvedInstance(inst) = self else { panic!("expected unresolved instance") };
                &mut inst.params[i].1
            }
            CellValSlot::UnresolvedInstancePortIn(i) => {
                let CellKind::UnresolvedInstance(inst) = self else { panic!("expected unresolved instance") };
                &mut inst.ports_in[i].1
            }
            CellValSlot::UnresolvedInstancePortBus(i) => {
                let CellKind::UnresolvedInstance(inst) = self else { panic!("expected unresolved instance") };
                &mut inst.ports_bus[i].1
            }
            CellValSlot::BusJoinerA => {
                let CellKind::BusJoiner(joiner) = self else { panic!("expected bus joiner") };
                &mut joiner.bus_a
            }
            CellValSlot::BusJoinerB => {
                let CellKind::BusJoiner(joiner) = self else { panic!("expected bus joiner") };
                &mut joiner.bus_b
            }
            CellValSlot::BusDriverBus => {
                let CellKind::BusDriver(driver) = self else { panic!("expected bus driver") };
                &mut driver.bus
            }
            CellValSlot::BusDriverData => {
                let CellKind::BusDriver(driver) = self else { panic!("expected bus driver") };
                &mut driver.val
            }
            CellValSlot::BusDriverCond => {
                let CellKind::BusDriver(driver) = self else { panic!("expected bus driver") };
                &mut driver.cond
            }
            CellValSlot::BlackboxBuf => {
                let CellKind::BlackboxBuf(buf) = self else { panic!("expected blackbox buf") };
                &mut buf.val
            }
            CellValSlot::Wire => {
                let CellKind::Wire(wire) = self else { panic!("expected wire") };
                &mut wire.val
            }
        };
        core::mem::replace(slot, val)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum CellValSlot {
    PortOut,
    SwizzleChunk(usize),
    BusSwizzleChunk(usize),
    Slice,
    Ext,
    Buf,
    BitOpA,
    BitOpB,
    UnaryXor,
    MuxSel,
    MuxInput(usize),
    SwitchSel,
    SwitchInput(usize),
    SwitchDefault,
    CmpA,
    CmpB,
    AddSubA,
    AddSubB,
    AddSubInv,
    AddSubCarry,
    MulA,
    MulB,
    ShiftInput,
    ShiftAmount,
    RegisterInit,
    RegisterAsyncCond(usize),
    RegisterAsyncData(usize),
    RegisterClock,
    RegisterSyncCond(usize),
    RegisterSyncData(usize),
    InstanceParam(ParamId),
    InstancePortIn(PortInId),
    InstancePortBus(PortBusId),
    UnresolvedInstanceParam(usize),
    UnresolvedInstancePortIn(usize),
    UnresolvedInstancePortBus(usize),
    BusJoinerA,
    BusJoinerB,
    BusDriverBus,
    BusDriverCond,
    BusDriverData,
    BlackboxBuf,
    Wire,
}

impl CellValSlot {
    pub fn is_bus(self) -> bool {
        matches!(
            self,
            CellValSlot::BusSwizzleChunk(_)
                | CellValSlot::BusJoinerA
                | CellValSlot::BusJoinerB
                | CellValSlot::BusDriverBus
                | CellValSlot::InstancePortBus(_)
                | CellValSlot::UnresolvedInstancePortBus(_)
        )
    }

    pub fn is_plain(self) -> bool {
        !self.is_bus()
    }
}

/// A module parameter cell.  All cells of this type are listed in the [`ModuleRef::params`] field.
///
/// The type is determined by the `typ` field.  Always belongs to the constant plane.
///
/// This cell will usually have a name, but this is mostly not required, as parameters
/// are bound by [`ParamId`].  Exceptions include blackboxes and top modules in cell libraries.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`]
/// - [`CellAnnotation::Position`]
/// - [`CellAnnotation::Attribute`]
/// - [`CellAnnotation::BitIndexing`] (only if `typ` is a known-width bitvec)
#[derive(Debug, Clone, Copy)]
pub struct Param {
    /// Must be equal to the index of this cell within the [`ModuleRef::params`] list.
    pub id: ParamId,
    /// The type of this parameter.
    pub typ: ParamType,
}

/// A type of a [`Param`].
#[derive(Debug, Clone, Copy)]
pub enum ParamType {
    BitVec(u32),
    BitVecAny,
    String,
    Int,
    Float,
}

/// A module input port.  All cells of this type are listed in the [`ModuleRef::ports_in`] field.
///
/// The type is always a bitvec, of width determined by the `width` field.  Always belongs to the main plane.
///
/// Input ports of unknown width are only allowed in blackbox modules.
///
/// This cell will usually have a name, but this is mostly not required, as ports
/// are bound by [`PortInId`].  Exceptions include blackboxes and top modules in cell libraries.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`]
/// - [`CellAnnotation::Position`]
/// - [`CellAnnotation::Attribute`]
/// - [`CellAnnotation::BitIndexing`]
#[derive(Debug, Clone, Copy)]
pub struct PortIn {
    /// Must be equal to the index of this cell within the [`ModuleRef::ports_in`] list.
    pub id: PortInId,
    /// The width of this port, or [`None`] if unknown.
    pub width: Option<u32>,
}

/// A module output port.  All cells of this type are listed in the [`ModuleRef::ports_out`] field.
///
/// This cell has no type and must not be referenced by other cells.
///
/// This cell will usually have a name, but this is mostly not required, as ports
/// are bound by [`PortOutId`].  Exceptions include blackboxes and top modules in cell libraries.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`]
/// - [`CellAnnotation::Position`]
/// - [`CellAnnotation::Attribute`]
/// - [`CellAnnotation::BitIndexing`]
#[derive(Debug, Clone, Copy)]
pub struct PortOut {
    /// Must be equal to the index of this cell within the [`ModuleRef::ports_out`] list.
    pub id: PortOutId,
    /// The width of this port, or [`None`] if unknown.
    pub width: Option<u32>,
    /// The value to be output on this port, or [`None`] for blackbox modules.
    ///
    /// The value must be on the main plane or lower.  The value width must match the port width.
    pub val: Option<CellId>,
}

/// A module bus port.  All cells of this type are listed in the [`ModuleRef::ports_bus`] field.
///
/// The type is always a bitvec, of width determined by the `width` field.  Always belongs to the main plane.
/// In addition to having a bitvec type,  it can also be referenced wherever a [`Bus`] can be referenced.
///
/// Bus ports of unknown width are only allowed in blackbox modules.
///
/// This cell will usually have a name, but this is mostly not required, as ports
/// are bound by [`PortBusId`].  Exceptions include blackboxes and top modules in cell libraries.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`]
/// - [`CellAnnotation::Position`]
/// - [`CellAnnotation::Attribute`]
/// - [`CellAnnotation::BitIndexing`]
#[derive(Debug, Clone, Copy)]
pub struct PortBus {
    /// Must be equal to the index of this cell within the [`ModuleRef::ports_bus`] list.
    pub id: PortBusId,
    /// The width of this port, or [`None`] if unknown.
    pub width: Option<u32>,
    /// The kind of this bus (determines value resolution rules).
    pub kind: BusKind,
}

/// The kind of a [`Bus`], or a [`PortBus`].  Determines value resolution rules.
///
/// When two buses are merged together, by means of bus port connection or a bus joiner, the following rules apply:
///
/// - if both kinds are the same, it becomes the kind of the merged bus
/// - if one kind is `Plain` and the other is `Pulldown` or `Pullup`, the merged bus takes on the non-`Plain` kind
/// - otherwise, an error is emitted
///
/// TODO: Verilog specifies more lax rules, with warnings instead of errors, and `WireOr`/`WireAnd` winning over
/// `Plain` without even a warning, but this generates spooky action at a distance; do we want to relax these rules?
#[derive(Debug, Clone, Eq, PartialEq, Copy)]
pub enum BusKind {
    /// The value is `x` when no driver is active.  When multiple drivers are active, and they don't agree on the value,
    /// the value is also `x` (and the target can get crispy).  Corresponds to `tri` net type in Verilog.
    Plain,
    /// Like `Plain`, but the value is `0` when no driver is active.  Corresponds to `tri0` net type in Verilog.
    Pulldown,
    /// Like `Plain`, but the value is `1` when no driver is active.  Corresponds to `tri1` net type in Verilog.
    Pullup,
    /// The value is:
    ///
    /// - `0` when any active driver drives `0`
    /// - otherwise, `x` if any active driver drives `x`
    /// - otherwise, `1` if any active driver drives `1`
    /// - otherwise, `x`
    ///
    /// Corresponds to `triand` net type in Verilog.
    ///
    /// TODO: do we actually want the "undriven is x" semantics?  This is what Verilog does, but it possibly makes more sense
    /// to just give a value of `1` in that case, so it's equivalent to an AND gate?
    WireAnd,
    /// The value is:
    ///
    /// - `1` when any active driver drives `1`
    /// - otherwise, `x` if any active driver drives `x`
    /// - otherwise, `0` if any active driver drives `0`
    /// - otherwise, `x`
    ///
    /// Corresponds to `trior` net type in Verilog.
    WireOr,
}

/// A swizzle cell.  Takes a list of bitvec value slices or consts and concatenates them together into a new bitvec.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// All chunks in the `chunks` field are concatenated together to make the output value, LSB-first.  The sum of chunk widths
/// must be equal to the `width` field.
///
/// All `Ext` cells are effectively special cases of this cell.
///
/// Does not technically count as a combinatorial cell, as it effectively maps to no hardware.
///
/// The flags and annotations valid for this cell are:
///
/// - param
/// - debug
#[derive(Debug, Clone)]
pub struct Swizzle {
    pub width: u32,
    pub chunks: Vec<SwizzleChunk>,
}

/// A single chunk of a [`Swizzle`] cell.
#[derive(Debug, Clone)]
pub enum SwizzleChunk {
    /// A constant chunk, with the given value.
    Const(Bits),
    /// A chunk extracted from a slice of a value.
    Value {
        /// The value to slice.
        val: CellId,
        /// The start index of the extracted bit slice within the value (0-based, counted from LSB).
        val_start: u32,
        /// The width of the slice to extract from the value.  `val_start + val_len` must not be
        /// larger than the width of the source value.
        val_len: u32,
        /// The final width of this chunk.  If larger than `val_len`, the extracted slice will be sign-extended to this length.
        /// Must not be smaller than `val_len`.
        sext_len: u32,
    },
}

/// A bus swizzle cell.  Like the [`Swizzle`] cell, but operates solely on buses, and the result can be used as a bus (ie. driven).
///
/// Has bitvec type, of width determined by the `width` field.  Always considered to be on the main plane.  In addition to having
/// a bitvec type, can also be used wherever a [`Bus`] be used.
///
/// All chunks in the `chunks` field are concatenated together to make the output bus, LSB-first.  The sum of chunk widths
/// must be equal to the `width` field.
///
/// There are no flags and annotations valid for this cell.
#[derive(Debug, Clone)]
pub struct BusSwizzle {
    pub width: u32,
    pub chunks: Vec<BusSwizzleChunk>,
}

/// A single chunk of a [`BusSwizzle`] cell.
#[derive(Debug, Clone)]
pub struct BusSwizzleChunk {
    /// The bus to slice.  Must be a [`Bus`], [`PortBus`], or [`BusSwizzle`].
    pub val: CellId,
    /// The start index of the extracted bit slice within the bus (0-based, counted from LSB).
    pub val_start: u32,
    /// The width of the slice to extract from the bus.  `val_start + val_len` must not be
    /// larger than the width of the source bus.
    pub val_len: u32,
}

/// A slice cell, effectively a special case of the more general [`Swizzle`] cell.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// The flags and annotations valid for this cell are:
///
/// - param
/// - debug
#[derive(Debug, Clone, Copy)]
pub struct Slice {
    pub width: u32,
    pub val: CellId,
    pub pos: u32,
}

/// An extension-type cell, effectively a special case of the more general [`Swizzle`] cell.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// The source value must have width no larger than the output.
///
/// The flags and annotations valid for this cell are:
///
/// - param
/// - debug
#[derive(Debug, Clone, Copy)]
pub struct Ext {
    pub kind: ExtKind,
    pub width: u32,
    pub val: CellId,
}

/// The subkind of an [`Ext`] cell.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ExtKind {
    /// Zero extension.  The source value will be extended with 0s on the MSB side.
    ///
    /// Equivalent to a [`Swizzle`] with two chunks:
    ///
    /// - [`SwizzleChunk::Value`] referencing the whole of `val`
    /// - [`SwizzleChunk::Const`] with all-0 value of width `(this cell width) - (val width)`
    Zext,
    /// Sign extension.  The source value will be extended with copies of the MSB on the MSB side.
    ///
    /// Equivalent to a [`Swizzle`] with one chunk that references the whole of `val` and sign-extends it to the output width.
    Sext,
}

/// A buffer-type combinatorial cell.  Passes the input straight to the output, potentially inverting all bits.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge (see warning)
/// - param
/// - debug
#[derive(Debug, Clone, Copy)]
pub struct Buf {
    /// If true, inverts the output (ie. is a NOT gate).  If false, is a non-inverting buffer.
    pub inv: bool,
    /// The output width.
    pub width: u32,
    /// The source.  Must have the same width as the output.
    pub val: CellId,
}

/// A binary bitwise operation combinatorial cell.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge (see warning)
/// - async (not valid for [`BitOpKind::Xor`] and [`BitOpKind::Xnor`])
/// - param
/// - debug
#[derive(Debug, Clone, Copy)]
pub struct BitOp {
    pub kind: BitOpKind,
    /// The output width.
    pub width: u32,
    /// The first source.  Must have the same width as the output.
    pub val_a: CellId,
    /// The second source.  Must have the same width as the output.
    pub val_b: CellId,
}

/// The sub-kind of a `BitOp` cell.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BitOpKind {
    /// `a & b`
    And,
    /// `a | b`
    Or,
    /// `a & ~b`
    AndNot,
    /// `a | ~b`
    OrNot,
    /// `~(a & b)`
    Nand,
    /// `~(a | b)`
    Nor,
    /// `a ^ b`
    Xor,
    /// `~(a ^ b)`
    Xnor,
}

/// An unary XOR or XNOR combinatorial cell.  XORs together all bits of the input into a single-bit output, and optionally inverts it.
///
/// Has bitvec type, of width 1.  Can be on any plane, as determined by the flags.
///
/// There is no unary AND.  Use [`CmpKind::Eq`] with a constant 0 instead.
///
/// Likewise, there is no unary OR.  Use [`CmpKind::Eq`] with an all-1 constant.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge (see warning)
/// - param
/// - debug
#[derive(Debug, Clone, Copy)]
pub struct UnaryXor {
    /// If true, inverts the output, making a XNOR gate.  If false, makes a XOR gate.
    pub inv: bool,
    /// The source.  Can be a bitvec of any width.
    pub val: CellId,
}

/// A multiplexer combinatorial cell.  Selects one of multiple inputs based on a selection input.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge (see warning)
/// - async (makes the mux glitchless wrt the select value)
/// - lax_x (see description in individual [`MuxKind`])
/// - param
/// - debug
#[derive(Debug, Clone)]
pub struct Mux {
    pub kind: MuxKind,
    /// The output width.
    pub width: u32,
    /// The selection input.  See [`MuxKind`]-specific requirements.
    pub val_sel: CellId,
    /// The data inputs — one of those will be passed through to the output.  Each input must have the same
    /// width as the output.  See [`MuxKind`]-specific requirements on the number of inputs.
    pub vals: SmallVec<[CellId; 2]>,
}

/// The sub-kind of a [`Mux`] cell.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MuxKind {
    /// A binary multiplexer.  The `val_sel` is treated as an index into the `vals` array.
    ///
    /// The length of the `vals` array must be equal to `2**(val.width)`.
    ///
    /// If lax_x is set, any `x` bit in `val_sel` results in all-`x` output.
    /// Otherwise, substitute all `x` bits in `val_sel` with all possible combinations of `0` and `1`
    /// and collect all corresponding data inputs from `val`.  Any bit lane that has the same value over
    /// all candidate inputs will have this value in the output.  For all remaining bit lanes, the output
    /// value will be `x`.  This matches the Verilog semantics if the mux was to be implemented as a tree
    /// of `?:` operators.
    Binary,
    /// A parallel one-hot multiplexer.  The `vals` array must be one longer than the `val_sel` input's width.
    /// The last input is the default input, and the remaining inputs correspond one-to-one with `val_sel` bits.
    ///
    /// The output value is determined as follows:
    ///
    /// - if `val_sel` is all-0, use the default input
    /// - if `val_sel` is one-hot (has exactly one `1` bit and all other bits are `0`), use the input corresponding to the `1` bit
    /// - otherwise, if lax_x is set, the output is all-`x`
    /// - otherwise:
    ///   - make a list of all candidate inputs:
    ///     - a non-default input is a candidate if its corresponding `val_sel` bit is not `0`
    ///     - the default input is a candidate if no bit is `1`
    ///   - if a bit lane is `0` in all candidate inputs, the output bit is also `0`
    ///   - if a bit lane is `1` in all candidate inputs, the output bit is also `1`
    ///   - otherwise, a given output bit is `x`
    Parallel,
    /// A priority multiplexer.  The `vals` array must be one longer than the `val_sel` input's width.
    /// The last input is the default input, and the remaining inputs correspond one-to-one with `val_sel` bits.
    ///
    /// The output value is taken from the input corresponding to the first (lowest) bit set in `val_sel`, or the default input
    /// if all `val_sel` bits are 0.
    ///
    /// Regarding `x`-propagation semantics, this cell behaves as if it was flattened into a chain of `Binary` multiplexers
    /// in the obvious way, with the lax_x flag (if any) copied to all of them.  In particular, this means
    /// that an `x` in `val_sel` does not necessarily make the whole output undefined even with `LaxX`.
    Priority,
}

/// A switch combinatorial cell.  Like a [`Mux`], but selection is based on comparing the selection input to a list
/// of patterns.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// The `val_sel` input is compared with every case's `sel` field.  Any `x` bits in the case's `sel` val are treated
/// as don't-care bits.  The active case, if any, is determined by priority or parallel rules. The active case's value
/// is then passed through to the output.  If no case is active, the `default` value is used.
///
/// The exact semantics are defined by lowering:
///
/// - for every case, generate a (non-inverted) [`CmpKind::Eq`] cell
///   - one output is a [`Swizzle`] concatenating together all `val_sel` bits correspoding to non-`x` bits in `sel`
///   - the other output is a concatenation of all non-`x` bits in `sel`
/// - transform the `Switch` into a `Mux` with corresponding `kind`
///   - set `val_sel` to a concatenation of all `Eq`s generated
///   - set `vals` to a list of all cases' `val` inputs, with `default` appended at the end
///   - keep `LaxX` flag as-is
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge (see warning)
/// - lax_x
/// - param
/// - debug
///
/// TODO: `Async`?
#[derive(Debug, Clone)]
pub struct Switch {
    pub kind: SwitchKind,
    /// The output width.
    pub width: u32,
    /// The selection input.
    pub val_sel: CellId,
    /// The cases.
    pub cases: Vec<SwitchCase>,
    /// The default input, used if no case is active.  Must be the same width as the output.
    pub default: CellId,
}

/// The sub-kind of a [`Switch`] cell.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SwitchKind {
    /// Priority switch — the first case with matching `sel` value is active.
    Priority,
    /// Parallel switch — any case with matching `sel` value is active.  If multiple cases
    /// are active, they're resolved via the same rules as [`MuxKind::Parallel`].
    Parallel,
}

/// A single case of [`Switch`].
#[derive(Debug, Clone)]
pub struct SwitchCase {
    /// The selection value to be compared with `val_sel`.  Must have the same width as `val_sel`.
    pub sel: Bits,
    /// The data input.  Must have the same width as the cell output.
    pub val: CellId,
}

/// A comparison combinatorial cell.
///
/// Has bitvec type, of width 1.  Can be on any plane, as determined by the flags.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge (see warning)
/// - async (for [`CmpKind::Eq`] only)
/// - lax_x
/// - param
/// - debug
#[derive(Debug, Clone, Copy)]
pub struct Cmp {
    /// The base comparison kind.
    pub kind: CmpKind,
    /// If true, the base comparison result is inverted (eg. `Eq` kind becomes a `!=` operator).  If false, it's passed as-is.
    pub inv: bool,
    /// First data input.
    pub val_a: CellId,
    /// Second data input.  Must be same width as `val_a`.
    pub val_b: CellId,
}

/// The sub-kind of a `Cmp` cell.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CmpKind {
    /// Equality comparison.  `1` iff the two inputs are equal.
    ///
    /// If lax_x flag is set, the result is `x` if any input bit is `x`.
    ///
    /// Otherwise:
    ///
    /// - if any pair of input bits is `(0, 1)` or `(1, 0)`, the output is `0`
    /// - otherwise, if any input bit is `x`, the output is `x`
    /// - otherwise (both inputs equal and have no `x` bits), the output is `1`
    ///
    /// This is equivalent to a wide XOR between the two inputs followed by a reduce-AND.
    Eq,
    /// Unsigned less-than comparison.  `1` if `val_a < val_b` when both inputs are treated as unsigned.
    ///
    /// If lax_x flag is set, the result is `x` if any input bit is `x`.
    /// Otherwise, the result is `x`  only if all input bits above the highest `x` bit in either input
    /// are equal (ie. if an `x` input bit is actually needed to determine the inequality).
    ///
    /// Only `Ult` is provided for unsigned inequalities.  The remaining three comparison operands can be
    /// implemented via operand swapping and the `inv` flag in the cell (which effectively changes the `Ult` into `Uge`).
    Ult,
    /// Signed less-than comparison.  `1` if `val_a < val_b` when both inputs are treated as signed.
    ///    
    /// If lax_x flag is set, the result is `x` if any input bit is `x`.
    /// Otherwise, the result is `x`  only if all input bits above the highest `x` bit in either input
    /// are equal (ie. if an `x` input bit is actually needed to determine the inequality).
    ///
    /// Like with `Ult`, the remaining signed inequalities can be synthesized from `Slt`.
    Slt,
}

/// The addition/subtraction combinatorial cell.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// Computes `val_a + (val_inv ? ~val_b : val_b) + val_carry`.
///
/// If `val_inv` and `val_carry` are both `0`, this effectively computes `val_a + val_b`.
///
/// If `val_inv` and `val_carry` are both `1`, this effectively computes `val_a - val_b`.
///
/// If lax_x is set, any `x` bit on input results in all-`x` output.
/// Otherwise, an `x` bit in `val_a` or `val_b` only sets the same and higher bits of output to `x`, keeping the lower bits defined.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge (see warning)
/// - lax_x
/// - param
/// - debug
#[derive(Debug, Clone, Copy)]
pub struct AddSub {
    /// The output width.
    pub width: u32,
    /// First input.  Must be the same width as the output.
    pub val_a: CellId,
    /// Second input, invertible.  Must be the same width as the output.
    pub val_b: CellId,
    /// Inversion input.  Must be of width 1.
    pub val_inv: CellId,
    /// Carry input.  Must be of width 1.
    pub val_carry: CellId,
}

/// The multiplication combinatorial cell.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// Computes `val_a * val_b`.
///
/// If lax_x is set, any `x` bit on input results in all-`x` output.
/// Otherwise, an `x` bit in `val_a` or `val_b` only sets the same and higher bits of output to `x`, keeping the lower bits defined.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge (see warning)
/// - lax_x
/// - param
/// - debug
#[derive(Debug, Clone, Copy)]
pub struct Mul {
    /// The output width.
    pub width: u32,
    /// First input.  Must be the same width as the output.
    pub val_a: CellId,
    /// Second input.  Must be the same width as the output.
    pub val_b: CellId,
}

/// The shift combinatorial cell.
///
/// Has bitvec type, of width determined by the `width` field.  Can be on any plane, as determined by the flags.
///
/// This is a fairly complex cell, used for performing bit shift and bitfield extraction operations.
///
/// Roughly, computes `val >> (val_shamt * shamt_scale + shamt_bias)`.  The shift amount is treated as signed, and negative values
/// effectively result in left shifts.
///
/// More precisely, the semantics are:
///
/// - cast `val_shamt` to infinite-precision signed integer by either zero extension or sign extension, as determined by `shamt_signed`
///   - if any `val_shamt` bit is `x`, abort; the output is all-`x`
/// - multiply the previous value by `shamt_scale`
/// - add `shamt_bias` to the previous value, obtaining `final_shamt`
/// - to compute output, take `width` consecutive bits from `val` starting from bit index `final_shamt` and going upwards
///   - when bit index is negative, take a default bit value instead:
///     - `0` for [`ShiftKind::Unsigned`] and [`ShiftKind::Signed`]
///     - `x` for [`ShiftKind::FillX`]
///   - when bit index is non-negative and out-of-bounds, take a default bit value instead:
///     - `0` for [`ShiftKind::Unsigned`]
///     - MSB of `val` for [`ShiftKind::Signed`]; if `val` is 0-width, use `0` instead
///     - `x` for [`ShiftKind::FillX`]
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge (see warning)
/// - param
/// - debug
///
/// TODO: LaxX?
#[derive(Debug, Clone, Copy)]
pub struct Shift {
    /// The shift kind.  Determines padding bits inserted before and/or after the shifted value.
    pub kind: ShiftKind,
    /// The output width.
    pub width: u32,
    /// The data input.  Can be of any width, unrelated to output width.
    pub val: CellId,
    /// The shift amount input.  Can be of any width, unrelated to either input or output width.
    pub val_shamt: CellId,
    /// Determines whether `val_shamt` is treated as signed.
    pub shamt_signed: bool,
    /// A (multiplicative) scale to be applied to `val_shamt`.  Negative values effectively result in a left shift.
    pub shamt_scale: i32,
    /// An additive bias to be applied to `val_shamt` (after scale).
    pub shamt_bias: i32,
}

/// The shift kind for [`Shift`] cell.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ShiftKind {
    /// Use `0` for padding.
    Unsigned,
    /// Use `0` for padding on LSB side, sign-extension for padding on MSB side.
    Signed,
    /// Use `x` for padding.
    FillX,
}

/// The register cell.
///
/// Has bitvec type, of width determined by the `width` field.  Always considered to be on the main plane.
///
/// This cell is a memory element, storing a value.  The value is changed as follows:
///
/// - initially, the value is equal to `init`
/// - at all times, the async trigger conditions are evaluated
///   - whenever any async trigger condition is active, the register value is set to the first active async trigger's data value
/// - whenever an active clock edge happens, and no async trigger is active, the clock trigger rules are evaluated
///   - the first rule with active condition determines the value to set the register to
///   - if no rule is active, register is unchanged
///
/// In any trigger or rule, the register's own output can be used as data to define a "no change" rule.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (see warning)
/// - [`CellAnnotation::Attribute`]
/// - keep (see warning)
/// - no_merge
/// - async
///
/// TODO: define semantics for X-valued conditions
#[derive(Debug, Clone)]
pub struct Register {
    /// The register width.
    pub width: u32,
    /// The initial value for the register.  Must have the same width as the register.  Must be on the constant plane.
    pub init: CellId,
    /// The async triggers, in descending priority order.
    pub async_trigs: Vec<RegisterRule>,
    /// The sync trigger, if any.
    pub clock_trig: Option<ClockTrigger>,
}

/// A [`Register`] async trigger or sync trigger rule.
#[derive(Debug, Clone)]
pub struct RegisterRule {
    /// The condition value.  Must have a width of 1.
    pub cond: CellId,
    /// Condition inversion.  If false, the condition is active when equal to `1`.  If true, the condition is active when equal to `0`.
    pub cond_inv: bool,
    /// The data input.  Must have the same width as the register.
    pub data: CellId,
}

/// A [`Register`] sync trigger rule.
#[derive(Debug, Clone)]
pub struct ClockTrigger {
    /// The clock value.  Must have a width of 1.
    pub clk: CellId,
    /// The clock edge or edges considered to be active.
    pub edge: ClockEdge,
    /// The rules to be applied on active clock edge, in descending priority order.
    pub rules: Vec<RegisterRule>,
}

/// The active clock edge for [`ClockTrigger`].
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ClockEdge {
    /// A 0-to-1 transition is an active edge.
    Posedge,
    /// A 1-to-0 transition is an active edge.
    Negedge,
    /// Both 0-to-1 and 1-to-0 transitions are active edges.
    Dualedge,
}

/// An instance cell.
///
/// Has no type, and can only be referred to by [`InstanceOutput`] cells.
///
/// Describes an instantiation of a [`ModuleRef`].
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`]
/// - [`CellAnnotation::Attribute`]
/// - keep
/// - no_merge
/// - no_flatten
#[derive(Debug, Clone)]
pub struct Instance {
    /// The [`ModuleRef`] to be instantiated.
    pub module: ModuleId,
    /// The parameter values.  The list must have length equal to the target's [`ModuleRef::params`],
    /// and the values must have compatible types.  The values must be on the constant plane.
    pub params: EntityVec<ParamId, CellId>,
    /// The input port values.  The list must have length equal to the target's [`ModuleRef::ports_in`],
    /// and the values must be bitvecs of the same width as the corresponding input port (if it has a defined width).
    /// The values must be on the main plane or lower.
    pub ports_in: EntityVec<PortInId, CellId>,
    /// References to [`InstanceOutput`] cells for each output port.  The list must have length equal to the
    /// target's [`ModuleRef::ports_out`].
    pub ports_out: EntityVec<PortOutId, CellId>,
    /// The bus port connections.  The list must have length equal to the target's [`ModuleRef::ports_bus`],
    /// and the values must reference a bus of the same width as the corresponding bus port (if it has a defined width).
    pub ports_bus: EntityVec<PortBusId, CellId>,
}

/// An unresolved instance cell.
///
/// Has no type, and can only be referred to by [`InstanceOutput`] cells.
///
/// Describes an instantiation of a yet-unknown module.  The target module is bound by name,
/// and so are its parameters and ports.
///
/// This cell will be used up in one of the following ways:
///
/// - emitted as-is to eg. Verilog or RTLIL output
/// - converted to a target-specific module instantiation by target code
/// - converted to a normal module instantiation when linking designs together (eg. during elaboration)
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`]
/// - [`CellAnnotation::Attribute`]
/// - keep
/// - no_merge
/// - no_flatten
#[derive(Debug, Clone)]
pub struct UnresolvedInstance {
    /// The name of the instantiated module.
    pub name: HierName,
    /// The parameters and their values, bound by name.  The order is not meaningful.
    /// The values must be on the constant plane.
    pub params: Vec<(PortBinding, CellId)>,
    /// The input ports and their values, bound by name.  The order is not meaningful.
    /// The values must be on the main plane or lower.
    pub ports_in: Vec<(PortBinding, CellId)>,
    /// The output ports and their widths, bound by name.  The order is not meaningful,
    /// but the indices in this array will be referenced by [`InstanceOutput`] cells.
    /// The value referenced is the (unique) [`InstanceOutput`] cell for this port.
    pub ports_out: EntityVec<PortOutId, (PortBinding, CellId)>,
    /// The bus ports and the buses to connect them to, bound by name.  The order is not meaningful.
    /// The referenced values must all be [`Bus`], [`PortBus`], or [`BusSwizzle`].
    pub ports_bus: Vec<(PortBinding, CellId)>,
}

/// Identifies which port or parameter of the target module is to be bound.
#[derive(Debug, Clone)]
pub enum PortBinding {
    /// Binds a parameter or port by name.
    Name(HierName),
    /// Binds a parameter or port by (0-based) HDL-level position.
    Position(u32),
}

/// An instance output cell.  Brings out an output port of an [`Instance`] or [`UnresolvedInstance`] as an value.
///
/// Has bitvec type, of width determined by the `width` field.  Always considered to be on the main plane.
///
/// If this cell refers to an [`Instance`] port, and the corresponding [`PortOut`] has known width, this cell's
/// width must be the same.
///
/// There must be exactly one `InstanceOutput` cell for every [`Instance`] and [`UnresolvedInstance`] output port.
/// This cell is back-referenced in the instance's port list.
///
/// This cell has no valid flags nor annotations.
#[derive(Debug, Clone, Copy)]
pub struct InstanceOutput {
    /// The port's width.
    pub width: u32,
    /// The referenced [`Instance`] or [`UnresolvedInstance`].
    pub inst: CellId,
    /// The referenced output port.  An index into target [`ModuleRef::ports_out`] or [`UnresolvedInstance::ports_out`].
    pub out: PortOutId,
}

/// A bus with multiple drivers.
///
/// The type is always a bitvec, of width determined by the `width` field.  Always belongs to the main plane.
/// In addition to having a bitvec type,  it can also be referenced by [`BusSwizzle`], [`BusJoiner`], [`BusDriver`]
/// cells and connected to instances' bus ports.
///
/// This cell will usually have a name, but this is mostly not required, as ports
/// are bound by [`PortBusId`].  Exceptions include blackboxes and top modules in cell libraries.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`]
/// - [`CellAnnotation::Attribute`]
#[derive(Debug, Clone, Copy)]
pub struct Bus {
    /// The width of this bus.
    pub width: u32,
    /// The kind of this bus (determines value resolution rules).
    pub kind: BusKind,
}

/// A bus joiner.  Connects two buses together.
///
/// Has no type, should not be referenced by other cells.
///
/// There are no valid flags and annotations for this cell.
#[derive(Debug, Clone, Copy)]
pub struct BusJoiner {
    /// The buses to join.  The referenced values must all have the same width, and must be [`Bus`], [`PortBus`], or [`BusSwizzle`].
    pub bus_a: CellId,
    pub bus_b: CellId,
}

/// A bus driver.  Drives a value onto a bus.
///
/// Has no type, should not be referenced by other cells.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`]
/// - [`CellAnnotation::Attribute`]
#[derive(Debug, Clone, Copy)]
pub struct BusDriver {
    /// The bus to drive.  The referenced cell must be [`Bus`], [`PortBus`], or [`BusSwizzle`].
    pub bus: CellId,
    /// The enable signal for this driver.  Must have a width of 1.
    pub cond: CellId,
    /// Enable inversion.  If false, the driver is active when `cond` is equal to `1`.  If true, the driver is active when `cond` is equal to `0`.
    pub cond_inv: bool,
    /// The value to drive onto the bus.  Must have the same width as the bus.
    pub val: CellId,
}

/// A blackbox buffer cell.  Passes input directly to output, but prevents optimizations across the buffer.
///
/// Has bitvec type, of width determined by the `width` field.  Always considered to be on the main plane.
///
/// This cell has twofold semantics:
///
/// - it guarantees that the input value will be directly materialized at some point in the target device
/// - it guarantees that any users of the output of this cell will use that materialized value and not
///   make any assumptions on it actually matching the input to this cell
///   (ie. synthesis will behave as if the materialized value could magically change to something else)
///
/// This cell can still be optimized away if its output is unused.  Mark it with keep to prevent it.
///
/// If there are multiple [`BlackboxBuf`]s referencing the same value, and they are marked with no_merge,
/// each of the outputs will be considered to be a possibly different value.
///
/// The primary purpose of this cell is easy benchmarking: blackbox buffers (with proper flags) can be added
/// around the perimeter of a circuit to be benchmarked to be certain that synthesizer actually implements all of it,
/// without worrying about optimizations being applied due to unused outputs or too simple inputs.
///
/// See [`core::hint::black_box`] for the idea.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`]
/// - [`CellAnnotation::Attribute`]
/// - keep
/// - no_merge
#[derive(Debug, Clone, Copy)]
pub struct BlackboxBuf {
    /// The output width.
    pub width: u32,
    /// The input value.  Must have the same width as output.
    pub val: CellId,
}

/// A wire cell.  Associates a value with a name and related information.
///
/// This cell is effectively debug information describing how to compute the value of
/// a source-level signal.  It should have a name annotation (this is not a validity requirement,
/// but the cell will be fairly useless without it).  If multi-bit, it should also have
/// a [`CellAnnotation::BitIndexing`] annotation.
///
/// The `optimized_out` field is a mask describing which bits of the wire are still available,
/// and which have been optimized out:
///
/// - a `0` bit means that the bit of the wire is still available and valid
/// - a `1` bit means that the bit has been optimized out, and the corresponding `val` bit is
///   irrelevant (it'll probably be connected to a const `x` bit via a swizzle)
/// - an `x` bit is not valid IR
///
/// If a wire is marked as keep, the value will always be preserved in the synthesized
/// netlist, even if it means leaving a bunch of logic with completely unconnected outputs.
/// The `optimized_out` mask will always be all-0.  However, combinatorial logic used only
/// by [`Wire`] cells may be lifted to the debug plane, and not directly present in synthesized
/// output, as long as all necessary register state is kept.  If the value must be directly present
/// in the synthesized output (eg. for benchmarking purposes), [`BlackboxBuf`] can be used.
///
/// If the `Keep` flag is not set, the wire value will be kept on best-effort basis.
/// Any bits of the wire that are no longer available will be marked as such in the `optimized_out`
/// field.  If all bits are optimized out, the wire itself will be removed.
///
/// Has no type, should not be referenced by other cells.  Always conceptually part of the debug plane.
///
/// The flags and annotations valid for this cell are:
///
/// - [`CellAnnotation::Name`] (in fact, a wire should always have a name)
/// - [`CellAnnotation::Attribute`]
/// - [`CellAnnotation::BitIndexing`]
/// - keep
///
/// TODO: it may be the case that some bits of the wire are conditionally available (eg. based on
/// some enable signal), is this something we want to model?
#[derive(Debug, Clone)]
pub struct Wire {
    /// The value of this wire.
    pub val: CellId,
    /// The mask of bits that have been optimized out and are no longer available.
    /// The corresponding bits of `val` must be assumed to be irrelevant.
    pub optimized_out: Bits,
}
