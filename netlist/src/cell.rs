use std::{borrow::Cow, hash::Hash};

use crate::{Design, Net, TargetCellPurity, Value};

mod decision;
mod flip_flop;
mod memory;
mod io_buffer;
mod target;
mod instance;

pub use decision::{MatchCell, AssignCell};
pub use flip_flop::FlipFlop;
pub use memory::{Memory, MemoryWritePort, MemoryReadPort, MemoryReadFlipFlop, MemoryPortRelation};
pub use io_buffer::IoBuffer;
pub use target::TargetCell;
pub use instance::Instance;

/// Compact, 16-byte representation for cells common in fine netlists.
///
/// This type is used internally within [`Design`]. Cells that can be
/// represented with [`CellRepr`] get stored without a separate heap
/// allocation. All others get represented as [`CellRepr::Coarse`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum CellRepr {
    /// Reserved index. Can be created with [`Design::add_void`] for later
    /// replacement with [`Design::replace_value`], or left over
    /// after [`CellRef::unalive`].
    ///
    /// [`CellRef::unalive`]: crate::CellRef::unalive
    Void,

    /// Represents an index taken by a cell with more than one bit of
    /// output. Stores the base index of the cell in question.
    Skip(u32),

    Buf(Net),
    Not(Net),
    And(Net, Net),
    Or(Net, Net),
    Xor(Net, Net),
    Mux(Net, Net, Net), // a ? b : c
    Adc(Net, Net, Net), // a + b + ci

    Coarse(Box<Cell>),
}

