use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, HashSet},
};

use crate::{design::TopoSortItem, Cell, ControlNet, Design, MetaItemRef, Net, Trit, Value};

pub enum RewriteResult<'a> {
    None,
    Cell(Cell),
    CellMeta(Cell, MetaItemRef<'a>),
    Value(Value),
}

impl From<Value> for RewriteResult<'_> {
    fn from(value: Value) -> Self {
        RewriteResult::Value(value)
    }
}

impl From<Net> for RewriteResult<'_> {
    fn from(net: Net) -> Self {
        RewriteResult::Value(net.into())
    }
}

impl From<ControlNet> for RewriteResult<'_> {
    fn from(cnet: ControlNet) -> Self {
        match cnet {
            ControlNet::Pos(net) => RewriteResult::Value(net.into()),
            ControlNet::Neg(net) => RewriteResult::Cell(Cell::Not(net.into())),
        }
    }
}

impl From<Cell> for RewriteResult<'_> {
    fn from(cell: Cell) -> Self {
        RewriteResult::Cell(cell)
    }
}

pub enum RewriteNetSource<'a> {
    Const(Trit),
    Opaque,
    Cell(Cow<'a, Cell>, MetaItemRef<'a>, usize),
}

pub trait RewriteRuleset {
    fn rewrite<'a>(
        &self,
        cell: &Cell,
        meta: MetaItemRef<'a>,
        output: Option<&Value>,
        rewriter: &Rewriter<'a>,
    ) -> RewriteResult<'a> {
        let _ = (cell, meta, output, rewriter);
        RewriteResult::None
    }
    fn cell_added(&self, design: &Design, cell: &Cell, output: &Value) {
        let _ = (design, cell, output);
    }
    fn net_replaced(&self, design: &Design, from: Net, to: Net) {
        let _ = (design, from, to);
    }
}

pub struct Rewriter<'a> {
    design: &'a Design,
    rules: &'a [&'a dyn RewriteRuleset],
    processed: RefCell<HashSet<Net>>,
    cache: RefCell<HashMap<Cell, Value>>,
}

