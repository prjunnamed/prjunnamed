use std::{cell::RefCell, cmp::Ordering, collections::HashMap};

use prjunnamed_netlist::{
    Cell, ControlNet, Design, MetaItemRef, Net, RewriteNetSource, RewriteResult, RewriteRuleset, Rewriter, Value,
};

use crate::{LevelAnalysis, Normalize, SimpleAigOpt};

#[derive(Clone, Debug)]
struct AigChain {
    invert: bool,
    min_level: u32,
    /// List of (level, propagate, generate) pairs to be used for further rebalancing.
    ///
    /// This list satisfies the following conditions:
    /// 1. The node is equivalent to an AND-OR of all inputs on this list (in order, starting from const-1).
    /// 2. The list is sorted strictly descending by level (no two nodes are the same level).
    /// 3. All prop/genr levels are no smaller than `min_level`.
    full_trees: Vec<AigFullTree>,
}

#[derive(Copy, Clone, Debug)]
struct PropGen {
    p: ControlNet,
    g: ControlNet,
}

impl PropGen {
    fn or(net: ControlNet) -> Self {
        PropGen { p: ControlNet::Pos(Net::ONE), g: net }
    }

    fn and(net: ControlNet) -> Self {
        PropGen { p: net, g: ControlNet::Pos(Net::ZERO) }
    }

    fn combine(rewriter: &Rewriter, a: PropGen, b: PropGen) -> PropGen {
        let prop_val = rewriter.add_cell(Cell::Aig(a.p, b.p));
        let tmp = rewriter.add_cell(Cell::Aig(a.g, b.p));
        let genr_val_b = rewriter.add_cell(Cell::Aig(ControlNet::Neg(tmp[0]), !b.g));
        PropGen { p: ControlNet::Pos(prop_val[0]), g: ControlNet::Neg(genr_val_b[0]) }
    }
}

#[derive(Copy, Clone, Debug)]
struct AigFullTree {
    level: u32,
    pg: PropGen,
    cumulative: PropGen,
}

#[derive(Clone, Debug)]
struct XorChain {
    min_level: u32,
    full_trees: Vec<XorFullTree>,
}

#[derive(Copy, Clone, Debug)]
struct XorFullTree {
    level: u32,
    net: Net,
    cumulative_net: Net,
}

pub struct ChainRebalance<'a> {
    levels: &'a LevelAnalysis,
    aig_chains: RefCell<HashMap<Net, AigChain>>,
    xor_chains: RefCell<HashMap<Net, XorChain>>,
}

impl<'a> ChainRebalance<'a> {
    pub fn new(levels: &'a LevelAnalysis) -> Self {
        Self { levels, aig_chains: Default::default(), xor_chains: Default::default() }
    }
}