/// A unit of logic.
///
/// Within a [`Design`], each cell is identified by a range of indices,
/// with each index corresponding to one of the output bits of the cell.
/// As such, the cell itself only contains information about its inputs,
/// with the outputs being implicit.
///
/// (do note that cells without any outputs, such as [`output`][Cell::Output]
/// or [`name`][Cell::Name], still get assigned an index)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cell {
    Buf(Value),
    Not(Value),
    /// `a & b`.
    ///
    /// Has short-circuiting behavior for inputs containing `X` — if the other
    /// bit is `0`, the output is `0` and the `X` doesn't propagate.
    And(Value, Value),
    /// `a | b`.
    ///
    /// Has short-circuiting behavior for inputs containing `X` — if the other
    /// bit is `1`, the output is `1` and the `X` doesn't propagate.
    Or(Value, Value),
    Xor(Value, Value),
    /// `a ? b : c`.
    ///
    /// Muxes are glitch free — if `a` is `X`, the bit positions that match
    /// between `b` and `c` still have a defined value. The `X` propagates
    /// only at the positions where `b` and `c` differ.
    Mux(Net, Value, Value),
    /// `a + b + ci` — add with carry.
    ///
    /// Output is one bit wider than `a` and `b` — the most significant bit
    /// is the carry-out.
    ///
    /// `X`s in the input propagate only to the more significant bits, and
    /// do not affect the less significant bits.
    Adc(Value, Value, Net), // a + b + ci

    Eq(Value, Value),
    ULt(Value, Value),
    SLt(Value, Value),

    /// `a << (b * c)`. The bottom bits are filled with zeros.
    ///
    /// General notes for all shift cells:
    /// - output is the same width as `a`. If you need wider output,
    ///   zero-extend or sign-extend your input first, as appropriate.
    /// - the shift count does not wrap. If you shift by more than
    ///   `a.len() - 1`, you get the same result as if you made an equivalent
    ///   sequence of 1-bit shifts (i.e. all zeros, all sign bits, or all `X`,
    ///   as appropriate).
    /// - shift cells are one of the few cells which *do not* expect their
    ///   inputs to be of the same width. In fact, that is the expected case.
    Shl(Value, Value, u32),
    /// `a >> (b * c)`. The top bits are filled with zeros.
    ///
    /// See also [general notes above][Cell::Shl].
    UShr(Value, Value, u32),
    /// `a >> (b * c)`. The top bits are filled with copies of the top bit
    /// of the input.
    ///
    /// `a` must be at least one bit wide (as otherwise there would be no sign
    /// bit to propagate, and while there wouldn't be anywhere to propagate it
    /// *to*, it's an edge-case it doesn't make sense to bother handling).
    ///
    /// See also [general notes above][Cell::Shl].
    SShr(Value, Value, u32),
    /// `a >> (b * c)`. The top bits are filled with `X`.
    ///
    /// See also [general notes above][Cell::Shl].
    XShr(Value, Value, u32),

    // future possibilities: popcnt, count leading/trailing zeros, powers
    Mul(Value, Value),
    UDiv(Value, Value),
    UMod(Value, Value),
    SDivTrunc(Value, Value),
    SDivFloor(Value, Value),
    SModTrunc(Value, Value),
    SModFloor(Value, Value),

    Match(MatchCell),
    Assign(AssignCell),

    Dff(FlipFlop),
    Memory(Memory),
    IoBuf(IoBuffer),
    Target(TargetCell),
    Other(Instance),

    /// Design input of a given width.
    ///
    /// If synthesizing for a specified target, and not in out-of-context mode,
    /// an input will be replaced with an [`IoBuffer`] and attached to a pin on
    /// the target device.
    Input(String, usize),
    /// Design output. Attaches a name to a given value.
    ///
    /// If synthesizing for a specified target, and not in out-of-context mode,
    /// an output will be replaced with an [`IoBuffer`] and attached to a pin on
    /// the target device.
    Output(String, Value),
    /// Attaches a name to a given value for debugging.
    ///
    /// `Name` keeps a given value alive during optimization and makes it easily
    /// available to be poked at during simulation.
    ///
    /// Do note that the [`unname` pass][unname], which runs during
    /// target-dependent synthesis, replaces all `Name` cells with [`Debug`]
    /// cells.
    ///
    /// [unname]: ../prjunnamed_generic/fn.unname.html
    /// [`Debug`]: Cell::Debug
    Name(String, Value),
    /// Tentatively attaches a name to a given value.
    ///
    /// `Debug` gives a name to a particular value, without insisting on keeping
    /// it alive during optimization. This helps correlate the output of
    /// synthesis with the corresponding input logic.
    ///
    /// If at any point a value is being kept alive only by a `Debug` cell,
    /// it will be optimized out and the input to the `Debug` cell will
    /// be replaced with `X`.
    ///
    /// See also: [`Name`][Cell::Name].
    Debug(String, Value),
}

