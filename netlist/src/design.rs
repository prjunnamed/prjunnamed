use std::ops::Range;
use std::cell::RefCell;
use std::borrow::Cow;
use std::collections::{btree_map, BTreeMap, BTreeSet};
use std::fmt::Display;
use std::hash::Hash;
use std::sync::Arc;

use crate::cell::{Cell, CellRepr};
use crate::{
    ControlNet, FlipFlop, Instance, IoBuffer, IoNet, IoValue, Net, Target, Trit, Value, TargetCellPurity, TargetCell,
    TargetPrototype, Memory,
};

#[derive(Debug, Clone)]
pub struct Design {
    ios: BTreeMap<String, Range<u32>>,
    cells: Vec<Cell>,
    changes: RefCell<ChangeQueue>,
    target: Option<Arc<dyn Target>>,
}

#[derive(Debug, Clone)]
struct ChangeQueue {
    next_io: u32,
    added_ios: BTreeMap<String, Range<u32>>,
    added_cells: Vec<Cell>,
    replaced_cells: BTreeMap<usize, CellRepr>,
    unalived_cells: BTreeSet<usize>,
    replaced_nets: BTreeMap<Net, Net>,
}

impl Design {
    pub fn new() -> Design {
        Self::with_target(None)
    }

    pub fn with_target(target: Option<Arc<dyn Target>>) -> Design {
        Design {
            ios: BTreeMap::new(),
            cells: vec![],
            changes: RefCell::new(ChangeQueue {
                next_io: 0,
                added_ios: BTreeMap::new(),
                added_cells: vec![],
                replaced_cells: BTreeMap::new(),
                unalived_cells: BTreeSet::new(),
                replaced_nets: BTreeMap::new(),
            }),
            target,
        }
    }

    pub fn add_io(&self, name: impl Into<String>, width: usize) -> IoValue {
        let mut changes = self.changes.borrow_mut();
        let name = name.into();
        let width = width as u32;
        let range = changes.next_io..(changes.next_io + width);
        changes.next_io += width;
        if self.ios.contains_key(&name) {
            panic!("duplicate IO port {name}");
        }
        match changes.added_ios.entry(name) {
            btree_map::Entry::Occupied(entry) => {
                panic!("duplicate IO port {}", entry.key());
            }
            btree_map::Entry::Vacant(entry) => {
                entry.insert(range.clone());
            }
        }
        IoValue::from_range(range)
    }

    pub fn get_io(&self, name: impl AsRef<str>) -> Option<IoValue> {
        self.ios.get(name.as_ref()).map(|range| IoValue::from_range(range.clone()))
    }

    pub fn find_io(&self, io_net: IoNet) -> Option<(&str, usize)> {
        for (name, range) in self.ios.iter() {
            if range.contains(&io_net.index) {
                return Some((name.as_str(), (io_net.index - range.start) as usize));
            }
        }
        None
    }

    pub fn iter_ios(&self) -> impl Iterator<Item = (&str, IoValue)> {
        self.ios.iter().map(|(name, range)| (name.as_str(), IoValue::from_range(range.clone())))
    }

    pub fn add_cell(&self, cell: CellRepr) -> Value {
        cell.validate(self);
        let mut changes = self.changes.borrow_mut();
        let index = self.cells.len() + changes.added_cells.len();
        let output_len = cell.output_len();
        changes.added_cells.push(cell.into());
        if output_len > 1 {
            for _ in 0..(output_len - 1) {
                changes.added_cells.push(Cell::Skip(index.try_into().expect("cell index too large")))
            }
        }
        Value::cell(index, output_len)
    }

    pub fn add_void(&self, width: usize) -> Value {
        let mut changes = self.changes.borrow_mut();
        let index = self.cells.len() + changes.added_cells.len();
        for _ in 0..width {
            changes.added_cells.push(Cell::Void);
        }
        Value::cell(index, width)
    }

    fn locate_cell(&self, net: Net) -> Result<(usize, usize), Trit> {
        if let Some(trit) = net.as_const() {
            return Err(trit);
        }
        let index = net.as_cell().unwrap();
        let (cell_index, bit_index) = match self.cells[index] {
            Cell::Void => panic!("located a void cell %{index} in design"),
            Cell::Skip(start) => (start as usize, index - start as usize),
            _ => (index, 0),
        };
        Ok((cell_index, bit_index))
    }

