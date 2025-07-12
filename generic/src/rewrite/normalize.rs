use prjunnamed_netlist::{Cell, MetaItemRef, RewriteResult, RewriteRuleset, Rewriter, Value};

pub struct Normalize;

impl RewriteRuleset for Normalize {
    fn rewrite<'a>(
        &self,
        cell: &Cell,
        _meta: MetaItemRef<'a>,
        _output: Option<&Value>,
        _rewriter: &Rewriter<'a>,
    ) -> RewriteResult<'a> {
        let _ = _output;
        match cell {
            Cell::Buf(val) => val.clone().into(),
            Cell::And(val1, val2) if val1 > val2 => RewriteResult::Cell(Cell::And(val2.clone(), val1.clone())),
            Cell::Or(val1, val2) if val1 > val2 => RewriteResult::Cell(Cell::Or(val2.clone(), val1.clone())),
            Cell::Xor(val1, val2) if val1 > val2 => RewriteResult::Cell(Cell::Xor(val2.clone(), val1.clone())),
            Cell::Adc(val1, val2, net) if val1 > val2 => {
                RewriteResult::Cell(Cell::Adc(val2.clone(), val1.clone(), *net))
            }
            Cell::Aig(net1, net2) if net1 > net2 => RewriteResult::Cell(Cell::Aig(*net2, *net1)),
            Cell::Eq(val1, val2) if val1 > val2 => RewriteResult::Cell(Cell::Eq(val2.clone(), val1.clone())),
            Cell::Mul(val1, val2) if val1 > val2 => RewriteResult::Cell(Cell::Mul(val2.clone(), val1.clone())),
            _ => RewriteResult::None,
        }
    }
}