impl Cell {
    pub fn validate(&self, design: &Design) {
        match self {
            Cell::Buf(_) => (),
            Cell::Not(_) => (),
            Cell::And(arg1, arg2)
            | Cell::Or(arg1, arg2)
            | Cell::Xor(arg1, arg2)
            | Cell::Mux(_, arg1, arg2)
            | Cell::Adc(arg1, arg2, _)
            | Cell::Eq(arg1, arg2)
            | Cell::ULt(arg1, arg2)
            | Cell::Mul(arg1, arg2)
            | Cell::UDiv(arg1, arg2)
            | Cell::UMod(arg1, arg2) => assert_eq!(arg1.len(), arg2.len()),
            Cell::SLt(arg1, arg2)
            | Cell::SDivTrunc(arg1, arg2)
            | Cell::SDivFloor(arg1, arg2)
            | Cell::SModTrunc(arg1, arg2)
            | Cell::SModFloor(arg1, arg2) => {
                assert_eq!(arg1.len(), arg2.len());
                assert!(!arg1.is_empty());
            }

            Cell::Shl(..) => (),
            Cell::UShr(..) => (),
            Cell::SShr(arg1, _, _) => assert!(!arg1.is_empty()),
            Cell::XShr(..) => (),

            Cell::Match(match_cell) => {
                for alternates in &match_cell.patterns {
                    for pattern in alternates {
                        assert_eq!(match_cell.value.len(), pattern.len());
                    }
                }
            }
            Cell::Assign(assign_cell) => {
                assert!(assign_cell.value.len() >= assign_cell.update.len() + assign_cell.offset);
            }

            Cell::Dff(flip_flop) => {
                assert_eq!(flip_flop.data.len(), flip_flop.init_value.len());
                assert_eq!(flip_flop.data.len(), flip_flop.clear_value.len());
                assert_eq!(flip_flop.data.len(), flip_flop.reset_value.len());
            }
            Cell::Memory(memory) => {
                assert_eq!(memory.init_value.len(), memory.depth * memory.width);
                for port in &memory.write_ports {
                    assert_eq!(port.data.len(), port.mask.len());
                    if memory.width == 0 {
                        assert_eq!(port.data.len(), 0);
                    } else {
                        assert_eq!(port.data.len() % memory.width, 0);
                        let wide_factor = port.data.len() / memory.width;
                        assert!(wide_factor.is_power_of_two());
                        assert_eq!(memory.depth % wide_factor, 0);
                    }
                }
                for port in &memory.read_ports {
                    if memory.width == 0 {
                        assert_eq!(port.data_len, 0);
                    } else {
                        assert_eq!(port.data_len % memory.width, 0);
                        let wide_factor = port.data_len / memory.width;
                        assert!(wide_factor.is_power_of_two());
                        assert_eq!(memory.depth % wide_factor, 0);
                    }
                    if let Some(ref flip_flop) = port.flip_flop {
                        assert_eq!(flip_flop.clear_value.len(), port.data_len);
                        assert_eq!(flip_flop.reset_value.len(), port.data_len);
                        assert_eq!(flip_flop.init_value.len(), port.data_len);
                        assert_eq!(flip_flop.relations.len(), memory.write_ports.len());
                        for (write_port_index, &relation) in flip_flop.relations.iter().enumerate() {
                            if relation != MemoryPortRelation::Undefined {
                                assert_eq!(memory.write_ports[write_port_index].clock, flip_flop.clock);
                            }
                        }
                    }
                }
            }
            Cell::IoBuf(io_buffer) => {
                assert_eq!(io_buffer.output.len(), io_buffer.io.len());
            }
            Cell::Target(target_cell) => {
                let prototype = design.target_prototype(target_cell);
                assert_eq!(target_cell.params.len(), prototype.params.len());
                for (param, value) in prototype.params.iter().zip(target_cell.params.iter()) {
                    assert!(param.kind.is_valid(value));
                }
                assert_eq!(target_cell.inputs.len(), prototype.input_len);
                assert_eq!(target_cell.output_len, prototype.output_len);
                assert_eq!(target_cell.ios.len(), prototype.io_len);
                let target = design.target().unwrap();
                target.validate(design, target_cell);
            }
            Cell::Other(_instance) => {
                // TODO
            }
            Cell::Input(..) => (),
            Cell::Output(..) => (),
            Cell::Name(..) | Cell::Debug(..) => (),
        }
    }

    /// If possible, return a cell that computes only a slice of the outputs
    /// of this cell.
    pub fn slice(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> Option<Cell> {
        match self {
            Cell::Buf(arg) => Some(Cell::Buf(arg.slice(range))),
            Cell::Not(arg) => Some(Cell::Not(arg.slice(range))),
            Cell::And(arg1, arg2) => Some(Cell::And(arg1.slice(range.clone()), arg2.slice(range))),
            Cell::Or(arg1, arg2) => Some(Cell::Or(arg1.slice(range.clone()), arg2.slice(range))),
            Cell::Xor(arg1, arg2) => Some(Cell::Xor(arg1.slice(range.clone()), arg2.slice(range))),
            Cell::Mux(arg1, arg2, arg3) => Some(Cell::Mux(*arg1, arg2.slice(range.clone()), arg3.slice(range))),
            Cell::Dff(flip_flop) => Some(Cell::Dff(flip_flop.slice(range))),
            Cell::IoBuf(io_buffer) => Some(Cell::IoBuf(io_buffer.slice(range))),
            _ => None,
        }
    }
}

impl<'a> From<&'a Cell> for Cow<'a, Cell> {
    fn from(value: &'a Cell) -> Self {
        Cow::Borrowed(value)
    }
}

