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

#[derive(Debug, Clone)]
pub(crate) enum CellRepr {
    Void,
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cell {
    Buf(Value),
    Not(Value),
    And(Value, Value),
    Or(Value, Value),
    Xor(Value, Value),
    Mux(Net, Value, Value), // a ? b : c
    Adc(Value, Value, Net), // a + b + ci

    Eq(Value, Value),
    ULt(Value, Value),
    SLt(Value, Value),

    Shl(Value, Value, u32),
    UShr(Value, Value, u32),
    SShr(Value, Value, u32),
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

    Input(String, usize),
    Output(String, Value),
    Name(String, Value),
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

            Cell::Shl(_, _, _) => (),
            Cell::UShr(_, _, _) => (),
            Cell::SShr(arg1, _, _) => assert!(!arg1.is_empty()),
            Cell::XShr(_, _, _) => (),

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
            Cell::Input(_, _) => (),
            Cell::Output(_, _) => (),
            Cell::Name(_, _) => (),
        }
    }

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

            CellRepr::Buf(_)
            | CellRepr::Not(_)
            | CellRepr::And(_, _)
            | CellRepr::Or(_, _)
            | CellRepr::Xor(_, _)
            | CellRepr::Mux(_, _, _) => 1,
            CellRepr::Adc(_, _, _) => 2,

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

            Cell::Eq(_, _) | Cell::ULt(_, _) | Cell::SLt(_, _) => 1,

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
            Cell::Output(_, _) => 0,
            Cell::Name(_, _) => 0,
        }
    }

    pub fn has_effects(&self, design: &Design) -> bool {
        match self {
            Cell::IoBuf(_) | Cell::Other(_) | Cell::Input(_, _) | Cell::Output(_, _) | Cell::Name(_, _) => true,

            Cell::Target(target_cell) => design.target_prototype(&target_cell).purity == TargetCellPurity::HasEffects,

            _ => false,
        }
    }

    pub fn visit(&self, mut f: impl FnMut(Net)) {
        match self {
            Cell::Input(_, _) => (),
            Cell::Buf(arg) | Cell::Not(arg) | Cell::Output(_, arg) | Cell::Name(_, arg) => arg.visit(&mut f),
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
            Cell::Input(_, _) => (),
            Cell::Buf(arg) | Cell::Not(arg) | Cell::Output(_, arg) | Cell::Name(_, arg) => arg.visit_mut(&mut f),
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
