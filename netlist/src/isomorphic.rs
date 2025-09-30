use std::fmt::Display;
use std::collections::{BTreeMap, BTreeSet};

use crate::{Cell, Design, IoNet, Net, Value};

#[derive(Debug)]
pub enum NotIsomorphic {
    NoOutputLeft(String),
    NoOutputRight(String),
    OutputSizeMismatch(String),
    IoSizeMismatch(String),
    NameSizeMismatch(String),
    ValueSizeMismatch(Value, Value),
    NetMismatch(Net, Net),
    IoNetMismatch(IoNet, IoNet),
}

impl Display for NotIsomorphic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotIsomorphic::NoOutputLeft(name) => write!(f, "output {:?} is missing in the left design", name),
            NotIsomorphic::NoOutputRight(name) => write!(f, "output {:?} is missing in the right design", name),
            NotIsomorphic::OutputSizeMismatch(name) => write!(f, "size of output {:?} does not match", name),
            NotIsomorphic::IoSizeMismatch(name) => write!(f, "size of IO {:?} does not match", name),
            NotIsomorphic::NameSizeMismatch(name) => write!(f, "size of name cell {:?} does not match", name),
            NotIsomorphic::ValueSizeMismatch(value_l, value_r) => {
                write!(f, "size of values {} and {} do not match", value_l, value_r)
            }
            NotIsomorphic::NetMismatch(net_l, net_r) => write!(f, "nets {} and {} are not isomorphic", net_l, net_r),
            NotIsomorphic::IoNetMismatch(io_net_l, io_net_r) => {
                write!(f, "IO nets {} and {} are not isomorphic", io_net_l, io_net_r)
            }
        }
    }
}