impl From<Cell> for Cow<'_, Cell> {
    fn from(value: Cell) -> Self {
        Cow::Owned(value)
    }
}

impl CellRepr {
    pub fn get(&self) -> Cow<'_, Cell> {
        match *self {
            CellRepr::Void => unreachable!("void cell"),
            CellRepr::Skip(_) => unreachable!("skip cell"),

            CellRepr::Buf(arg) => Cow::Owned(Cell::Buf(Value::from(arg))),
            CellRepr::Not(arg) => Cow::Owned(Cell::Not(Value::from(arg))),
            CellRepr::And(arg1, arg2) => Cow::Owned(Cell::And(Value::from(arg1), Value::from(arg2))),
            CellRepr::Or(arg1, arg2) => Cow::Owned(Cell::Or(Value::from(arg1), Value::from(arg2))),
            CellRepr::Xor(arg1, arg2) => Cow::Owned(Cell::Xor(Value::from(arg1), Value::from(arg2))),
            CellRepr::Mux(arg1, arg2, arg3) => Cow::Owned(Cell::Mux(arg1, Value::from(arg2), Value::from(arg3))),
            CellRepr::Adc(arg1, arg2, arg3) => Cow::Owned(Cell::Adc(Value::from(arg1), Value::from(arg2), arg3)),

            CellRepr::Coarse(ref coarse) => Cow::Borrowed(coarse),
        }
    }
}

impl From<Cell> for CellRepr {
    fn from(value: Cell) -> Self {
        match value {
            Cell::Buf(arg) if arg.len() == 1 => CellRepr::Buf(arg[0]),
            Cell::Not(arg) if arg.len() == 1 => CellRepr::Not(arg[0]),
            Cell::And(arg1, arg2) if arg1.len() == 1 && arg2.len() == 1 => CellRepr::And(arg1[0], arg2[0]),
            Cell::Or(arg1, arg2) if arg1.len() == 1 && arg2.len() == 1 => CellRepr::Or(arg1[0], arg2[0]),
            Cell::Xor(arg1, arg2) if arg1.len() == 1 && arg2.len() == 1 => CellRepr::Xor(arg1[0], arg2[0]),
            Cell::Mux(arg1, arg2, arg3) if arg2.len() == 1 && arg3.len() == 1 => CellRepr::Mux(arg1, arg2[0], arg3[0]),
            Cell::Adc(arg1, arg2, arg3) if arg1.len() == 1 && arg2.len() == 1 => CellRepr::Adc(arg1[0], arg2[0], arg3),

            coarse => CellRepr::Coarse(Box::new(coarse)),
        }
    }
}

impl CellRepr {
    pub fn output_len(&self) -> usize {
        match self {
            CellRepr::Void => unreachable!("void cell"),
            CellRepr::Skip(_) => unreachable!("skip cell"),

            CellRepr::Buf(..)
            | CellRepr::Not(..)
            | CellRepr::And(..)
            | CellRepr::Or(..)
            | CellRepr::Xor(..)
            | CellRepr::Mux(..) => 1,
            CellRepr::Adc(..) => 2,

            CellRepr::Coarse(coarse) => coarse.output_len(),
        }
    }

