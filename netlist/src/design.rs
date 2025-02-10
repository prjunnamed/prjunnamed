use std::fmt::Display;
use std::ops::Range;
use std::cell::RefCell;
use std::borrow::Cow;
use std::hash::Hash;
use std::collections::{btree_map, BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use crate::cell::CellRepr;
use crate::smt::{SmtEngine, SmtBuilder};
use crate::{
    Trit, Net, Value, ControlNet, IoNet, IoValue, Cell, AssignCell, MatchCell, FlipFlop, Instance, IoBuffer, Memory,
    Target, TargetCell, TargetCellPurity, TargetPrototype,
};

/// Sea of [`Cell`]s.
#[derive(Debug, Clone)]
pub struct Design {
    ios: BTreeMap<String, Range<u32>>,
    cells: Vec<CellRepr>,
    changes: RefCell<ChangeQueue>,
    target: Option<Arc<dyn Target>>,
}

#[derive(Debug, Clone)]
struct ChangeQueue {
    next_io: u32,
    added_ios: BTreeMap<String, Range<u32>>,
    added_cells: Vec<CellRepr>,
    cell_cache: HashMap<Cell, Value>,
    replaced_cells: BTreeMap<usize, Cell>,
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
                cell_cache: HashMap::new(),
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

    pub fn add_cell(&self, cell: Cell) -> Value {
        let mut changes = self.changes.borrow_mut();
        if let Some(value) = changes.cell_cache.get(&cell) {
            return value.clone();
        }
        cell.validate(self);
        let index = self.cells.len() + changes.added_cells.len();
        let output_len = cell.output_len();
        changes.added_cells.push(cell.clone().into());
        for _ in 0..output_len.checked_sub(1).unwrap_or(0) {
            changes.added_cells.push(CellRepr::Skip(index.try_into().expect("cell index too large")))
        }
        let output = Value::cell(index, output_len);
        if !cell.has_effects(self) {
            changes.cell_cache.insert(cell, output.clone());
        }
        output
    }

    pub fn add_void(&self, width: usize) -> Value {
        let mut changes = self.changes.borrow_mut();
        let index = self.cells.len() + changes.added_cells.len();
        for _ in 0..width {
            changes.added_cells.push(CellRepr::Void);
        }
        Value::cell(index, width)
    }

    fn locate_cell(&self, net: Net) -> Result<(usize, usize), Trit> {
        if let Some(trit) = net.as_const() {
            return Err(trit);
        }
        let index = net.as_cell_index().unwrap();
        let (cell_index, bit_index) = match self.cells[index] {
            CellRepr::Void => panic!("located a void cell %{index} in design"),
            CellRepr::Skip(start) => (start as usize, index - start as usize),
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
        let mut mapped_net = net;
        while let Some(new_net) = changes.replaced_nets.get(&mapped_net) {
            mapped_net = *new_net;
        }
        // Assume the caller might want to locate the cell behind the net.
        match mapped_net.as_cell_index() {
            Ok(index) if index >= self.cells.len() => net,
            _ => mapped_net,
        }
    }

    pub fn map_value(&self, value: impl Into<Value>) -> Value {
        let mut value = value.into();
        value.visit_mut(|net| *net = self.map_net(*net));
        value
    }

    pub fn is_changed(&self) -> bool {
        let changes = self.changes.borrow();
        changes.added_ios.len()
            + changes.added_cells.len()
            + changes.replaced_cells.len()
            + changes.unalived_cells.len()
            + changes.replaced_nets.len()
            > 0
    }

    pub fn verify<SMT: SmtEngine>(&self, engine: SMT) -> Result<(), SMT::Error> {
        let changes = self.changes.borrow();
        let locate_cell = |net: Net| {
            net.as_cell_index().map(|index| {
                if index < self.cells.len() {
                    &self.cells[index]
                } else {
                    &changes.added_cells[index - self.cells.len()]
                }
            })
        };
        let get_cell = |net: Net| match locate_cell(net) {
            Ok(CellRepr::Skip(index)) => locate_cell(Net::from_cell_index(*index as usize)),
            result => result,
        };

        let mut smt = SmtBuilder::new(self, engine);
        for (index, cell) in self.cells.iter().chain(changes.added_cells.iter()).enumerate() {
            if matches!(cell, CellRepr::Skip(_) | CellRepr::Void) {
            } else if cell.output_len() == 0 {
            } else if let Some(new_cell) = changes.replaced_cells.get(&index) {
                smt.replace_cell(&Value::cell(index, cell.output_len()), &*cell.get(), new_cell)?;
            } else {
                smt.add_cell(&Value::cell(index, cell.output_len()), &*cell.get())?;
            }
        }
        for (&net, &new_net) in changes.replaced_nets.iter() {
            if let Ok(cell) = get_cell(net) {
                if matches!(cell, CellRepr::Void) {
                    smt.replace_void_net(net, new_net)?;
                    continue;
                } else if matches!(&*cell.get(), Cell::Dff(_)) {
                    if let Ok(new_cell) = get_cell(new_net) {
                        if matches!(&*new_cell.get(), Cell::Dff(_)) {
                            smt.replace_dff_net(net, new_net)?;
                        }
                    }
                }
            }
            smt.replace_net(net, new_net)?;
        }
        if let Some(example) = smt.check()? {
            let mut message = format!("verification failed!\n");
            message.push_str(&format!("\ndesign:\n{self:#}"));
            message.push_str("\ncounterexample:\n");
            for (index, cell) in self.cells.iter().chain(changes.added_cells.iter()).enumerate() {
                if matches!(cell, CellRepr::Skip(_) | CellRepr::Void) {
                } else if cell.output_len() == 0 {
                } else {
                    let output = Value::cell(index, cell.output_len());
                    let (was, now) = (example.get_past_value(&output), example.get_value(&output));
                    message.push_str(&match (was, now) {
                        (Some(was), Some(now)) => format!("{} = {} -> {}\n", self.display_value(&output), was, now),
                        (None, Some(now)) => format!("{} = {}\n", self.display_value(&output), now),
                        (Some(was), None) => format!("{} = {} -> ?\n", self.display_value(&output), was),
                        (None, None) => unreachable!(),
                    });
                }
            }
            for (&net, &new_net) in changes.replaced_nets.iter() {
                if example.get_value(net) != example.get_value(new_net) {
                    message.push_str(&format!(
                        "\npossible cause: replacing net {} with net {} is not valid",
                        self.display_net(net),
                        self.display_net(new_net)
                    ));
                }
            }
            panic!("{message}");
        }
        Ok(())
    }

    pub fn apply(&mut self) -> bool {
        #[cfg(feature = "verify")]
        self.verify(crate::EasySmtEngine::z3().unwrap()).unwrap();

        let changes = self.changes.get_mut();
        let mut did_change = !changes.added_ios.is_empty() || !changes.added_cells.is_empty();
        for cell_index in std::mem::take(&mut changes.unalived_cells) {
            let output_len = self.cells[cell_index].output_len().max(1);
            for index in cell_index..cell_index + output_len {
                self.cells[index] = CellRepr::Void;
            }
            did_change = true;
        }
        for (index, new_cell) in std::mem::take(&mut changes.replaced_cells) {
            assert_eq!(self.cells[index].output_len(), new_cell.output_len());
            // CellRef::replace() ensures the new cell is different.
            self.cells[index] = new_cell.into();
            did_change = true;
        }
        self.ios.extend(std::mem::take(&mut changes.added_ios));
        self.cells.extend(std::mem::take(&mut changes.added_cells));
        changes.cell_cache.clear();
        if !changes.replaced_nets.is_empty() {
            for cell in self.cells.iter_mut().filter(|cell| !matches!(cell, CellRepr::Skip(_) | CellRepr::Void)) {
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

impl PartialOrd<CellRef<'_>> for CellRef<'_> {
    fn partial_cmp(&self, other: &CellRef<'_>) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CellRef<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.design as *const Design).cmp(&(other.design as *const Design)) {
            core::cmp::Ordering::Equal => self.index.cmp(&other.index),
            ord => ord,
        }
    }
}

impl Hash for CellRef<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

impl<'a> CellRef<'a> {
    pub fn get(&self) -> Cow<'a, Cell> {
        self.design.cells[self.index].get()
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

    pub fn replace(&self, to_cell: Cell) {
        if *self.design.cells[self.index].get() != to_cell {
            to_cell.validate(self.design);
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
        while matches!(self.design.cells.get(self.index), Some(CellRepr::Void)) {
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

    ($func:ident( $($arg:ident : $argty:ty),+ ) -> $retty:ty : $cstr:ident $body:tt; $($rest:tt)*) => {
        pub fn $func(&self, $( $arg: $argty ),+) -> $retty {
            self.add_cell(Cell::$cstr $body).try_into().unwrap()
        }

        builder_fn!{ $($rest)* }
    };

    // For cells with no output value.
    ($func:ident( $($arg:ident : $argty:ty),+ ) : $cstr:ident $body:tt; $($rest:tt)*) => {
        pub fn $func(&self, $( $arg: $argty ),+) {
            self.add_cell(Cell::$cstr $body);
        }

        builder_fn!{ $($rest)* }
    };
}

impl Design {
    builder_fn! {
        add_buf(arg: impl Into<Value>) -> Value :
            Buf(arg.into());
        add_buf1(arg: impl Into<Net>) -> Net :
            Buf(arg.into().into());
        add_not(arg: impl Into<Value>) -> Value :
            Not(arg.into());
        add_not1(arg: impl Into<Net>) -> Net :
            Not(arg.into().into());
        add_and(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            And(arg1.into(), arg2.into());
        add_and1(arg1: impl Into<Net>, arg2: impl Into<Net>) -> Net :
            And(arg1.into().into(), arg2.into().into());
        add_or(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            Or(arg1.into(), arg2.into());
        add_or1(arg1: impl Into<Net>, arg2: impl Into<Net>) -> Net :
            Or(arg1.into().into(), arg2.into().into());
        add_xor(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            Xor(arg1.into(), arg2.into());
        add_xor1(arg1: impl Into<Net>, arg2: impl Into<Net>) -> Net :
            Xor(arg1.into().into(), arg2.into().into());
        add_adc(arg1: impl Into<Value>, arg2: impl Into<Value>, arg3: impl Into<Net>) -> Value :
            Adc(arg1.into(), arg2.into(), arg3.into());

        add_eq(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Net :
            Eq(arg1.into(), arg2.into());
        add_ult(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Net :
            ULt(arg1.into(), arg2.into());
        add_slt(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Net :
            SLt(arg1.into(), arg2.into());

        add_shl(arg1: impl Into<Value>, arg2: impl Into<Value>, stride: u32) -> Value :
            Shl(arg1.into(), arg2.into(), stride);
        add_ushr(arg1: impl Into<Value>, arg2: impl Into<Value>, stride: u32) -> Value :
            UShr(arg1.into(), arg2.into(), stride);
        add_sshr(arg1: impl Into<Value>, arg2: impl Into<Value>, stride: u32) -> Value :
            SShr(arg1.into(), arg2.into(), stride);
        add_xshr(arg1: impl Into<Value>, arg2: impl Into<Value>, stride: u32) -> Value :
            XShr(arg1.into(), arg2.into(), stride);

        add_mul(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            Mul(arg1.into(), arg2.into());
        add_udiv(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            UDiv(arg1.into(), arg2.into());
        add_umod(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            UMod(arg1.into(), arg2.into());
        add_sdiv_trunc(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            SDivTrunc(arg1.into(), arg2.into());
        add_sdiv_floor(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            SDivFloor(arg1.into(), arg2.into());
        add_smod_trunc(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            SModTrunc(arg1.into(), arg2.into());
        add_smod_floor(arg1: impl Into<Value>, arg2: impl Into<Value>) -> Value :
            SModFloor(arg1.into(), arg2.into());

        add_match(arg: impl Into<MatchCell>) -> Value :
            Match(arg.into());
        add_assign(arg: impl Into<AssignCell>) -> Value :
            Assign(arg.into());
        add_dff(arg: impl Into<FlipFlop>) -> Value :
            Dff(arg.into());
        add_memory(arg: impl Into<Memory>) -> Value :
            Memory(arg.into());
        add_iobuf(arg: impl Into<IoBuffer>) -> Value :
            IoBuf(arg.into());
        add_other(arg: impl Into<Instance>) -> Value :
            Other(arg.into());
        add_target(arg: impl Into<TargetCell>) -> Value :
            Target(arg.into());

        add_input(name: impl Into<String>, width: usize) -> Value :
            Input(name.into(), width);
        add_input1(name: impl Into<String>) -> Net :
            Input(name.into(), 1);
        add_output(name: impl Into<String>, value: impl Into<Value>) :
            Output(name.into(), value.into());
        add_name(name: impl Into<String>, value: impl Into<Value>) :
            Name(name.into(), value.into());
        add_debug(name: impl Into<String>, value: impl Into<Value>) :
            Debug(name.into(), value.into());
    }

    pub fn add_mux(&self, arg1: impl Into<ControlNet>, arg2: impl Into<Value>, arg3: impl Into<Value>) -> Value {
        match arg1.into() {
            ControlNet::Pos(net) => self.add_cell(Cell::Mux(net, arg2.into(), arg3.into())),
            ControlNet::Neg(net) => self.add_cell(Cell::Mux(net, arg3.into(), arg2.into())),
        }
    }

    pub fn add_mux1(&self, arg1: impl Into<ControlNet>, arg2: impl Into<Net>, arg3: impl Into<Net>) -> Net {
        match arg1.into() {
            ControlNet::Pos(net) => self.add_cell(Cell::Mux(net, arg2.into().into(), arg3.into().into())).unwrap_net(),
            ControlNet::Neg(net) => self.add_cell(Cell::Mux(net, arg3.into().into(), arg2.into().into())).unwrap_net(),
        }
    }

    pub fn add_ne(&self, arg1: impl Into<Value>, arg2: impl Into<Value>) -> Net {
        let eq = self.add_eq(arg1, arg2);
        self.add_not1(eq)
    }
}

impl Design {
    pub fn iter_cells_topo(&self) -> impl DoubleEndedIterator<Item = CellRef<'_>> {
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
            match &*cell.get() {
                Cell::Input(..) | Cell::IoBuf(..) | Cell::Dff(..) | Cell::Other(..) => {
                    visited.insert(cell.index);
                    result.push(cell);
                }
                Cell::Target(target_cell) => {
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
            if matches!(&*cell.get(), Cell::Output(..) | Cell::Name(..) | Cell::Debug(..)) {
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
        // finally, emit outputs, names, and debugs
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
        let mut debug = BTreeMap::new();
        for (index, cell_repr) in self.cells.iter().enumerate() {
            if matches!(cell_repr, CellRepr::Skip(_) | CellRepr::Void) {
                continue;
            }
            let cell = &*cell_repr.get();
            if cell.has_effects(self) {
                queue.insert(index);
            } else if let Cell::Debug(name, value) = cell {
                debug.insert(name.clone(), value.clone());
            }
        }

        let mut keep = BTreeSet::new();
        while let Some(index) = queue.pop_first() {
            keep.insert(index);
            self.cells[index].visit(|net| {
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
                    net_map.insert(Net::from_cell_index(old_index + offset), Net::from_cell_index(new_index + offset));
                }
                let skip_count = cell.output_len().checked_sub(1).unwrap_or(0);
                self.cells.push(cell);
                for _ in 0..skip_count {
                    self.cells.push(CellRepr::Skip(new_index as u32));
                }
            }
        }

        for cell in self.cells.iter_mut().filter(|cell| !matches!(cell, CellRepr::Skip(_))) {
            cell.visit_mut(|net| {
                if net.is_cell() {
                    *net = net_map[net];
                }
            });
        }

        for (name, mut value) in debug {
            value.visit_mut(|net| {
                if net.is_cell() {
                    if let Some(&new_net) = net_map.get(net) {
                        *net = new_net;
                    } else {
                        *net = Net::UNDEF;
                    }
                }
            });
            self.cells.push(CellRepr::Coarse(Box::new(Cell::Debug(name, value))));
        }

        did_change
    }

    pub fn statistics(&self) -> BTreeMap<String, usize> {
        let result = RefCell::new(BTreeMap::<String, usize>::new());
        for cell_ref in self.iter_cells() {
            let fine = |name: &str, amount: usize| {
                *result.borrow_mut().entry(format!("{name}")).or_default() += amount;
            };
            let coarse = |name: &str, size: usize| {
                *result.borrow_mut().entry(format!("{name}:{size}")).or_default() += 1;
            };
            let custom = |args: std::fmt::Arguments| {
                *result.borrow_mut().entry(format!("{args}")).or_default() += 1;
            };
            match &*cell_ref.get() {
                Cell::Buf(arg) => fine("buf", arg.len()),
                Cell::Not(arg) => fine("not", arg.len()),
                Cell::And(arg, _) => fine("and", arg.len()),
                Cell::Or(arg, _) => fine("or", arg.len()),
                Cell::Xor(arg, _) => fine("xor", arg.len()),
                Cell::Mux(_, arg, _) => fine("mux", arg.len()),
                Cell::Adc(arg, _, _) => coarse("adc", arg.len()),
                Cell::Eq(arg, _) => coarse("eq", arg.len()),
                Cell::ULt(arg, _) => coarse("ult", arg.len()),
                Cell::SLt(arg, _) => coarse("slt", arg.len()),
                Cell::Shl(arg, _, _) => coarse("shl", arg.len()),
                Cell::UShr(arg, _, _) => coarse("ushr", arg.len()),
                Cell::SShr(arg, _, _) => coarse("sshr", arg.len()),
                Cell::XShr(arg, _, _) => coarse("xshr", arg.len()),
                Cell::Mul(arg, _) => coarse("mul", arg.len()),
                Cell::UDiv(arg, _) => coarse("udiv", arg.len()),
                Cell::UMod(arg, _) => coarse("umod", arg.len()),
                Cell::SDivTrunc(arg, _) => coarse("sdiv_trunc", arg.len()),
                Cell::SDivFloor(arg, _) => coarse("sdiv_floor", arg.len()),
                Cell::SModTrunc(arg, _) => coarse("smod_trunc", arg.len()),
                Cell::SModFloor(arg, _) => coarse("smod_floor", arg.len()),
                Cell::Match(_) => custom(format_args!("match")),
                Cell::Assign(AssignCell { value, .. }) => fine("assign", value.len()),
                Cell::Dff(FlipFlop { data, .. }) => fine("dff", data.len()),
                Cell::Memory(Memory { depth, width, .. }) => custom(format_args!("memory:{depth}:{width}")),
                Cell::IoBuf(IoBuffer { io, .. }) => fine("iobuf", io.len()),
                Cell::Target(TargetCell { kind, .. }) => custom(format_args!("{kind}")),
                Cell::Other(Instance { kind, .. }) => custom(format_args!("{kind}")),
                Cell::Input(_, width) => fine("input", *width),
                Cell::Output(_, value) => fine("output", value.len()),
                Cell::Name(..) | Cell::Debug(..) => (),
            }
        }
        result.into_inner()
    }
}

// This can't be in `crate::print` because of the privacy violations.
impl Display for Design {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let changes = self.changes.borrow();

        let diff = self.is_changed();
        let added = "+";
        let removed = "-";
        let unchanged = " ";

        let mut net_names = BTreeMap::new();
        for cell_ref in self.iter_cells() {
            match &*cell_ref.get() {
                Cell::Output(name, value) | Cell::Name(name, value) | Cell::Debug(name, value) => {
                    let name = Rc::new(name.clone());
                    for (offset, net) in value.iter().enumerate() {
                        if net.is_cell() {
                            net_names.insert(net, (name.clone(), offset));
                        }
                    }
                }
                _ => ()
            }
        }

        if let Some(target) = self.target() {
            write!(f, "{}target ", if !diff { "" } else { unchanged })?;
            self.write_string(f, target.name())?;
            for (name, value) in target.options() {
                write!(f, " ")?;
                self.write_string(f, &name)?;
                write!(f, "=")?;
                self.write_string(f, &value)?;
            }
            writeln!(f)?;
        }

        for (name, io_value) in self.iter_ios() {
            write!(f, "{}&", if !diff { "" } else { unchanged })?;
            self.write_string(f, name)?;
            writeln!(f, ":{}", io_value.len())?;
        }
        for (name, io_value) in &changes.added_ios {
            write!(f, "{added}&")?;
            self.write_string(f, name)?;
            writeln!(f, ":{}", io_value.len())?;
        }

        let write_cell = |f: &mut std::fmt::Formatter, index: usize, cell: &Cell| {
            if cell.output_len() == 1 && !matches!(cell, Cell::Input(..)) {
                if let Some((name, offset)) = net_names.get(&Net::from_cell_index(index)) {
                    write!(f, "{}; ", if !diff { "" } else { unchanged })?;
                    self.write_string(f, &*name)?;
                    writeln!(f, "+{offset}")?;
                }
            }
            if !diff {
                write!(f, "%{}:{} = ", index, cell.output_len())?;
                self.write_cell(f, cell, "")?;
                writeln!(f)
            } else if changes.unalived_cells.contains(&index) {
                write!(f, "{removed}%{}:{} = ", index, cell.output_len())?;
                self.write_cell(f, cell, "")?;
                writeln!(f)
            } else {
                let mut mapped_cell;
                if let Some(replaced_cell) = changes.replaced_cells.get(&index) {
                    mapped_cell = replaced_cell.clone();
                } else {
                    mapped_cell = cell.clone();
                }
                mapped_cell.visit_mut(|net| {
                    while let Some(&new_net) = changes.replaced_nets.get(net) {
                        *net = new_net;
                    }
                });
                if index >= self.cells.len() {
                    write!(f, "{added}%_{}:{} = ", index, mapped_cell.output_len())?;
                    self.write_cell(f, &mapped_cell, added)?;
                    writeln!(f)
                } else if mapped_cell != *cell {
                    write!(f, "{removed}%{}:{} = ", index, cell.output_len())?;
                    self.write_cell(f, cell, removed)?;
                    writeln!(f)?;
                    write!(f, "{added}%{}:{} = ", index, mapped_cell.output_len())?;
                    self.write_cell(f, &mapped_cell, added)?;
                    writeln!(f)
                } else {
                    write!(f, "{unchanged}%{}:{} = ", index, cell.output_len())?;
                    self.write_cell(f, cell, unchanged)?;
                    writeln!(f)
                }
            }
        };

        if f.alternate() {
            for cell_ref in self.iter_cells() {
                write_cell(f, cell_ref.index, &*cell_ref.get())?;
            }
        } else {
            for cell_ref in self.iter_cells_topo() {
                write_cell(f, cell_ref.index, &*cell_ref.get())?;
            }
        }
        for (offset, cell_repr) in changes.added_cells.iter().enumerate() {
            if !matches!(cell_repr, CellRepr::Skip(_) | CellRepr::Void) {
                write_cell(f, self.cells.len() + offset, &*cell_repr.get())?;
            }
        }

        Ok(())
    }
}