// Beware: this function will ignore instances that have no output bits.
pub fn isomorphic(lft: &Design, rgt: &Design) -> Result<(), NotIsomorphic> {
    let mut queue: BTreeSet<(Net, Net)> = BTreeSet::new();
    fn queue_vals(queue: &mut BTreeSet<(Net, Net)>, val_l: &Value, val_r: &Value) -> Result<(), NotIsomorphic> {
        if val_l.len() != val_r.len() {
            return Err(NotIsomorphic::ValueSizeMismatch(val_l.clone(), val_r.clone()));
        }
        for (net_l, net_r) in val_l.iter().zip(val_r) {
            queue.insert((net_l, net_r));
        }
        Ok(())
    }

    let mut visited: BTreeSet<(Net, Net)> = BTreeSet::new();
    visited.insert((Net::UNDEF, Net::UNDEF));
    visited.insert((Net::ZERO, Net::ZERO));
    visited.insert((Net::ONE, Net::ONE));
    let mut outputs_l = BTreeMap::new();
    let mut names_l = BTreeMap::new();
    for cell in lft.iter_cells() {
        match &*cell.get() {
            Cell::Output(name, value) => {
                outputs_l.insert(name.clone(), value.clone());
            }
            Cell::Name(name, value) => {
                names_l.insert(name.clone(), value.clone());
            }
            _ => (),
        }
    }
    let mut outputs_r = BTreeMap::new();
    let mut names_r = BTreeMap::new();
    for cell in rgt.iter_cells() {
        match &*cell.get() {
            Cell::Output(name, value) => {
                outputs_r.insert(name.clone(), value.clone());
            }
            Cell::Name(name, value) => {
                names_r.insert(name.clone(), value.clone());
            }
            _ => (),
        }
    }
    for (name, value_l) in &outputs_l {
        if let Some(value_r) = outputs_r.get(name) {
            if value_l.len() != value_r.len() {
                return Err(NotIsomorphic::OutputSizeMismatch(name.clone()));
            }
            for (net_l, net_r) in value_l.iter().zip(value_r) {
                queue.insert((net_l, net_r));
            }
        } else {
            return Err(NotIsomorphic::NoOutputRight(name.clone()));
        }
    }
    for name in outputs_r.keys() {
        if !outputs_l.contains_key(name) {
            return Err(NotIsomorphic::NoOutputLeft(name.clone()));
        }
    }
    for (name, value_l) in &names_l {
        if let Some(value_r) = names_r.get(name) {
            if value_l.len() != value_r.len() {
                return Err(NotIsomorphic::NameSizeMismatch(name.clone()));
            }
            for (net_l, net_r) in value_l.iter().zip(value_r) {
                queue.insert((net_l, net_r));
            }
        }
    }
    let mut ios = BTreeSet::new();
    ios.insert((IoNet::FLOATING, IoNet::FLOATING));
    for (name, _) in lft.iter_ios() {
        if let (Some(io_l), Some(io_r)) = (lft.get_io(name), rgt.get_io(name)) {
            if io_l.len() != io_r.len() {
                return Err(NotIsomorphic::IoSizeMismatch(name.to_owned()));
            }
            for (ionet_l, ionet_r) in io_l.iter().zip(io_r.iter()) {
                ios.insert((ionet_l, ionet_r));
            }
        }
    }
    while let Some((net_l, net_r)) = queue.pop_first() {
        if visited.contains(&(net_l, net_r)) {
            continue;
        }
        if net_l.as_const().is_some() || net_r.as_const().is_some() {
            // (const, const) pairs already added to visitted at the beginning
            return Err(NotIsomorphic::NetMismatch(net_l, net_r));
        }
        let (cell_l, bit_l) = lft.find_cell(net_l).unwrap();
        let (cell_r, bit_r) = rgt.find_cell(net_r).unwrap();
        let out_l = cell_l.output();
        let out_r = cell_r.output();
        if bit_l != bit_r || out_l.len() != out_r.len() {
            return Err(NotIsomorphic::NetMismatch(net_l, net_r));
        }
        for (net_l, net_r) in out_l.iter().zip(out_r) {
            visited.insert((net_l, net_r));
        }
        match (&*cell_l.get(), &*cell_r.get()) {
            (Cell::Buf(val_l), Cell::Buf(val_r)) | (Cell::Not(val_l), Cell::Not(val_r)) => {
                queue_vals(&mut queue, val_l, val_r)?
            }
            (Cell::And(arg1_l, arg2_l), Cell::And(arg1_r, arg2_r))
            | (Cell::Or(arg1_l, arg2_l), Cell::Or(arg1_r, arg2_r))
            | (Cell::Xor(arg1_l, arg2_l), Cell::Xor(arg1_r, arg2_r))
            | (Cell::Eq(arg1_l, arg2_l), Cell::Eq(arg1_r, arg2_r))
            | (Cell::ULt(arg1_l, arg2_l), Cell::ULt(arg1_r, arg2_r))
            | (Cell::SLt(arg1_l, arg2_l), Cell::SLt(arg1_r, arg2_r))
            | (Cell::Mul(arg1_l, arg2_l), Cell::Mul(arg1_r, arg2_r))
            | (Cell::UDiv(arg1_l, arg2_l), Cell::UDiv(arg1_r, arg2_r))
            | (Cell::UMod(arg1_l, arg2_l), Cell::UMod(arg1_r, arg2_r))
            | (Cell::SDivTrunc(arg1_l, arg2_l), Cell::SDivTrunc(arg1_r, arg2_r))
            | (Cell::SDivFloor(arg1_l, arg2_l), Cell::SDivFloor(arg1_r, arg2_r))
            | (Cell::SModTrunc(arg1_l, arg2_l), Cell::SModTrunc(arg1_r, arg2_r))
            | (Cell::SModFloor(arg1_l, arg2_l), Cell::SModFloor(arg1_r, arg2_r)) => {
                queue_vals(&mut queue, arg1_l, arg1_r)?;
                queue_vals(&mut queue, arg2_l, arg2_r)?;
            }
            (Cell::Mux(arg1_l, arg2_l, arg3_l), Cell::Mux(sel_r, arg2_r, arg3_r)) => {
                queue.insert((*arg1_l, *sel_r));
                queue_vals(&mut queue, arg2_l, arg2_r)?;
                queue_vals(&mut queue, arg3_l, arg3_r)?;
            }
            (Cell::Adc(arg1_l, arg2_l, arg3_l), Cell::Adc(arg1_r, arg2_r, arg3_r)) => {
                queue_vals(&mut queue, arg1_l, arg1_r)?;
                queue_vals(&mut queue, arg2_l, arg2_r)?;
                queue.insert((*arg3_l, *arg3_r));
            }
            (Cell::Aig(arg1_l, arg2_l), Cell::Aig(arg1_r, arg2_r)) => {
                queue.insert((arg1_l.net(), arg1_r.net()));
                queue.insert((arg2_l.net(), arg2_r.net()));
                if arg1_l.is_positive() != arg1_r.is_positive() || arg2_l.is_positive() != arg2_r.is_positive() {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
            }
            (Cell::Shl(arg1_l, arg2_l, stride_l), Cell::Shl(arg1_r, arg2_r, stride_r))
            | (Cell::UShr(arg1_l, arg2_l, stride_l), Cell::UShr(arg1_r, arg2_r, stride_r))
            | (Cell::SShr(arg1_l, arg2_l, stride_l), Cell::SShr(arg1_r, arg2_r, stride_r))
            | (Cell::XShr(arg1_l, arg2_l, stride_l), Cell::XShr(arg1_r, arg2_r, stride_r)) => {
                queue_vals(&mut queue, arg1_l, arg1_r)?;
                queue_vals(&mut queue, arg2_l, arg2_r)?;
                if stride_l != stride_r {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
            }
            (Cell::Dff(ff_l), Cell::Dff(ff_r)) => {
                queue_vals(&mut queue, &ff_l.data, &ff_r.data)?;
                
                // zip the control nets and check if they are isomorphic
                let ff_l_clear = ff_l.clear.clone().control_nets();
                let ff_r_clear = ff_r.clear.clone().control_nets();
                let zipped_clears = ff_l_clear.iter().zip(ff_r_clear.iter());
                for (clear_l, clear_r) in zipped_clears {
                    queue.insert((clear_l.net(), clear_r.net()));
                }

                queue.insert((ff_l.clock.net(), ff_r.clock.net()));
                // queue.insert((ff_l.clear.net(), ff_r.clear.net()));
                queue.insert((ff_l.reset.net(), ff_r.reset.net()));
                queue.insert((ff_l.enable.net(), ff_r.enable.net()));
                if ff_l.clock.is_positive() != ff_r.clock.is_positive()
                    || ff_l.clear.is_positive() != ff_r.clear.is_positive()
                    || ff_l.reset.is_positive() != ff_r.reset.is_positive()
                    || ff_l.enable.is_positive() != ff_r.enable.is_positive()
                    || (ff_l.reset_over_enable != ff_r.reset_over_enable
                        && !ff_l.reset.is_always(false)
                        && !ff_l.enable.is_always(true))
                    || ff_l.clear_value != ff_r.clear_value
                    || ff_l.reset_value != ff_r.reset_value
                    || ff_l.init_value != ff_r.init_value
                {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
            }
            (Cell::IoBuf(iobuf_l), Cell::IoBuf(iobuf_r)) => {
                for (io_net_l, io_net_r) in iobuf_l.io.iter().zip(iobuf_r.io.iter()) {
                    if !ios.contains(&(io_net_l, io_net_r)) {
                        return Err(NotIsomorphic::IoNetMismatch(io_net_l, io_net_r));
                    }
                }
                queue_vals(&mut queue, &iobuf_l.output, &iobuf_r.output)?;
                queue.insert((iobuf_l.enable.net(), iobuf_r.enable.net()));
                if iobuf_l.enable.is_positive() != iobuf_r.enable.is_positive() {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
            }
            (Cell::Memory(memory_l), Cell::Memory(memory_r)) => {
                if memory_l.depth != memory_r.depth
                    || memory_l.width != memory_r.width
                    || memory_l.init_value != memory_r.init_value
                    || memory_l.write_ports.len() != memory_r.write_ports.len()
                    || memory_l.read_ports.len() != memory_r.read_ports.len()
                {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
                for (write_port_l, write_port_r) in memory_l.write_ports.iter().zip(memory_r.write_ports.iter()) {
                    queue_vals(&mut queue, &write_port_l.addr, &write_port_r.addr)?;
                    queue_vals(&mut queue, &write_port_l.data, &write_port_r.data)?;
                    queue_vals(&mut queue, &write_port_l.mask, &write_port_r.mask)?;
                    queue.insert((write_port_l.clock.net(), write_port_r.clock.net()));
                    if write_port_l.clock.is_positive() != write_port_r.clock.is_positive() {
                        return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                    }
                }
                for (read_port_l, read_port_r) in memory_l.read_ports.iter().zip(memory_r.read_ports.iter()) {
                    queue_vals(&mut queue, &read_port_l.addr, &read_port_r.addr)?;
                    if read_port_l.data_len != read_port_r.data_len {
                        return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                    }
                    match (&read_port_l.flip_flop, &read_port_r.flip_flop) {
                        (None, None) => (),
                        (Some(ff_l), Some(ff_r)) => {
                            queue.insert((ff_l.clock.net(), ff_r.clock.net()));
                            queue.insert((ff_l.clear.net(), ff_r.clear.net()));
                            queue.insert((ff_l.reset.net(), ff_r.reset.net()));
                            queue.insert((ff_l.enable.net(), ff_r.enable.net()));
                            if ff_l.clock.is_positive() != ff_r.clock.is_positive()
                                || ff_l.clear.is_positive() != ff_r.clear.is_positive()
                                || ff_l.reset.is_positive() != ff_r.reset.is_positive()
                                || ff_l.enable.is_positive() != ff_r.enable.is_positive()
                                || (ff_l.reset_over_enable != ff_r.reset_over_enable
                                    && !ff_l.reset.is_always(false)
                                    && !ff_l.enable.is_always(true))
                                || ff_l.clear_value != ff_r.clear_value
                                || ff_l.reset_value != ff_r.reset_value
                                || ff_l.init_value != ff_r.init_value
                                || ff_l.relations != ff_r.relations
                            {
                                return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                            }
                        }
                        _ => return Err(NotIsomorphic::NetMismatch(net_l, net_r)),
                    }
                }
            }
            (Cell::Target(target_cell_l), Cell::Target(target_cell_r)) => {
                for (io_net_l, io_net_r) in target_cell_l.ios.iter().zip(target_cell_r.ios.iter()) {
                    if !ios.contains(&(io_net_l, io_net_r)) {
                        return Err(NotIsomorphic::IoNetMismatch(io_net_l, io_net_r));
                    }
                }
                if target_cell_l.kind != target_cell_r.kind || target_cell_l.params != target_cell_r.params {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
                queue_vals(&mut queue, &target_cell_l.inputs, &target_cell_r.inputs)?;
            }
            (Cell::Other(inst_l), Cell::Other(inst_r)) => {
                if inst_l.kind != inst_r.kind || inst_l.params != inst_r.params || inst_l.outputs != inst_r.outputs {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
                for (name, value_l) in &inst_l.inputs {
                    let Some(value_r) = inst_r.inputs.get(name) else {
                        return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                    };
                    queue_vals(&mut queue, value_l, value_r)?;
                }
                for name in inst_r.inputs.keys() {
                    if !inst_l.inputs.contains_key(name) {
                        return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                    }
                }
                for (name, io_value_l) in &inst_l.ios {
                    let Some(io_value_r) = inst_r.ios.get(name) else {
                        return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                    };
                    for (io_net_l, io_net_r) in io_value_l.iter().zip(io_value_r.iter()) {
                        if !ios.contains(&(io_net_l, io_net_r)) {
                            return Err(NotIsomorphic::IoNetMismatch(io_net_l, io_net_r));
                        }
                    }
                }
                for name in inst_r.ios.keys() {
                    if !inst_l.ios.contains_key(name) {
                        return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                    }
                }
            }
            (Cell::Input(name_l, _), Cell::Input(name_r, _)) => {
                if name_l != name_r {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
            }
            _ => return Err(NotIsomorphic::NetMismatch(net_l, net_r)),
        }
    }
    Ok(())
}

#[macro_export]
macro_rules! assert_isomorphic {
    ( $lft:ident, $rgt:ident ) => {
        $lft.apply();
        $rgt.apply();
        let result = prjunnamed_netlist::isomorphic(&$lft, &$rgt);
        if let Err(error) = result {
            panic!("{}\nleft design:\n{}\nright design:\n{}", error, $lft, $rgt);
        }
    };
}