    pub(crate) fn visit(&self, mut f: impl FnMut(Net)) {
        match self {
            CellRepr::Void => unreachable!("void cell"),
            CellRepr::Skip(_) => unreachable!("skip cell"),

            CellRepr::Buf(arg) | CellRepr::Not(arg) => arg.visit(&mut f),
            CellRepr::And(arg1, arg2) | CellRepr::Or(arg1, arg2) | CellRepr::Xor(arg1, arg2) => {
                arg1.visit(&mut f);
                arg2.visit(&mut f);
            }
            CellRepr::Mux(arg1, arg2, arg3) | CellRepr::Adc(arg1, arg2, arg3) => {
                arg1.visit(&mut f);
                arg2.visit(&mut f);
                arg3.visit(&mut f);
            }

            CellRepr::Coarse(coarse) => coarse.visit(&mut f),
        }
    }

    pub(crate) fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        match self {
            CellRepr::Void => unreachable!("void cell"),
            CellRepr::Skip(_) => unreachable!("skip cell"),

            CellRepr::Buf(arg) | CellRepr::Not(arg) => arg.visit_mut(&mut f),
            CellRepr::And(arg1, arg2) | CellRepr::Or(arg1, arg2) | CellRepr::Xor(arg1, arg2) => {
                arg1.visit_mut(&mut f);
                arg2.visit_mut(&mut f);
            }
            CellRepr::Mux(arg1, arg2, arg3) | CellRepr::Adc(arg1, arg2, arg3) => {
                arg1.visit_mut(&mut f);
                arg2.visit_mut(&mut f);
                arg3.visit_mut(&mut f);
            }

            CellRepr::Coarse(coarse) => coarse.visit_mut(&mut f),
        }
    }
}

impl Cell {
    pub fn output_len(&self) -> usize {
        match self {
            Cell::Buf(arg) | Cell::Not(arg) => arg.len(),
            Cell::And(arg1, arg2) | Cell::Or(arg1, arg2) | Cell::Xor(arg1, arg2) | Cell::Mux(_, arg1, arg2) => {
                debug_assert_eq!(arg1.len(), arg2.len());
                arg1.len()
            }
            Cell::Adc(arg1, arg2, _) => {
                debug_assert_eq!(arg1.len(), arg2.len());
                arg1.len() + 1
            }

            Cell::Eq(..) | Cell::ULt(..) | Cell::SLt(..) => 1,

            Cell::Shl(arg1, _, _) | Cell::UShr(arg1, _, _) | Cell::SShr(arg1, _, _) | Cell::XShr(arg1, _, _) => {
                arg1.len()
            }

            Cell::Mul(arg1, arg2)
            | Cell::UDiv(arg1, arg2)
            | Cell::UMod(arg1, arg2)
            | Cell::SDivTrunc(arg1, arg2)
            | Cell::SDivFloor(arg1, arg2)
            | Cell::SModTrunc(arg1, arg2)
            | Cell::SModFloor(arg1, arg2) => {
                debug_assert_eq!(arg1.len(), arg2.len());
                arg1.len()
            }

            Cell::Match(match_cell) => match_cell.output_len(),
            Cell::Assign(assign_cell) => assign_cell.output_len(),

            Cell::Dff(flip_flop) => flip_flop.output_len(),
            Cell::Memory(memory) => memory.output_len(),
            Cell::IoBuf(io_buffer) => io_buffer.output_len(),
            Cell::Target(target_cell) => target_cell.output_len,
            Cell::Other(instance) => instance.output_len(),

            Cell::Input(_, width) => *width,
            Cell::Output(..) => 0,
            Cell::Name(..) | Cell::Debug(..) => 0,
        }
    }

    pub fn has_effects(&self, design: &Design) -> bool {
        match self {
            Cell::IoBuf(_) | Cell::Other(_) | Cell::Input(..) | Cell::Output(..) | Cell::Name(..) => true,

            Cell::Target(target_cell) => design.target_prototype(&target_cell).purity == TargetCellPurity::HasEffects,

            _ => false,
        }
    }