impl RewriteRuleset for ChainRebalance<'_> {
    fn rewrite<'a>(
        &self,
        cell: &Cell,
        _meta: MetaItemRef<'a>,
        output: Option<&Value>,
        rewriter: &Rewriter<'a>,
    ) -> RewriteResult<'a> {
        let Some(output) = output else { return RewriteResult::None };
        if output.len() != 1 {
            return RewriteResult::None;
        }
        let output = output[0];
        match cell {
            &Cell::Aig(net1, net2) => {
                let level1 = self.levels.get(net1.net());
                let level2 = self.levels.get(net2.net());
                let (net_a, net_b, level_a, level_b) = match level1.cmp(&level2) {
                    Ordering::Less => (net2, net1, level2, level1),
                    Ordering::Equal => return RewriteResult::None,
                    Ordering::Greater => (net1, net2, level1, level2),
                };
                let mut aig_chains = self.aig_chains.borrow_mut();
                if let Some(chain) = aig_chains.get(&net_a.net()) {
                    let mut chain = chain.clone();
                    if net_a.is_negative() {
                        chain.invert = !chain.invert;
                    }
                    chain.min_level = chain.min_level.max(level_b);
                    // adjust levels of everything to at least the new min_level
                    let mut top = chain.full_trees.pop().unwrap();
                    while top.level < chain.min_level {
                        if let Some(&next_top) = chain.full_trees.last()
                            && next_top.level <= chain.min_level
                        {
                            top.level = next_top.level + 1;
                            top.pg = next_top.cumulative;
                            top.cumulative = next_top.cumulative;
                            chain.full_trees.pop();
                        } else {
                            top.level = chain.min_level;
                            break;
                        }
                    }
                    chain.full_trees.push(top);
                    // add the new input; merge last two entries until invariant holds
                    let pg = if chain.invert { PropGen::or(!net_b) } else { PropGen::and(net_b) };
                    let mut new_top = AigFullTree { level: chain.min_level, pg, cumulative: pg };
                    while let Some(&cur_top) = chain.full_trees.last()
                        && cur_top.level == new_top.level
                    {
                        chain.full_trees.pop();
                        new_top.pg = PropGen::combine(rewriter, cur_top.pg, new_top.pg);
                        new_top.cumulative = new_top.pg;
                        new_top.level += 1;
                    }
                    // don't push the new last entry just yet; compute cumulative first
                    let mut cumulative = new_top.pg;
                    for subtree in chain.full_trees.iter_mut().rev() {
                        cumulative = PropGen::combine(rewriter, subtree.pg, cumulative);
                        subtree.cumulative = cumulative;
                    }
                    // now push the new last entry
                    chain.full_trees.push(new_top);
                    let mut result = rewriter.add_cell(Cell::Aig(!cumulative.p, !cumulative.g))[0];
                    if !chain.invert {
                        result = rewriter.add_cell(Cell::Not(result.into()))[0];
                    }
                    if let RewriteNetSource::Cell(cell, _, _) = rewriter.find_cell(result)
                        && let Cell::Not(ref inv_result) = *cell
                    {
                        let inv_result = inv_result[0];
                        chain.invert = !chain.invert;
                        aig_chains.insert(inv_result, chain);
                    } else {
                        aig_chains.insert(result, chain);
                    }
                    result.into()
                } else {
                    if net_a.is_negative() {
                        let chain = AigChain {
                            invert: true,
                            min_level: level_a - 1,
                            full_trees: vec![
                                AigFullTree {
                                    level: level_a,
                                    pg: PropGen::and(!net_a),
                                    cumulative: PropGen { p: !net_a, g: !net_b },
                                },
                                AigFullTree {
                                    level: level_a - 1,
                                    pg: PropGen::or(!net_b),
                                    cumulative: PropGen::or(!net_b),
                                },
                            ],
                        };
                        aig_chains.insert(output, chain);
                    } else {
                        let chain = AigChain {
                            invert: false,
                            min_level: level_a - 1,
                            full_trees: vec![
                                AigFullTree {
                                    level: level_a,
                                    pg: PropGen::and(net_a),
                                    cumulative: PropGen::and(output.into()),
                                },
                                AigFullTree {
                                    level: level_a - 1,
                                    pg: PropGen::and(net_b),
                                    cumulative: PropGen::and(net_b),
                                },
                            ],
                        };
                        aig_chains.insert(output, chain);
                    }
                    RewriteResult::None
                }
            }
            Cell::Xor(val1, val2) if val1.len() == 1 => {
                let net1 = val1[0];
                let net2 = val2[0];
                let level1 = self.levels.get(net1);
                let level2 = self.levels.get(net2);
                let (net_a, net_b, level_a, level_b) = match level1.cmp(&level2) {
                    Ordering::Less => (net2, net1, level2, level1),
                    Ordering::Equal => return RewriteResult::None,
                    Ordering::Greater => (net1, net2, level1, level2),
                };
                let mut xor_chains = self.xor_chains.borrow_mut();
                if let Some(chain) = xor_chains.get(&net_a) {
                    let mut chain = chain.clone();
                    chain.min_level = chain.min_level.max(level_b);
                    if chain.full_trees.len() == 1 {
                        if chain.full_trees[0].level > level_b {
                            chain.full_trees[0].cumulative_net = output;
                            chain.full_trees.push(XorFullTree { level: level_b, net: net_b, cumulative_net: net_b });
                            xor_chains.insert(output, chain);
                        }
                        return RewriteResult::None;
                    }
                    // adjust levels of everything to at least the new min_level
                    let mut top = chain.full_trees.pop().unwrap();
                    while top.level < chain.min_level {
                        if let Some(&next_top) = chain.full_trees.last()
                            && next_top.level <= chain.min_level
                        {
                            top.level = next_top.level + 1;
                            top.net = next_top.cumulative_net;
                            top.cumulative_net = next_top.cumulative_net;
                            chain.full_trees.pop();
                        } else {
                            top.level = chain.min_level;
                            break;
                        }
                    }
                    chain.full_trees.push(top);
                    // add the new input; merge last two entries until invariant holds
                    let mut level_top = chain.min_level;
                    let mut net_top = net_b;
                    while let Some(&next_top) = chain.full_trees.last()
                        && next_top.level == level_top
                    {
                        chain.full_trees.pop();
                        let val = rewriter.add_cell(Cell::Xor(net_top.into(), next_top.net.into()));
                        net_top = val[0];
                        level_top += 1;
                    }
                    // don't push the new last entry just yet; compute cumulative_net first
                    let mut cumulative_net = net_top;
                    for subtree in chain.full_trees.iter_mut().rev() {
                        let val = rewriter.add_cell(Cell::Xor(cumulative_net.into(), subtree.net.into()));
                        cumulative_net = val[0];
                        subtree.cumulative_net = cumulative_net;
                    }
                    // now push the new last entry
                    chain.full_trees.push(XorFullTree { level: level_top, net: net_top, cumulative_net: net_top });
                    xor_chains.insert(cumulative_net, chain);
                    cumulative_net.into()
                } else {
                    let chain = XorChain {
                        min_level: level_a - 1,
                        full_trees: vec![
                            XorFullTree { level: level_a, net: net_a, cumulative_net: output },
                            XorFullTree { level: level_a - 1, net: net_b, cumulative_net: net_b },
                        ],
                    };
                    xor_chains.insert(output, chain);
                    RewriteResult::None
                }
            }
            _ => RewriteResult::None,
        }
    }

    fn net_replaced(&self, _design: &Design, from: Net, to: Net) {
        let mut aig_chains = self.aig_chains.borrow_mut();
        if let Some(chain) = aig_chains.get(&from)
            && !aig_chains.contains_key(&to)
        {
            let chain = chain.clone();
            aig_chains.insert(to, chain);
        }
        let mut xor_chains = self.xor_chains.borrow_mut();
        if let Some(chain) = xor_chains.get(&from)
            && !xor_chains.contains_key(&to)
        {
            let chain = chain.clone();
            xor_chains.insert(to, chain);
        }
    }
}

pub fn chain_rebalance(design: &mut Design) {
    let levels = LevelAnalysis::new();
    let rebalance = ChainRebalance::new(&levels);
    design.rewrite(&[&Normalize, &SimpleAigOpt, &levels, &rebalance]);
}