    pub fn find_cell(&self, net: Net) -> Result<(CellRef, usize), Trit> {
        self.locate_cell(net).map(|(cell_index, bit_index)| (CellRef { design: self, index: cell_index }, bit_index))
    }

    pub fn iter_cells(&self) -> CellIter {
        CellIter { design: self, index: 0 }
    }

    pub(crate) fn is_valid_cell_index(&self, index: usize) -> bool {
        index < self.cells.len()
    }

    pub fn replace_net(&self, from_net: impl Into<Net>, to_net: impl Into<Net>) {
        let (from_net, to_net) = (from_net.into(), to_net.into());
        if from_net != to_net {
            let mut changes = self.changes.borrow_mut();
            assert_eq!(changes.replaced_nets.insert(from_net, to_net), None);
        }
    }

    pub fn replace_value<'a, 'b>(&self, from_value: impl Into<Cow<'a, Value>>, to_value: impl Into<Cow<'b, Value>>) {
        let (from_value, to_value) = (from_value.into(), to_value.into());
        assert_eq!(from_value.len(), to_value.len());
        for (from_net, to_net) in from_value.iter().zip(to_value.iter()) {
            self.replace_net(from_net, to_net);
        }
    }

    pub fn map_net(&self, net: impl Into<Net>) -> Net {
        let changes = self.changes.borrow();
        let net = net.into();
        let mapped_net = *changes.replaced_nets.get(&net).unwrap_or(&net);
        // Assume the caller might want to locate the cell behind the net.
        match mapped_net.as_cell() {
            Some(index) if index >= self.cells.len() => return net,
            _ => return mapped_net,
        }
    }

    pub fn map_value(&self, value: impl Into<Value>) -> Value {
        value.into().iter().map(|net| self.map_net(net)).collect::<Vec<_>>().into()
    }

    pub fn apply(&mut self) -> bool {
        let changes = self.changes.get_mut();
        let mut did_change = !changes.added_ios.is_empty() || !changes.added_cells.is_empty();
        for cell_index in std::mem::take(&mut changes.unalived_cells) {
            let output_len = self.cells[cell_index].output_len().max(1);
            for index in cell_index..cell_index + output_len {
                self.cells[index] = Cell::Void;
            }
            did_change = true;
        }
        for (index, new_cell) in std::mem::take(&mut changes.replaced_cells) {
            assert_eq!(self.cells[index].output_len(), new_cell.output_len());
            // CellRef::replace() ensures the new repr is different.
            self.cells[index] = new_cell.into();
            did_change = true;
        }
        self.ios.extend(std::mem::take(&mut changes.added_ios));
        self.cells.extend(std::mem::take(&mut changes.added_cells));
        if !changes.replaced_nets.is_empty() {
            for cell in self.cells.iter_mut().filter(|cell| !matches!(cell, Cell::Skip(_) | Cell::Void)) {
                cell.visit_mut(|net| {
                    while let Some(new_net) = changes.replaced_nets.get(net) {
                        if *net != *new_net {
                            *net = *new_net;
                            did_change = true;
                        }
                    }
                });
            }
            changes.replaced_nets.clear();
        }
        did_change
    }

    pub fn target(&self) -> Option<Arc<dyn Target>> {
        self.target.as_ref().map(|target| target.clone())
    }

    pub fn target_prototype(&self, target_cell: &TargetCell) -> &TargetPrototype {
        self.target.as_ref().unwrap().prototype(&target_cell.kind).unwrap()
    }
}

#[derive(Clone, Copy)]
pub struct CellRef<'a> {
    design: &'a Design,
    index: usize,
}

impl PartialEq<CellRef<'_>> for CellRef<'_> {
    fn eq(&self, other: &CellRef<'_>) -> bool {
        std::ptr::eq(self.design, other.design) && self.index == other.index
    }
}

impl Eq for CellRef<'_> {}

impl Hash for CellRef<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