impl<'a> Rewriter<'a> {
    pub fn find_cell(&self, net: Net) -> RewriteNetSource<'a> {
        if !self.processed.borrow().contains(&net) && !net.is_const() {
            return RewriteNetSource::Opaque;
        }
        match self.design.find_new_cell(net) {
            Ok((cell, meta, bit)) => RewriteNetSource::Cell(cell, meta, bit),
            Err(trit) => RewriteNetSource::Const(trit),
        }
    }

    fn process_cell(&self, cell: &Cell, meta: MetaItemRef<'a>, output: Option<&Value>) -> RewriteResult<'a> {
        let mut replacement_cell = None;
        let mut replacement_meta = None;
        'outer: loop {
            for &rule in self.rules {
                let cur_cell = replacement_cell.as_ref().unwrap_or(cell);
                let cur_meta = replacement_meta.unwrap_or(meta);
                let _guard = self.design.use_metadata(cur_meta);
                match rule.rewrite(cur_cell, cur_meta, output, self) {
                    RewriteResult::None => (),
                    RewriteResult::Cell(new_cell) => {
                        replacement_cell = Some(new_cell);
                        continue 'outer;
                    }
                    RewriteResult::CellMeta(new_cell, new_meta) => {
                        replacement_cell = Some(new_cell);
                        replacement_meta = Some(new_meta);
                        continue 'outer;
                    }
                    RewriteResult::Value(value) => {
                        return RewriteResult::Value(value);
                    }
                }
            }
            break;
        }
        let cell = replacement_cell.as_ref().unwrap_or(cell);
        if let Some(value) = self.cache.borrow().get(&cell) {
            let meta = replacement_meta.unwrap_or(meta);
            self.design.append_metadata_by_net(value[0], meta);
            RewriteResult::Value(value.clone())
        } else {
            match (replacement_cell, replacement_meta) {
                (None, _) => RewriteResult::None,
                (Some(cell), None) => RewriteResult::Cell(cell),
                (Some(cell), Some(meta)) => RewriteResult::CellMeta(cell, meta),
            }
        }
    }

    pub fn add_cell(&self, cell: Cell) -> Value {
        self.add_cell_meta(cell, self.design.get_use_metadata())
    }

    pub fn add_cell_meta(&self, cell: Cell, meta: MetaItemRef<'_>) -> Value {
        self.add_cell_meta_output(cell, meta, None)
    }

    fn add_cell_meta_output(&self, cell: Cell, meta: MetaItemRef<'_>, output: Option<&Value>) -> Value {
        let (cell, meta) = match self.process_cell(&cell, meta, output) {
            RewriteResult::None => (cell, meta),
            RewriteResult::Cell(new_cell) => (new_cell, meta),
            RewriteResult::CellMeta(new_cell, new_meta) => (new_cell, new_meta),
            RewriteResult::Value(value) => return value,
        };
        let value = self.design.add_cell_with_metadata_ref(cell.clone(), meta);
        for &rule in self.rules {
            rule.cell_added(self.design, &cell, &value);
        }
        for net in &value {
            self.processed.borrow_mut().insert(net);
        }
        if !cell.has_effects(self.design) {
            self.cache.borrow_mut().insert(cell, value.clone());
        }
        value
    }

    fn run(&mut self) {
        let worklist = self.design.topo_sort();
        for item in worklist {
            match item {
                TopoSortItem::Cell(cell_ref) => {
                    let output = cell_ref.output();
                    let mut cell = cell_ref.get().into_owned();
                    cell.visit_mut(|net| *net = self.design.map_net_new(*net));
                    match self.process_cell(&cell, cell_ref.metadata(), Some(&output)) {
                        RewriteResult::None => {
                            for &rule in self.rules {
                                rule.cell_added(self.design, &cell, &output);
                            }
                            if !cell.has_effects(self.design) {
                                self.cache.borrow_mut().insert(cell, output.clone());
                            }
                            for net in output {
                                self.processed.borrow_mut().insert(net);
                            }
                        }
                        RewriteResult::Cell(new_cell) => {
                            cell_ref.replace(new_cell.clone());
                            for &rule in self.rules {
                                rule.cell_added(self.design, &new_cell, &output);
                            }
                            if !new_cell.has_effects(self.design) {
                                self.cache.borrow_mut().insert(new_cell, output.clone());
                            }
                            for net in output {
                                self.processed.borrow_mut().insert(net);
                            }
                        }
                        RewriteResult::CellMeta(new_cell, new_meta) => {
                            cell_ref.replace(new_cell.clone());
                            for &rule in self.rules {
                                rule.cell_added(self.design, &new_cell, &output);
                            }
                            cell_ref.append_metadata(new_meta);
                            if !new_cell.has_effects(self.design) {
                                self.cache.borrow_mut().insert(new_cell, output.clone());
                            }
                            for net in output {
                                self.processed.borrow_mut().insert(net);
                            }
                        }
                        RewriteResult::Value(value) => {
                            assert_eq!(value.len(), output.len());
                            for (net, new_net) in output.iter().zip(value) {
                                self.design.replace_net(net, new_net);
                                for &rule in self.rules {
                                    rule.net_replaced(self.design, net, new_net);
                                }
                                self.processed.borrow_mut().insert(net);
                            }
                            cell_ref.unalive();
                        }
                    }
                }
                TopoSortItem::CellBit(cell, bit) => {
                    let mut slice = cell.get().slice(bit..bit + 1).unwrap();
                    slice.visit_mut(|net| *net = self.design.map_net_new(*net));
                    let net = cell.output()[bit];
                    let new_value = self.add_cell_meta_output(slice, cell.metadata(), Some(&net.into()));
                    let new_net = new_value[0];
                    self.design.replace_net(net, new_net);
                    for &rule in self.rules {
                        rule.net_replaced(self.design, net, new_net);
                    }
                    self.processed.borrow_mut().insert(net);
                }
            }
        }
    }
}

impl Design {
    pub fn rewrite(&mut self, rules: &[&dyn RewriteRuleset]) {
        assert!(!self.is_changed());
        let mut rewriter = Rewriter {
            design: self,
            rules,
            processed: RefCell::new(HashSet::new()),
            cache: RefCell::new(HashMap::new()),
        };
        rewriter.run();
        self.compact();
    }
}
