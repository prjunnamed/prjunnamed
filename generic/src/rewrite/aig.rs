use prjunnamed_netlist::{
    Cell, ControlNet, MetaItemRef, Net, RewriteNetSource, RewriteResult, RewriteRuleset, Rewriter, Value,
};

/// Implements simple AIG optimizations.
///
/// The following transformations are done:
///
/// - multi-bit And, Or, Xor, Not are bitblasted
/// - single-bit And, Or are transformed into Aig
/// - Not cells on Aig inputs are merged into the Aig
/// - Not cells on Xor inputs are pushed onto the output
/// - const folding:
///   - ~0 -> 1
///   - ~1 -> 0
///   - ~X -> X
///   - a & 0 -> 0
///   - a & 1 -> a
///   - a ^ 0 -> a
///   - a ^ 1 -> ~a
///   - a ^ X -> X
/// - idempotence:
///   - a & a -> a
///   - (a & b) & a -> a
///   - (a & b) & (a & c) -> (a & b) & c
/// - contradiction:
///   - a & ~a -> 0
///   - (a & b) & ~a -> 0
///   - (a & b) & (~a & c) -> 0
/// - subsumption:
///   - (a | b) & a -> a
///   - (a | b) & (a & c) -> a & c
/// - resolution:
///   - (a | b) & (~a | b) -> b
/// - substitution:
///   - (a | b) & ~b -> a & ~b
///   - (a | b) & (~b & c) -> a & (~b & c)
///   - (a ^ b) & ~b -> a & ~b
///   - (a ^ b) & b -> ~a & b
///   - ~(a ^ b) & ~b -> ~a & ~b
///   - ~(a ^ b) & b -> a & b
///   - (a ^ b) & (~b & c) -> a & (~b & c)
///   - (a ^ b) & (b & c) -> ~a & (b & c)
///   - ~(a ^ b) & (~b & c) -> ~a & (~b & c)
///   - ~(a ^ b) & (b & c) -> a & (b & c)
/// - XOR folding:
///   - a ^ a -> 0
///   - (a ^ b) ^ a -> b
///   - (a ^ b) ^ (a ^ c) -> b ^ c
/// - AND-XOR optimization:
///   - (a & b) ^ a -> a & ~b
///   - (a & b) ^ ~a -> ~(a & ~b)
/// - XOR recognition:
///   - ~(a & b) & ~(~a & ~b) -> a ^ b
///
/// All rules above are replicated for any commutation of operands, and any negation of inputs.
///
/// This ruleset ensures that any fragment of the netlist including only Not, And, Or, Xor, Aig cells will
/// be transformed into a fragment containing only Aig, single-bit Xor, and single-bit Not cells, with the single-bit
/// Not cells only present when required by non-Aig non-Xor cell inputs.
///
/// The optimizations performed here are mostly borrowed from https://fmv.jku.at/papers/BrummayerBiere-MEMICS06.pdf
/// and have an important property described in the paper: they will never create more than one new cell (Not cells
/// don't count), and they will not increase the logic level. Thus, they can never make the netlist "worse".
pub struct SimpleAigOpt;

