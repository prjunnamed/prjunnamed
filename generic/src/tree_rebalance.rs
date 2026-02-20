use std::{
    cell::RefCell,
    collections::{BTreeSet, BinaryHeap, HashMap, HashSet},
};

use prjunnamed_netlist::{Cell, ControlNet, Design, Net, RewriteResult, RewriteRuleset};

use crate::{LevelAnalysis, Normalize, SimpleAigOpt};

struct TreeRebalance<'a> {
    levels: &'a LevelAnalysis,
    inner_aig: HashSet<Net>,
    inner_xor: HashSet<Net>,
    aig_trees: RefCell<HashMap<Net, BTreeSet<ControlNet>>>,
    xor_trees: RefCell<HashMap<Net, BTreeSet<Net>>>,
}

impl<'a> TreeRebalance<'a> {
    fn new(design: &Design, levels: &'a LevelAnalysis) -> Self {
        let mut inner_aig = HashSet::new();
        let mut inner_xor = HashSet::new();
        let mut use_count = HashMap::<Net, u32>::new();
        for cell in design.iter_cells() {
            cell.visit(|net| {
                *use_count.entry(net).or_default() += 1;
            });
        }
        for cell in design.iter_cells() {
            if let Cell::Aig(net1, net2) = *cell.get() {
                for net in [net1, net2] {
                    if let ControlNet::Pos(net) = net
                        && use_count[&net] == 1
                    {
                        inner_aig.insert(net);
                    }
                }
            }
            if let Cell::Xor(ref val1, ref val2) = *cell.get() {
                for val in [val1, val2] {
                    for net in val {
                        if use_count[&net] == 1 {
                            inner_xor.insert(net);
                        }
                    }
                }
            }
        }
        Self { levels, inner_aig, inner_xor, aig_trees: Default::default(), xor_trees: Default::default() }
    }
}

impl RewriteRuleset for TreeRebalance<'_> {
    fn rewrite<'a>(
        &self,
        cell: &Cell,
        _meta: prjunnamed_netlist::MetaItemRef<'a>,
        output: Option<&prjunnamed_netlist::Value>,
        rewriter: &prjunnamed_netlist::Rewriter<'a>,
    ) -> RewriteResult<'a> {
        let Some(output) = output else {
            return RewriteResult::None;
        };
        if output.len() != 1 {
            return RewriteResult::None;
        }
        let output = output[0];
        match *cell {
            Cell::Aig(net1, net2) => {
                let mut aig_trees = self.aig_trees.borrow_mut();
                let mut inputs1 = if let ControlNet::Pos(net) = net1
                    && let Some(inputs) = aig_trees.remove(&net)
                {
                    inputs
                } else {
                    BTreeSet::from_iter([net1])
                };
                let mut inputs2 = if let ControlNet::Pos(net) = net2
                    && let Some(inputs) = aig_trees.remove(&net)
                {
                    inputs
                } else {
                    BTreeSet::from_iter([net2])
                };
                if inputs1.len() < inputs2.len() {
                    std::mem::swap(&mut inputs1, &mut inputs2);
                }
                inputs1.extend(inputs2);
                let inputs = inputs1;
                if self.inner_aig.contains(&output) {
                    aig_trees.insert(output, inputs);
                    RewriteResult::None
                } else {
                    if inputs.len() == 2 {
                        return RewriteResult::None;
                    }
                    let mut inputs = BinaryHeap::from_iter(
                        inputs.into_iter().map(|net| std::cmp::Reverse((self.levels.get(net.net()), net))),
                    );
                    while inputs.len() > 1 {
                        let (lvl1, net1) = inputs.pop().unwrap().0;
                        let (lvl2, net2) = inputs.pop().unwrap().0;
                        let lvl = lvl1.max(lvl2) + 1;
                        let val = rewriter.add_cell(Cell::Aig(net1, net2));
                        let net = ControlNet::Pos(val[0]);
                        inputs.push(std::cmp::Reverse((lvl, net)));
                    }
                    let net = inputs.pop().unwrap().0.1;
                    net.into()
                }
            }
            Cell::Xor(ref val1, ref val2) => {
                let net1 = val1[0];
                let net2 = val2[0];
                let mut xor_trees = self.xor_trees.borrow_mut();
                let mut inputs1 =
                    if let Some(inputs) = xor_trees.remove(&net1) { inputs } else { BTreeSet::from_iter([net1]) };
                let mut inputs2 =
                    if let Some(inputs) = xor_trees.remove(&net2) { inputs } else { BTreeSet::from_iter([net2]) };
                if inputs1.len() < inputs2.len() {
                    std::mem::swap(&mut inputs1, &mut inputs2);
                }
                for net in inputs2 {
                    if !inputs1.remove(&net) {
                        inputs1.insert(net);
                    }
                }
                let inputs = inputs1;
                if self.inner_xor.contains(&output) {
                    xor_trees.insert(output, inputs);
                    RewriteResult::None
                } else {
                    if inputs.len() == 2 {
                        return RewriteResult::None;
                    }
                    let mut inputs = BinaryHeap::from_iter(
                        inputs.into_iter().map(|net| std::cmp::Reverse((self.levels.get(net), net))),
                    );
                    while inputs.len() > 1 {
                        let (lvl1, net1) = inputs.pop().unwrap().0;
                        let (lvl2, net2) = inputs.pop().unwrap().0;
                        let lvl = lvl1.max(lvl2) + 1;
                        let val = rewriter.add_cell(Cell::Xor(net1.into(), net2.into()));
                        inputs.push(std::cmp::Reverse((lvl, val[0])));
                    }
                    let net = inputs.pop().unwrap().0.1;
                    net.into()
                }
            }
            _ => RewriteResult::None,
        }
    }

    fn net_replaced(&self, _design: &Design, from: Net, to: Net) {
        let mut aig_trees = self.aig_trees.borrow_mut();
        if let Some(tree) = aig_trees.remove(&from) {
            aig_trees.insert(to, tree);
        }
        let mut xor_trees = self.xor_trees.borrow_mut();
        if let Some(tree) = xor_trees.remove(&from) {
            xor_trees.insert(to, tree);
        }
    }
}

pub fn tree_rebalance(design: &mut Design) {
    let levels = LevelAnalysis::new();
    let rebalance = TreeRebalance::new(design, &levels);
    design.rewrite(&[&Normalize, &SimpleAigOpt, &levels, &rebalance]);
}