impl<'a> CellRef<'a> {
    pub fn repr(&self) -> Cow<'a, CellRepr> {
        self.design.cells[self.index].repr()
    }

    pub fn output_len(&self) -> usize {
        self.design.cells[self.index].output_len()
    }

    pub fn output(&self) -> Value {
        Value::cell(self.index, self.output_len())
    }

    pub fn visit(&self, f: impl FnMut(Net)) {
        self.design.cells[self.index].visit(f)
    }

    pub fn replace(&self, to_cell: CellRepr) {
        if *self.design.cells[self.index].repr() != to_cell {
            to_cell.validate(&self.design);
            let mut changes = self.design.changes.borrow_mut();
            assert!(changes.replaced_cells.insert(self.index, to_cell).is_none());
        }
    }

    pub fn unalive(&self) {
        let mut changes = self.design.changes.borrow_mut();
        changes.unalived_cells.insert(self.index);
    }

    // Returns the same index as the one used by `Display` implementation.`
    pub fn debug_index(&self) -> usize {
        self.index
    }
}

pub struct CellIter<'a> {
    design: &'a Design,
    index: usize,
}

impl<'a> Iterator for CellIter<'a> {
    type Item = CellRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while matches!(self.design.cells.get(self.index), Some(Cell::Void)) {
            self.index += 1;
        }
        if self.index < self.design.cells.len() {
            let cell_ref = CellRef { design: self.design, index: self.index };
            self.index += self.design.cells[self.index].output_len().max(1);
            Some(cell_ref)
        } else {
            None
        }
    }
}

macro_rules! builder_fn {
    () => {};

    ($func:ident( $($arg:ident : $ty:ty),+ ) -> $repr:ident $body:tt; $($rest:tt)*) => {
        pub fn $func(&self, $( $arg: $ty ),+) -> Value {
            self.add_cell(CellRepr::$repr $body)
        }

        builder_fn!{ $($rest)* }
    };

    // For cells with no output value.
    ($func:ident( $($arg:ident : $ty:ty),+ ) : $repr:ident $body:tt; $($rest:tt)*) => {
        pub fn $func(&self, $( $arg: $ty ),+) {
            self.add_cell(CellRepr::$repr $body);
        }

        builder_fn!{ $($rest)* }
    };
}

