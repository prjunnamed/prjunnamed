use std::fmt::Display;
use std::ops::{Deref, Range};
use std::cell::{Ref, RefCell};
use std::borrow::Cow;
use std::hash::Hash;
use std::collections::{btree_map, BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use crate::{MetaItem, MetaStringRef, MetaItemRef};
use crate::{
    cell::CellRepr, AssignCell, Cell, ControlNet, FlipFlop, Instance, IoBuffer, IoNet, IoValue, MatchCell, Memory, Net,
    Target, TargetCell, TargetCellPurity, TargetPrototype, Trit, Value,
};
use crate::metadata::{MetadataStore, MetaStringIndex, MetaItemIndex};
use crate::smt::{SmtEngine, SmtBuilder};

/// Sea of [`Cell`]s.
#[derive(Debug, Clone)]
pub struct Design {
    ios: BTreeMap<String, Range<u32>>,
    cells: Vec<AnnotatedCell>,
    changes: RefCell<ChangeQueue>,
    metadata: RefCell<MetadataStore>,
    target: Option<Arc<dyn Target>>,
}

#[derive(Debug, Clone, Default)]
struct ChangeQueue {
    next_io: u32,
    added_ios: BTreeMap<String, Range<u32>>,
    added_cells: Vec<AnnotatedCell>,
    cell_cache: HashMap<AnnotatedCell, Value>,
    cell_metadata: MetaItemIndex,
    appended_metadata: BTreeMap<usize, Vec<MetaItemIndex>>,
    replaced_cells: BTreeMap<usize, AnnotatedCell>,
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
            changes: RefCell::new(ChangeQueue::default()),
            metadata: RefCell::new(MetadataStore::new()),
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

    pub(crate) fn add_cell_with_metadata_index(&self, cell: Cell, metadata: MetaItemIndex) -> Value {
        cell.validate(self);
        let cell_with_meta = AnnotatedCell { repr: cell.clone().into(), meta: metadata };
        let mut changes = self.changes.borrow_mut();
        if let Some(value) = changes.cell_cache.get(&cell_with_meta) {
            value.clone()
        } else {
            let index = self.cells.len() + changes.added_cells.len();
            let output_len = cell.output_len();
            let output = Value::from_cell_range(index, output_len);
            if !cell.has_effects(self) {
                changes.cell_cache.insert(cell_with_meta.clone(), output.clone());
            }
            changes.added_cells.push(cell_with_meta);
            for _ in 0..output_len.checked_sub(1).unwrap_or(0) {
                changes.added_cells.push(CellRepr::Skip(index.try_into().expect("cell index too large")).into())
            }
            output
        }
    }

    pub fn add_cell_with_metadata_ref(&self, cell: Cell, metadata: MetaItemRef) -> Value {
        self.add_cell_with_metadata_index(cell, metadata.index())
    }

    pub fn add_cell_with_metadata(&self, cell: Cell, metadata: &MetaItem) -> Value {
        metadata.validate();
        let metadata = self.metadata.borrow_mut().add_item(metadata);
        self.add_cell_with_metadata_index(cell, metadata)
    }

    pub fn use_metadata_from(&self, cell_refs: &[CellRef]) -> WithMetadataGuard {
        let mut changes = self.changes.borrow_mut();
        let guard = WithMetadataGuard { design: self, restore: changes.cell_metadata };
        changes.cell_metadata = MetaItemRef::from_merge(self, cell_refs.iter().map(CellRef::metadata)).index();
        guard
    }

    pub fn add_cell(&self, cell: Cell) -> Value {
        let metadata = self.changes.borrow().cell_metadata;
        self.add_cell_with_metadata_index(cell, metadata)
    }

    pub fn add_void(&self, width: usize) -> Value {
        let mut changes = self.changes.borrow_mut();
        let index = self.cells.len() + changes.added_cells.len();
        for _ in 0..width {
            changes.added_cells.push(CellRepr::Void.into());
        }
        Value::from_cell_range(index, width)
    }

    #[inline]
    pub fn find_cell(&self, net: Net) -> Result<(CellRef, usize), Trit> {
        let index = net.as_cell_index()?;
        match self.cells[index].repr {
            CellRepr::Void => panic!("located a void cell %{index} in design"),
            CellRepr::Skip(start) => Ok((CellRef { design: self, index: start as usize }, index - start as usize)),
            _ => Ok((CellRef { design: self, index }, 0)),
        }
    }

    pub fn iter_cells(&self) -> CellIter {
        CellIter { design: self, index: 0 }
    }

    pub(crate) fn is_valid_cell_index(&self, index: usize) -> bool {
        index < self.cells.len()
    }

    pub(crate) fn metadata(&self) -> Ref<MetadataStore> {
        self.metadata.borrow()
    }

    pub(crate) fn add_metadata_string(&self, string: &str) -> MetaStringIndex {
        self.metadata.borrow_mut().add_string(string)
    }

    pub(crate) fn ref_metadata_string(&self, index: MetaStringIndex) -> MetaStringRef {
        self.metadata.borrow().ref_string(self, index)
    }

    pub(crate) fn add_metadata_item(&self, item: &MetaItem) -> MetaItemIndex {
        item.validate();
        self.metadata.borrow_mut().add_item(item)
    }

    pub(crate) fn ref_metadata_item(&self, index: MetaItemIndex) -> MetaItemRef {
        self.metadata.borrow().ref_item(self, index)
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

    pub fn is_empty(&self) -> bool {
        self.ios.is_empty() && self.cells.is_empty() && !self.is_changed() && self.target.is_none()
    }

    pub fn is_changed(&self) -> bool {
        let changes = self.changes.borrow();
        !changes.added_ios.is_empty()
            || !changes.added_cells.is_empty()
            || !changes.replaced_cells.is_empty()
            || !changes.unalived_cells.is_empty()
            || !changes.replaced_nets.is_empty()
    }

    pub fn verify<SMT: SmtEngine>(&self, engine: SMT) -> Result<(), SMT::Error> {
        let changes = self.changes.borrow();
        let locate_cell = |net: Net| {
            net.as_cell_index().map(|index| {
                if index < self.cells.len() {
                    &self.cells[index].repr
                } else {
                    &changes.added_cells[index - self.cells.len()].repr
                }
            })
        };
        let get_cell = |net: Net| match locate_cell(net) {
            Ok(CellRepr::Skip(index)) => locate_cell(Net::from_cell_index(*index as usize)),
            result => result,
        };

        let mut smt = SmtBuilder::new(self, engine);
        for (index, cell) in self.cells.iter().chain(changes.added_cells.iter()).enumerate() {
            if matches!(cell.repr, CellRepr::Skip(_) | CellRepr::Void) {
            } else if cell.output_len() == 0 {
            } else if let Some(new_cell) = changes.replaced_cells.get(&index) {
                smt.replace_cell(&Value::from_cell_range(index, cell.output_len()), &*cell.get(), &*new_cell.get())?;
            } else {
                smt.add_cell(&Value::from_cell_range(index, cell.output_len()), &*cell.get())?;
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
                if matches!(cell.repr, CellRepr::Skip(_) | CellRepr::Void) {
                } else if cell.output_len() == 0 {
                } else {
                    let output = Value::from_cell_range(index, cell.output_len());
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

        let mut changes = std::mem::take(self.changes.get_mut());
        self.changes.get_mut().next_io = changes.next_io;

        let mut did_change = !changes.added_ios.is_empty() || !changes.added_cells.is_empty();
        for (cell_index, new_items) in changes.appended_metadata {
            let cell_meta_iter = self.ref_metadata_item(self.cells[cell_index].meta).iter();
            let new_items_iter = new_items.into_iter().flat_map(|new_item| self.ref_metadata_item(new_item).iter());
            self.cells[cell_index].meta = MetaItemRef::from_iter(self, cell_meta_iter.chain(new_items_iter)).index();
        }
        for cell_index in changes.unalived_cells {
            let output_len = self.cells[cell_index].output_len().max(1);
            for index in cell_index..cell_index + output_len {
                self.cells[index] = CellRepr::Void.into();
            }
            did_change = true;
        }
        for (index, new_cell) in changes.replaced_cells {
            assert_eq!(self.cells[index].output_len(), new_cell.output_len());
            self.cells[index] = new_cell;
            // CellRef::replace() ensures the new cell is different.
            did_change = true;
        }
        self.ios.extend(changes.added_ios);
        self.cells.extend(changes.added_cells);
        changes.cell_cache.clear();
        if !changes.replaced_nets.is_empty() {
            for cell in self.cells.iter_mut().filter(|cell| !matches!(cell.repr, CellRepr::Skip(_) | CellRepr::Void)) {
                cell.repr.visit_mut(|net| {
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
        self.target
            .as_ref()
            .expect("design has no target")
            .prototype(&target_cell.kind)
            .expect("target prototype not defined")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AnnotatedCell {
    repr: CellRepr,
    meta: MetaItemIndex,
}

impl Deref for AnnotatedCell {
    type Target = CellRepr;

    fn deref(&self) -> &Self::Target {
        &self.repr
    }
}

impl Into<AnnotatedCell> for CellRepr {
    fn into(self) -> AnnotatedCell {
        AnnotatedCell { repr: self, meta: MetaItemIndex::NONE }
    }
}

pub struct WithMetadataGuard<'a> {
    design: &'a Design,
    restore: MetaItemIndex,
}

impl Drop for WithMetadataGuard<'_> {
    fn drop(&mut self) {
        self.design.changes.borrow_mut().cell_metadata = self.restore;
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
    pub fn get(&self) -> Cow<Cell> {
        self.design.cells[self.index].get()
    }

    pub fn metadata(&self) -> MetaItemRef<'a> {
        self.design.metadata.borrow().ref_item(self.design, self.design.cells[self.index].meta)
    }

    pub fn output_len(&self) -> usize {
        self.design.cells[self.index].output_len()
    }

    pub fn output(&self) -> Value {
        Value::from_cell_range(self.index, self.output_len())
    }

    pub fn visit(&self, f: impl FnMut(Net)) {
        self.design.cells[self.index].visit(f)
    }

    pub fn replace(&self, to_cell: Cell) {
        if *self.design.cells[self.index].get() != to_cell {
            to_cell.validate(self.design);
            let to_cell = AnnotatedCell { repr: to_cell.into(), meta: self.design.cells[self.index].meta };
            let mut changes = self.design.changes.borrow_mut();
            assert!(changes.replaced_cells.insert(self.index, to_cell).is_none());
        }
    }

    pub fn append_metadata(&self, metadata: MetaItemRef<'a>) {
        let mut changes = self.design.changes.borrow_mut();
        changes.appended_metadata.entry(self.index).or_default().push(metadata.index())
    }

    pub fn unalive(&self) {
        let mut changes = self.design.changes.borrow_mut();
        changes.unalived_cells.insert(self.index);
    }

    // Returns the same index as the one used by `Display` implementation. There is intentionally no way to retrieve
    // a cell by its index.
    pub fn debug_index(&self) -> usize {
        self.index
    }

    pub(crate) fn metadata_index(&self) -> MetaItemIndex {
        self.design.cells[self.index].meta
    }
}

pub struct CellIter<'a> {
    design: &'a Design,
    index: usize,
}

impl<'a> Iterator for CellIter<'a> {
    type Item = CellRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while matches!(self.design.cells.get(self.index), Some(AnnotatedCell { repr: CellRepr::Void, .. })) {
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
        for (index, cell) in self.cells.iter().enumerate() {
            if matches!(cell.repr, CellRepr::Skip(_) | CellRepr::Void) {
                continue;
            }
            match &*cell.get() {
                cell if cell.has_effects(self) => {
                    queue.insert(index);
                }
                Cell::Debug(name, value) => {
                    debug.insert(name.clone(), (value.clone(), cell.meta));
                }
                _ => (),
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
                    self.cells.push(CellRepr::Skip(new_index as u32).into());
                }
            }
        }

        for cell in self.cells.iter_mut().filter(|cell| !matches!(cell.repr, CellRepr::Skip(_))) {
            cell.repr.visit_mut(|net| {
                if net.is_cell() {
                    *net = net_map[net];
                }
            });
        }

        for (name, (mut value, meta)) in debug {
            value.visit_mut(|net| {
                if net.is_cell() {
                    if let Some(&new_net) = net_map.get(net) {
                        *net = new_net;
                    } else {
                        *net = Net::UNDEF;
                    }
                }
            });
            self.cells.push(AnnotatedCell { repr: CellRepr::Boxed(Box::new(Cell::Debug(name, value))), meta });
        }

        did_change
    }

    pub fn statistics(&self) -> BTreeMap<String, usize> {
        let result = RefCell::new(BTreeMap::<String, usize>::new());
        for cell_ref in self.iter_cells() {
            let bitwise = |name: &str, amount: usize| {
                *result.borrow_mut().entry(format!("{name}")).or_default() += amount;
            };
            let wide = |name: &str, size: usize| {
                *result.borrow_mut().entry(format!("{name}:{size}")).or_default() += 1;
            };
            let custom = |args: std::fmt::Arguments| {
                *result.borrow_mut().entry(format!("{args}")).or_default() += 1;
            };
            match &*cell_ref.get() {
                Cell::Buf(arg) => bitwise("buf", arg.len()),
                Cell::Not(arg) => bitwise("not", arg.len()),
                Cell::And(arg, _) => bitwise("and", arg.len()),
                Cell::Or(arg, _) => bitwise("or", arg.len()),
                Cell::Xor(arg, _) => bitwise("xor", arg.len()),
                Cell::Mux(_, arg, _) => bitwise("mux", arg.len()),
                Cell::Adc(arg, _, _) => wide("adc", arg.len()),
                Cell::Eq(arg, _) => wide("eq", arg.len()),
                Cell::ULt(arg, _) => wide("ult", arg.len()),
                Cell::SLt(arg, _) => wide("slt", arg.len()),
                Cell::Shl(arg, _, _) => wide("shl", arg.len()),
                Cell::UShr(arg, _, _) => wide("ushr", arg.len()),
                Cell::SShr(arg, _, _) => wide("sshr", arg.len()),
                Cell::XShr(arg, _, _) => wide("xshr", arg.len()),
                Cell::Mul(arg, _) => wide("mul", arg.len()),
                Cell::UDiv(arg, _) => wide("udiv", arg.len()),
                Cell::UMod(arg, _) => wide("umod", arg.len()),
                Cell::SDivTrunc(arg, _) => wide("sdiv_trunc", arg.len()),
                Cell::SDivFloor(arg, _) => wide("sdiv_floor", arg.len()),
                Cell::SModTrunc(arg, _) => wide("smod_trunc", arg.len()),
                Cell::SModFloor(arg, _) => wide("smod_floor", arg.len()),
                Cell::Match(_) => custom(format_args!("match")),
                Cell::Assign(AssignCell { value, .. }) => bitwise("assign", value.len()),
                Cell::Dff(FlipFlop { data, .. }) => bitwise("dff", data.len()),
                Cell::Memory(Memory { depth, width, .. }) => custom(format_args!("memory:{depth}:{width}")),
                Cell::IoBuf(IoBuffer { io, .. }) => bitwise("iobuf", io.len()),
                Cell::Target(TargetCell { kind, .. }) => custom(format_args!("{kind}")),
                Cell::Other(Instance { kind, .. }) => custom(format_args!("{kind}")),
                Cell::Input(_, width) => bitwise("input", *width),
                Cell::Output(_, value) => bitwise("output", value.len()),
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
        let comment = if !diff { "; " } else { " ; " };

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
                _ => (),
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

        for metadata_ref in self.metadata.borrow().iter_items(self) {
            if metadata_ref.index() == MetaItemIndex::NONE {
                continue;
            }
            write!(f, "{}", if !diff { "" } else { unchanged })?;
            write!(f, "{} = ", metadata_ref.index())?;
            let item = metadata_ref.get();
            match item {
                MetaItem::None => unreachable!(),
                MetaItem::Set(items) => {
                    write!(f, "{{")?;
                    for item in items {
                        write!(f, " {}", item.index())?;
                    }
                    write!(f, " }}")?;
                }
                MetaItem::Source { file, start, end } => {
                    write!(f, "source ")?;
                    self.write_string(f, &file.get())?;
                    write!(f, " (#{} #{}) (#{} #{})", start.line, start.column, end.line, end.column)?;
                }
                MetaItem::NamedScope { name: _, source, parent }
                | MetaItem::IndexedScope { index: _, source, parent } => {
                    write!(f, "scope ")?;
                    match item {
                        MetaItem::NamedScope { name, .. } => self.write_string(f, &name.get())?,
                        MetaItem::IndexedScope { index, .. } => write!(f, "#{index}")?,
                        _ => unreachable!(),
                    }
                    if !parent.is_none() {
                        write!(f, " in={}", parent.index())?;
                    }
                    if !source.is_none() {
                        write!(f, " src={}", source.index())?;
                    }
                }
                MetaItem::Ident { name, scope } => {
                    write!(f, "ident ")?;
                    self.write_string(f, &name.get())?;
                    write!(f, " in={}", scope.index())?;
                }
                MetaItem::Attr { name, value } => {
                    write!(f, "attr ")?;
                    self.write_string(f, &name.get())?;
                    write!(f, " ")?;
                    self.write_param_value(f, &value)?;
                }
            }
            writeln!(f)?;
        }

        for (name, io_value) in self.iter_ios() {
            write!(f, "{}&", if !diff { "" } else { unchanged })?;
            self.write_string(f, name)?;
            writeln!(f, ":{} = io", io_value.len())?;
        }
        for (name, io_value) in &changes.added_ios {
            write!(f, "{added}&")?;
            self.write_string(f, name)?;
            writeln!(f, ":{} = io", io_value.len())?;
        }

        let write_cell = |f: &mut std::fmt::Formatter, index: usize, cell: &Cell, metadata: MetaItemIndex| {
            for item in self.ref_metadata_item(metadata).iter() {
                match item.get() {
                    MetaItem::Source { file, start, end: _ } => {
                        writeln!(f, "{comment}source file://{}#{}", file.get(), start.line + 1)?;
                    }
                    MetaItem::NamedScope { .. } => {
                        let mut names = Vec::new();
                        let mut scope = item;
                        while !scope.is_none() {
                            let MetaItem::NamedScope { name, parent, .. } = scope.get() else { break };
                            names.push(name);
                            scope = parent;
                        }
                        if !names.is_empty() {
                            write!(f, "{comment}scope ")?;
                            for (index, name) in names.iter().rev().enumerate() {
                                if index > 0 {
                                    write!(f, ".")?;
                                }
                                write!(f, "{}", name.get())?;
                            }
                            writeln!(f)?;
                        }
                    }
                    _ => (),
                }
            }
            if matches!(cell, Cell::Target(..)) {
                for index in (index..index + cell.output_len()).rev() {
                    if let Some((name, offset)) = net_names.get(&Net::from_cell_index(index)) {
                        write!(f, "{comment}drives ")?;
                        self.write_string(f, &*name)?;
                        writeln!(f, "+{offset}")?;
                    }
                }
            }
            if !diff {
                self.write_cell(f, "", index, cell, metadata)?;
            } else if changes.unalived_cells.contains(&index) {
                self.write_cell(f, removed, index, cell, metadata)?;
            } else {
                let mut mapped_cell;
                if let Some(replaced_cell) = changes.replaced_cells.get(&index) {
                    mapped_cell = (*replaced_cell.get()).clone();
                } else {
                    mapped_cell = cell.clone();
                }
                mapped_cell.visit_mut(|net| {
                    while let Some(&new_net) = changes.replaced_nets.get(net) {
                        *net = new_net;
                    }
                });
                if index >= self.cells.len() {
                    self.write_cell(f, added, index, &mapped_cell, metadata)?;
                } else if mapped_cell != *cell {
                    self.write_cell(f, removed, index, cell, metadata)?;
                    writeln!(f)?;
                    self.write_cell(f, added, index, &mapped_cell, metadata)?;
                } else {
                    self.write_cell(f, unchanged, index, cell, metadata)?;
                }
            }
            writeln!(f)
        };

        if f.alternate() {
            for cell_ref in self.iter_cells() {
                write_cell(f, cell_ref.index, &*cell_ref.get(), cell_ref.metadata_index())?;
            }
        } else {
            for cell_ref in self.iter_cells_topo() {
                write_cell(f, cell_ref.index, &*cell_ref.get(), cell_ref.metadata_index())?;
            }
        }
        for (offset, cell) in changes.added_cells.iter().enumerate() {
            if !matches!(cell.repr, CellRepr::Skip(_) | CellRepr::Void) {
                write_cell(f, self.cells.len() + offset, &*cell.get(), cell.meta)?;
            }
        }

        Ok(())
    }
}