    pub fn visit(&self, mut f: impl FnMut(Net)) {
        match self {
            Cell::Input(..) => (),
            Cell::Buf(arg) | Cell::Not(arg) | Cell::Output(_, arg) | Cell::Name(_, arg) | Cell::Debug(_, arg) => {
                arg.visit(&mut f)
            }
            Cell::And(arg1, arg2)
            | Cell::Or(arg1, arg2)
            | Cell::Xor(arg1, arg2)
            | Cell::Mul(arg1, arg2)
            | Cell::UDiv(arg1, arg2)
            | Cell::UMod(arg1, arg2)
            | Cell::SDivTrunc(arg1, arg2)
            | Cell::SDivFloor(arg1, arg2)
            | Cell::SModTrunc(arg1, arg2)
            | Cell::SModFloor(arg1, arg2)
            | Cell::Eq(arg1, arg2)
            | Cell::ULt(arg1, arg2)
            | Cell::SLt(arg1, arg2)
            | Cell::Shl(arg1, arg2, _)
            | Cell::UShr(arg1, arg2, _)
            | Cell::SShr(arg1, arg2, _)
            | Cell::XShr(arg1, arg2, _) => {
                arg1.visit(&mut f);
                arg2.visit(&mut f);
            }
            Cell::Mux(net, value1, value2) | Cell::Adc(value1, value2, net) => {
                value1.visit(&mut f);
                value2.visit(&mut f);
                net.visit(&mut f);
            }
            Cell::Match(match_cell) => match_cell.visit(&mut f),
            Cell::Assign(assign_cell) => assign_cell.visit(&mut f),
            Cell::Dff(flip_flop) => flip_flop.visit(&mut f),
            Cell::Memory(memory) => memory.visit(&mut f),
            Cell::IoBuf(io_buffer) => io_buffer.visit(&mut f),
            Cell::Target(target_cell) => target_cell.visit(&mut f),
            Cell::Other(instance) => instance.visit(&mut f),
        }
    }

    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        match self {
            Cell::Input(..) => (),
            Cell::Buf(arg) | Cell::Not(arg) | Cell::Output(_, arg) | Cell::Name(_, arg) | Cell::Debug(_, arg) => {
                arg.visit_mut(&mut f)
            }
            Cell::And(arg1, arg2)
            | Cell::Or(arg1, arg2)
            | Cell::Xor(arg1, arg2)
            | Cell::Mul(arg1, arg2)
            | Cell::UDiv(arg1, arg2)
            | Cell::UMod(arg1, arg2)
            | Cell::SDivTrunc(arg1, arg2)
            | Cell::SDivFloor(arg1, arg2)
            | Cell::SModTrunc(arg1, arg2)
            | Cell::SModFloor(arg1, arg2)
            | Cell::Eq(arg1, arg2)
            | Cell::ULt(arg1, arg2)
            | Cell::SLt(arg1, arg2)
            | Cell::Shl(arg1, arg2, _)
            | Cell::UShr(arg1, arg2, _)
            | Cell::SShr(arg1, arg2, _)
            | Cell::XShr(arg1, arg2, _) => {
                arg1.visit_mut(&mut f);
                arg2.visit_mut(&mut f);
            }
            Cell::Mux(net, value1, value2) | Cell::Adc(value1, value2, net) => {
                value1.visit_mut(&mut f);
                value2.visit_mut(&mut f);
                net.visit_mut(&mut f);
            }
            Cell::Match(match_cell) => match_cell.visit_mut(&mut f),
            Cell::Assign(assign_cell) => assign_cell.visit_mut(&mut f),
            Cell::Dff(flip_flop) => flip_flop.visit_mut(&mut f),
            Cell::Memory(memory) => memory.visit_mut(&mut f),
            Cell::IoBuf(io_buffer) => io_buffer.visit_mut(&mut f),
            Cell::Target(target_cell) => target_cell.visit_mut(&mut f),
            Cell::Other(instance) => instance.visit_mut(&mut f),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::cell::CellRepr;

    #[test]
    fn test_size() {
        assert!(size_of::<CellRepr>() == 16);
    }
}