impl Design {
    builder_fn! {
        add_buf(arg: impl Into<Value>) ->
            Buf(arg.into());
        add_not(arg: impl Into<Value>) ->
            Not(arg.into());
        add_and(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            And(arg1.into(), arg2.into());
        add_or(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            Or(arg1.into(), arg2.into());
        add_xor(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            Xor(arg1.into(), arg2.into());
        add_adc(arg1: impl Into<Value>, arg2: impl Into<Value>, arg3: impl Into<Net>) ->
            Adc(arg1.into(), arg2.into(), arg3.into());

        add_eq(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            Eq(arg1.into(), arg2.into());
        add_ult(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            ULt(arg1.into(), arg2.into());
        add_slt(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            SLt(arg1.into(), arg2.into());

        add_shl(arg1: impl Into<Value>, arg2: impl Into<Value>, stride: u32) ->
            Shl(arg1.into(), arg2.into(), stride);
        add_ushr(arg1: impl Into<Value>, arg2: impl Into<Value>, stride: u32) ->
            UShr(arg1.into(), arg2.into(), stride);
        add_sshr(arg1: impl Into<Value>, arg2: impl Into<Value>, stride: u32) ->
            SShr(arg1.into(), arg2.into(), stride);
        add_xshr(arg1: impl Into<Value>, arg2: impl Into<Value>, stride: u32) ->
            XShr(arg1.into(), arg2.into(), stride);

        add_mul(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            Mul(arg1.into(), arg2.into());
        add_udiv(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            UDiv(arg1.into(), arg2.into());
        add_umod(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            UMod(arg1.into(), arg2.into());
        add_sdiv_trunc(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            SDivTrunc(arg1.into(), arg2.into());
        add_sdiv_floor(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            SDivFloor(arg1.into(), arg2.into());
        add_smod_trunc(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            SModTrunc(arg1.into(), arg2.into());
        add_smod_floor(arg1: impl Into<Value>, arg2: impl Into<Value>) ->
            SModFloor(arg1.into(), arg2.into());

        add_dff(arg: impl Into<FlipFlop>) ->
            Dff(arg.into());
        add_memory(arg: impl Into<Memory>) ->
            Memory(arg.into());
        add_iob(arg: impl Into<IoBuffer>) ->
            Iob(arg.into());
        add_other(arg: impl Into<Instance>) ->
            Other(arg.into());
        add_target(arg: impl Into<TargetCell>) ->
            Target(arg.into());

        add_input(name: impl Into<String>, width: usize) ->
            Input(name.into(), width);
        add_output(name: impl Into<String>, value: impl Into<Value>) :
            Output(name.into(), value.into());
        add_name(name: impl Into<String>, value: impl Into<Value>) :
            Name(name.into(), value.into());
    }

    pub fn add_mux(&self, arg1: impl Into<ControlNet>, arg2: impl Into<Value>, arg3: impl Into<Value>) -> Value {
        match arg1.into() {
            ControlNet::Pos(net) => self.add_cell(CellRepr::Mux(net, arg2.into(), arg3.into())),
            ControlNet::Neg(net) => self.add_cell(CellRepr::Mux(net, arg3.into(), arg2.into())),
        }
    }

    pub fn add_ne(&self, arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value {
        let eq = self.add_eq(arg1, arg2);
        self.add_not(eq)
    }

    pub fn add_not1(&self, arg: impl Into<Net>) -> Net {
        self.add_not(arg.into()).unwrap_net()
    }

    pub fn add_and1(&self, arg1: impl Into<Net>, arg2: impl Into<Net>) -> Net {
        self.add_and(arg1.into(), arg2.into()).unwrap_net()
    }

    pub fn add_or1(&self, arg1: impl Into<Net>, arg2: impl Into<Net>) -> Net {
        self.add_or(arg1.into(), arg2.into()).unwrap_net()
    }

    pub fn add_xor1(&self, arg1: impl Into<Net>, arg2: impl Into<Net>) -> Net {
        self.add_xor(arg1.into(), arg2.into()).unwrap_net()
    }

    pub fn add_input1(&self, name: impl Into<String>) -> Net {
        self.add_input(name, 1).unwrap_net()
    }
}

impl Design {
    pub fn iter_cells_topo<'a>(&'a self) -> impl DoubleEndedIterator<Item = CellRef<'a>> {
        fn get_deps(design: &Design, cell: CellRef) -> BTreeSet<usize> {
            let mut result = BTreeSet::new();
            cell.visit(|net| {
                if let Ok((cell, _offset)) = design.find_cell(net) {
                    result.insert(cell.index);
                }
            });
            result
        }

        let mut result = vec![];
        let mut visited = BTreeSet::new();
        // emit inputs, iobs and stateful cells first, in netlist order
        for cell in self.iter_cells() {
            match &*cell.repr() {
                CellRepr::Input(..) | CellRepr::Iob(..) | CellRepr::Dff(..) | CellRepr::Other(..) => {
                    visited.insert(cell.index);
                    result.push(cell);
                }
                CellRepr::Target(target_cell) => {
                    if self.target_prototype(target_cell).purity != TargetCellPurity::Pure {
                        visited.insert(cell.index);
                        result.push(cell);
                    }
                }
                _ => (),
            }
        }
        // now emit combinational cells, in topologically-sorted order whenever possible.
        // we try to emit them in netlist order; however, if we try to emit a cell
        // that has an input that has not yet been emitted, we push it on a stack,
        // and go emit the inputs instead.  the cell is put on the "visitted" list
        // as soon as we start processing it, so cycles will be automatically broken
        // by considering inputs already on the processing stack as "already emitted".
        for cell in self.iter_cells() {
            if matches!(&*cell.repr(), CellRepr::Output(..) | CellRepr::Name(..)) {
                continue;
            }
            if visited.contains(&cell.index) {
                continue;
            }
            visited.insert(cell.index);
            let mut stack = vec![(cell, get_deps(self, cell))];
            'outer: while let Some((cell, deps)) = stack.last_mut() {
                while let Some(dep_index) = deps.pop_first() {
                    if !visited.contains(&dep_index) {
                        let cell = CellRef { design: self, index: dep_index };
                        visited.insert(dep_index);
                        stack.push((cell, get_deps(self, cell)));
                        continue 'outer;
                    }
                }
                result.push(*cell);
                stack.pop();
            }
        }
        // finally, emit outputs and names
        for cell in self.iter_cells() {
            if visited.contains(&cell.index) {
                continue;
            }
            result.push(cell);
        }
        result.into_iter()
    }

    pub fn compact(&mut self) -> bool {
        let did_change = self.apply();

        let mut queue = BTreeSet::new();
        for (index, cell) in self.cells.iter().enumerate() {
            if matches!(cell, Cell::Skip(_) | Cell::Void) {
                continue;
            }
            match &*cell.repr() {
                CellRepr::Iob(_)
                | CellRepr::Other(_)
                | CellRepr::Input(_, _)
                | CellRepr::Output(_, _)
                | CellRepr::Name(_, _) => {
                    queue.insert(index);
                }
                CellRepr::Target(target_cell) => {
                    if self.target_prototype(target_cell).purity == TargetCellPurity::HasEffects {
                        queue.insert(index);
                    }
                }
                _ => (),
            }
        }

        let mut keep = BTreeSet::new();
        while let Some(index) = queue.pop_first() {
            keep.insert(index);
            let cell = &self.cells[index];
            cell.visit(|net| {
                if let Ok((cell_ref, _offset)) = self.find_cell(net) {
                    if !keep.contains(&cell_ref.index) {
                        queue.insert(cell_ref.index);
                    }
                }
            });
        }

        let mut net_map = BTreeMap::new();
        for (old_index, cell) in std::mem::take(&mut self.cells).into_iter().enumerate() {
            if keep.contains(&old_index) {
                let new_index = self.cells.len();
                for offset in 0..cell.output_len() {
                    net_map.insert(Net::from_cell(old_index + offset), Net::from_cell(new_index + offset));
                }
                let skip_count = cell.output_len().checked_sub(1).unwrap_or(0);
                self.cells.push(cell);
                for _ in 0..skip_count {
                    self.cells.push(Cell::Skip(new_index as u32));
                }
            }
        }

        for cell in self.cells.iter_mut().filter(|cell| !matches!(cell, Cell::Skip(_))) {
            cell.visit_mut(|net| {
                if ![Net::UNDEF, Net::ZERO, Net::ONE].contains(net) {
                    *net = net_map[net];
                }
            });
        }

        did_change
    }

    pub fn replace_bufs(&mut self) {
        self.apply();

        for cell_ref in self.iter_cells() {
            match &*cell_ref.repr() {
                CellRepr::Buf(arg) => self.replace_value(cell_ref.output(), arg),
                _ => (),
            }
        }
    }
}

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
        match &*cell.repr() {
            CellRepr::Output(name, value) => {
                outputs_l.insert(name.clone(), value.clone());
            }
            CellRepr::Name(name, value) => {
                names_l.insert(name.clone(), value.clone());
            }
            _ => (),
        }
    }
    let mut outputs_r = BTreeMap::new();
    let mut names_r = BTreeMap::new();
    for cell in rgt.iter_cells() {
        match &*cell.repr() {
            CellRepr::Output(name, value) => {
                outputs_r.insert(name.clone(), value.clone());
            }
            CellRepr::Name(name, value) => {
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
    for name in lft.ios.keys() {
        if let (Some(io_l), Some(io_r)) = (lft.get_io(name), rgt.get_io(name)) {
            if io_l.len() != io_r.len() {
                return Err(NotIsomorphic::IoSizeMismatch(name.clone()));
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
        match (&*cell_l.repr(), &*cell_r.repr()) {
            (CellRepr::Buf(val_l), CellRepr::Buf(val_r)) | (CellRepr::Not(val_l), CellRepr::Not(val_r)) => {
                queue_vals(&mut queue, val_l, val_r)?
            }
            (CellRepr::And(arg1_l, arg2_l), CellRepr::And(arg1_r, arg2_r))
            | (CellRepr::Or(arg1_l, arg2_l), CellRepr::Or(arg1_r, arg2_r))
            | (CellRepr::Xor(arg1_l, arg2_l), CellRepr::Xor(arg1_r, arg2_r))
            | (CellRepr::Eq(arg1_l, arg2_l), CellRepr::Eq(arg1_r, arg2_r))
            | (CellRepr::ULt(arg1_l, arg2_l), CellRepr::ULt(arg1_r, arg2_r))
            | (CellRepr::SLt(arg1_l, arg2_l), CellRepr::SLt(arg1_r, arg2_r))
            | (CellRepr::Mul(arg1_l, arg2_l), CellRepr::Mul(arg1_r, arg2_r))
            | (CellRepr::UDiv(arg1_l, arg2_l), CellRepr::UDiv(arg1_r, arg2_r))
            | (CellRepr::UMod(arg1_l, arg2_l), CellRepr::UMod(arg1_r, arg2_r))
            | (CellRepr::SDivTrunc(arg1_l, arg2_l), CellRepr::SDivTrunc(arg1_r, arg2_r))
            | (CellRepr::SDivFloor(arg1_l, arg2_l), CellRepr::SDivFloor(arg1_r, arg2_r))
            | (CellRepr::SModTrunc(arg1_l, arg2_l), CellRepr::SModTrunc(arg1_r, arg2_r))
            | (CellRepr::SModFloor(arg1_l, arg2_l), CellRepr::SModFloor(arg1_r, arg2_r)) => {
                queue_vals(&mut queue, arg1_l, arg1_r)?;
                queue_vals(&mut queue, arg2_l, arg2_r)?;
            }
            (CellRepr::Mux(arg1_l, arg2_l, arg3_l), CellRepr::Mux(sel_r, arg2_r, arg3_r)) => {
                queue.insert((*arg1_l, *sel_r));
                queue_vals(&mut queue, arg2_l, arg2_r)?;
                queue_vals(&mut queue, arg3_l, arg3_r)?;
            }
            (CellRepr::Adc(arg1_l, arg2_l, arg3_l), CellRepr::Adc(arg1_r, arg2_r, arg3_r)) => {
                queue_vals(&mut queue, arg1_l, arg1_r)?;
                queue_vals(&mut queue, arg2_l, arg2_r)?;
                queue.insert((*arg3_l, *arg3_r));
            }
            (CellRepr::Shl(arg1_l, arg2_l, stride_l), CellRepr::Shl(arg1_r, arg2_r, stride_r))
            | (CellRepr::UShr(arg1_l, arg2_l, stride_l), CellRepr::UShr(arg1_r, arg2_r, stride_r))
            | (CellRepr::SShr(arg1_l, arg2_l, stride_l), CellRepr::SShr(arg1_r, arg2_r, stride_r))
            | (CellRepr::XShr(arg1_l, arg2_l, stride_l), CellRepr::XShr(arg1_r, arg2_r, stride_r)) => {
                queue_vals(&mut queue, arg1_l, arg1_r)?;
                queue_vals(&mut queue, arg2_l, arg2_r)?;
                if stride_l != stride_r {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
            }
            (CellRepr::Dff(ff_l), CellRepr::Dff(ff_r)) => {
                queue_vals(&mut queue, &ff_l.data, &ff_r.data)?;
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
                {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
            }
            (CellRepr::Iob(iob_l), CellRepr::Iob(iob_r)) => {
                for (io_net_l, io_net_r) in iob_l.io.iter().zip(iob_r.io.iter()) {
                    if !ios.contains(&(io_net_l, io_net_r)) {
                        return Err(NotIsomorphic::IoNetMismatch(io_net_l, io_net_r));
                    }
                }
                queue_vals(&mut queue, &iob_l.output, &iob_r.output)?;
                queue.insert((iob_l.enable.net(), iob_r.enable.net()));
                if iob_l.enable.is_positive() != iob_r.enable.is_positive() {
                    return Err(NotIsomorphic::NetMismatch(net_l, net_r));
                }
            }
            (CellRepr::Target(target_cell_l), CellRepr::Target(target_cell_r)) => {
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
            (CellRepr::Other(inst_l), CellRepr::Other(inst_r)) => {
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
            (CellRepr::Input(name_l, _), CellRepr::Input(name_r, _)) => {
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