impl RewriteRuleset for SimpleAigOpt {
    fn rewrite<'a>(&self, cell: &Cell, meta: MetaItemRef<'a>, rewriter: &Rewriter<'a>) -> RewriteResult<'a> {
        match *cell {
            Cell::Not(ref val) if val.len() == 1 => {
                let net = val[0];
                if net == Net::ZERO {
                    return Net::ONE.into();
                }
                if net == Net::ONE {
                    return Net::ZERO.into();
                }
                if net == Net::UNDEF {
                    return Net::UNDEF.into();
                }
                let src = rewriter.find_cell(net);
                if let RewriteNetSource::Cell(cell, _, bit) = src
                    && let Cell::Not(ref val) = *cell
                {
                    return val[bit].into();
                }
                RewriteResult::None
            }

            Cell::Aig(net1, net2) => {
                for (net_a, net_b) in [(net1, net2), (net2, net1)] {
                    let src_a = rewriter.find_cell(net_a.net());
                    let src_b = rewriter.find_cell(net_b.net());

                    // idempotence: a & a -> a
                    if net_a == net_b {
                        return net_a.into();
                    }

                    // contradiction: a & ~a, a & 0 -> 0
                    if net_a == !net_b || net_b.is_always(false) {
                        return Net::ZERO.into();
                    }

                    // identity: a & 1 -> a
                    if net_b.is_always(true) {
                        return net_a.into();
                    }

                    // merge inverters into AIG cell
                    if let RewriteNetSource::Cell(ref cell_a, meta_a, bit) = src_a
                        && let Cell::Not(ref val) = **cell_a
                    {
                        let new_a = ControlNet::from_net_invert(val[bit], !net_a.is_negative());
                        return RewriteResult::CellMeta(Cell::Aig(new_a, net_b), meta.merge(meta_a));
                    }

                    if let RewriteNetSource::Cell(ref cell_a, _, _) = src_a
                        && let Cell::Aig(net_a1, net_a2) = **cell_a
                        && net_a.is_positive()
                    {
                        // (aa & ab) & b
                        for (net_aa, _net_ab) in [(net_a1, net_a2), (net_a2, net_a1)] {
                            // idempotence: (aa & ab) & aa -> aa & ab
                            if net_aa == net_b {
                                return net_a.into();
                            }
                            // contradiction: (aa & ab) & ~aa -> 0
                            if net_aa == !net_b {
                                return Net::ZERO.into();
                            }

                            if let RewriteNetSource::Cell(ref cell_b, _, _) = src_b
                                && let Cell::Aig(net_b1, net_b2) = **cell_b
                                && net_b.is_positive()
                            {
                                // (aa & ab) & (ba & bb)
                                for (net_ba, net_bb) in [(net_b1, net_b2), (net_b2, net_b1)] {
                                    // idempotence: (aa & ab) & (aa & bb) -> (aa & ab) & bb
                                    if net_aa == net_ba {
                                        return Cell::Aig(net_a, net_bb).into();
                                    }
                                    // contradiction: (aa & ab) & (~aa & bb) -> 0
                                    if net_aa == !net_ba {
                                        return Net::ZERO.into();
                                    }
                                }
                            }
                        }
                    }

                    if let RewriteNetSource::Cell(ref cell_a, meta_a, _) = src_a
                        && let Cell::Aig(net_a1, net_a2) = **cell_a
                        && net_a.is_negative()
                    {
                        // ~(aa & ab) & b
                        for (net_aa, net_ab) in [(net_a1, net_a2), (net_a2, net_a1)] {
                            // substitution: ~(aa & ab) & aa -> ~ab & aa
                            if net_aa == net_b {
                                return Cell::Aig(!net_ab, net_b).into();
                            }
                            // subsumption: ~(aa & ab) & ~aa -> ~aa
                            if net_aa == !net_b {
                                return net_b.into();
                            }

                            if let RewriteNetSource::Cell(ref cell_b, _, _) = src_b
                                && let Cell::Aig(net_b1, net_b2) = **cell_b
                                && net_b.is_positive()
                            {
                                // ~(aa & ab) & (ba & bb)
                                for (net_ba, _net_bb) in [(net_b1, net_b2), (net_b2, net_b1)] {
                                    // substitution: ~(aa & ab) & (aa & bb) -> ~ab & (aa & bb)
                                    if net_aa == net_ba {
                                        return Cell::Aig(!net_ab, net_b).into();
                                    }
                                    // subsumption: ~(aa & ab) & (~aa & bb) -> (~aa & bb)
                                    if net_aa == !net_ba {
                                        return net_b.into();
                                    }
                                }
                            }

                            if let RewriteNetSource::Cell(ref cell_b, meta_b, _) = src_b
                                && let Cell::Aig(net_b1, net_b2) = **cell_b
                                && net_b.is_negative()
                            {
                                // ~(aa & ab) & ~(ba & bb)
                                for (net_ba, net_bb) in [(net_b1, net_b2), (net_b2, net_b1)] {
                                    // resolution: ~(aa & ab) & ~(~aa & ab) -> ~ab
                                    if net_aa == !net_ba && net_ab == net_bb {
                                        return (!net_ab).into();
                                    }
                                    // XOR recognition: ~(aa & ab) & ~(~aa & ~ab) -> aa ^ ab
                                    if net_aa == !net_ba && net_ab == !net_bb {
                                        let xor_meta = meta.merge(meta_a).merge(meta_b);
                                        let xor = Cell::Xor(net_aa.net().into(), net_ab.net().into());
                                        if net_aa.is_negative() ^ net_ab.is_negative() {
                                            let xor = rewriter.add_cell_meta(xor, xor_meta);
                                            return Cell::Not(xor).into();
                                        } else {
                                            return RewriteResult::CellMeta(xor.into(), xor_meta);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if let RewriteNetSource::Cell(ref cell_a, _, bit) = src_a
                        && let Cell::Xor(ref val_a1, ref val_a2) = **cell_a
                    {
                        let inv_a = net_a.is_negative();
                        let net_a1 = val_a1[bit];
                        let net_a2 = val_a2[bit];
                        // (aa ^ ab) & b
                        for (net_aa, net_ab) in [(net_a1, net_a2), (net_a2, net_a1)] {
                            // substitution: (aa ^ ab) & aa -> ~ab & aa
                            // substitution: ~(aa ^ ab) & aa -> ab & aa
                            // substitution: (aa ^ ab) & ~aa -> ab & ~aa
                            // substitution: ~(aa ^ ab) & ~aa -> ~ab & ~aa
                            if net_aa == net_b.net() {
                                let inv = !(inv_a ^ net_b.is_negative());
                                return Cell::Aig(ControlNet::from_net_invert(net_ab, inv), net_b).into();
                            }

                            if let RewriteNetSource::Cell(ref cell_b, _, _) = src_b
                                && let Cell::Aig(net_b1, net_b2) = **cell_b
                                && net_b.is_positive()
                            {
                                // (aa ^ ab) & (ba & bb)
                                for (net_ba, _net_bb) in [(net_b1, net_b2), (net_b2, net_b1)] {
                                    // substitution: (aa ^ ab) & (aa & bb) -> ~ab & aa
                                    // substitution: ~(aa ^ ab) & (aa & bb) -> ab & aa
                                    // substitution: (aa ^ ab) & (~aa & bb) -> ab & ~aa
                                    // substitution: ~(aa ^ ab) & (~aa & bb) -> ~ab & ~aa
                                    if net_aa == net_ba.net() {
                                        let inv = !(inv_a ^ net_ba.is_negative());
                                        return Cell::Aig(ControlNet::from_net_invert(net_ab, inv), net_b).into();
                                    }
                                }
                            }
                        }
                    }
                }

                RewriteResult::None
            }

            Cell::Xor(ref val1, ref val2) if val1.len() == 1 => {
                let net1 = val1[0];
                let net2 = val2[0];
                for (net_a, net_b) in [(net1, net2), (net2, net1)] {
                    let src_a = rewriter.find_cell(net_a);
                    let src_b = rewriter.find_cell(net_b);

                    // a ^ a -> 0
                    if net_a == net_b {
                        return Net::ZERO.into();
                    }

                    // a ^ X -> X
                    if net_b == Net::UNDEF {
                        return Net::UNDEF.into();
                    }

                    // a ^ 0 -> a
                    if net_b == Net::ZERO {
                        return net_a.into();
                    }

                    // a ^ 1 -> ~a
                    if net_b == Net::ONE {
                        return Cell::Not(net_a.into()).into();
                    }

                    // !a ^ b -> !(a ^ b)
                    if let RewriteNetSource::Cell(ref cell_a, meta_a, bit) = src_a
                        && let Cell::Not(ref val) = **cell_a
                    {
                        let meta = meta.merge(meta_a);
                        let xor = rewriter.add_cell_meta(Cell::Xor(val[bit].into(), net_b.into()), meta);
                        return RewriteResult::CellMeta(Cell::Not(xor), meta);
                    }

                    if let RewriteNetSource::Cell(ref cell_a, meta_a, _) = src_a
                        && let Cell::Aig(net_a1, net_a2) = **cell_a
                    {
                        // (aa & ab) ^ b
                        for (net_aa, net_ab) in [(net_a1, net_a2), (net_a2, net_a1)] {
                            // (aa & ab) ^ aa -> aa & ~ab
                            let meta = meta.merge(meta_a);
                            if net_aa == ControlNet::Pos(net_b) {
                                return RewriteResult::CellMeta(Cell::Aig(net_aa, !net_ab), meta);
                            }
                            // (aa & ab) ^ ~aa -> ~(aa & ~ab)
                            if net_aa == ControlNet::Neg(net_b) {
                                let aig = rewriter.add_cell(Cell::Aig(net_aa, !net_ab));
                                return RewriteResult::CellMeta(Cell::Not(aig), meta);
                            }
                        }
                    }

                    if let RewriteNetSource::Cell(ref cell_a, meta_a, bit) = src_a
                        && let Cell::Xor(ref val_a1, ref val_a2) = **cell_a
                    {
                        let net_a1 = val_a1[bit];
                        let net_a2 = val_a2[bit];
                        // (aa ^ ab) ^ b
                        for (net_aa, net_ab) in [(net_a1, net_a2), (net_a2, net_a1)] {
                            // (aa ^ ab) ^ aa -> ab
                            if net_aa == net_b {
                                return net_ab.into();
                            }

                            if let RewriteNetSource::Cell(ref cell_b, meta_b, bit) = src_b
                                && let Cell::Xor(ref val_b1, ref val_b2) = **cell_b
                            {
                                let net_b1 = val_b1[bit];
                                let net_b2 = val_b2[bit];
                                // (aa ^ ab) ^ (ba ^ bb)
                                for (net_ba, net_bb) in [(net_b1, net_b2), (net_b2, net_b1)] {
                                    // (aa ^ ab) ^ (aa ^ bb) -> ab ^ bb
                                    if net_aa == net_ba {
                                        let meta = meta.merge(meta_a).merge(meta_b);
                                        return RewriteResult::CellMeta(
                                            Cell::Xor(net_ab.into(), net_bb.into()).into(),
                                            meta,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                RewriteResult::None
            }

            // Split bitwise Not and Xor into single-bit cells.
            Cell::Not(ref val) => {
                let mut result = Value::new();
                for net in val {
                    result.extend(rewriter.add_cell(Cell::Not(net.into())));
                }
                result.into()
            }
            Cell::Xor(ref val_a, ref val_b) => {
                let mut result = Value::new();
                for (net_a, net_b) in val_a.iter().zip(val_b.iter()) {
                    result.extend(rewriter.add_cell(Cell::Xor(net_a.into(), net_b.into())));
                }
                result.into()
            }

            // Convert And and Or into AIG.
            Cell::And(ref val_a, ref val_b) => {
                let mut result = Value::new();
                for (net_a, net_b) in val_a.iter().zip(val_b.iter()) {
                    result.extend(rewriter.add_cell(Cell::Aig(net_a.into(), net_b.into())));
                }
                result.into()
            }
            Cell::Or(ref val_a, ref val_b) => {
                let mut result = Value::new();
                for (net_a, net_b) in val_a.iter().zip(val_b.iter()) {
                    let aig = rewriter.add_cell(Cell::Aig(ControlNet::Neg(net_a), ControlNet::Neg(net_b)));
                    result.extend(rewriter.add_cell(Cell::Not(aig)));
                }
                result.into()
            }

            _ => RewriteResult::None,
        }
    }
}
