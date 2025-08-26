use prjunnamed_netlist::{Cell, MetaItemRef, Net, RewriteResult, RewriteRuleset, Rewriter, Value};

pub struct LowerMux;

impl RewriteRuleset for LowerMux {
    fn rewrite<'a>(
        &self,
        cell: &Cell,
        _meta: MetaItemRef<'a>,
        _output: Option<&Value>,
        rewriter: &Rewriter<'a>,
    ) -> RewriteResult<'a> {
        let &Cell::Mux(sel, ref val1, ref val2) = cell else {
            return RewriteResult::None;
        };
        let sel = sel.repeat(val1.len());
        let nsel = rewriter.add_cell(Cell::Not(sel.clone()));
        let term1 = rewriter.add_cell(Cell::And(sel, val1.clone()));
        let term2 = rewriter.add_cell(Cell::And(nsel, val2.clone()));
        return Cell::Or(term1, term2).into();
    }
}

pub struct LowerEq;

impl RewriteRuleset for LowerEq {
    fn rewrite<'a>(
        &self,
        cell: &Cell,
        _meta: MetaItemRef<'a>,
        _output: Option<&Value>,
        rewriter: &Rewriter<'a>,
    ) -> RewriteResult<'a> {
        let &Cell::Eq(ref val1, ref val2) = cell else {
            return RewriteResult::None;
        };
        let xor = rewriter.add_cell(Cell::Xor(val1.clone(), val2.clone()));
        let xnor = rewriter.add_cell(Cell::Not(xor));
        let mut eq = Net::ONE;
        for bit in xnor {
            eq = rewriter.add_cell(Cell::And(eq.into(), bit.into()))[0];
        }
        eq.into()
    }
}

pub struct LowerLt;

impl RewriteRuleset for LowerLt {
    fn rewrite<'a>(
        &self,
        cell: &Cell,
        _meta: MetaItemRef<'a>,
        _output: Option<&Value>,
        rewriter: &Rewriter<'a>,
    ) -> RewriteResult<'a> {
        match cell {
            Cell::ULt(a, b) => {
                let b_inv = rewriter.add_cell(Cell::Not(b.clone()));
                let sub = rewriter.add_cell(Cell::Adc(a.clone(), b_inv, Net::ONE));
                Cell::Not(sub.msb().into()).into()
            }
            Cell::SLt(a, b) => {
                let a_inv = a.slice(..a.len() - 1).concat(rewriter.add_cell(Cell::Not(a.msb().into())));
                let b_inv = rewriter.add_cell(Cell::Not(b.slice(..b.len() - 1))).concat(b.msb());
                let sub = rewriter.add_cell(Cell::Adc(a_inv, b_inv, Net::ONE));
                Cell::Not(sub.msb().into()).into()
            }
            _ => RewriteResult::None,
        }
    }
}

pub struct LowerMul;

impl RewriteRuleset for LowerMul {
    fn rewrite<'a>(
        &self,
        cell: &Cell,
        _meta: MetaItemRef<'a>,
        _output: Option<&Value>,
        rewriter: &Rewriter<'a>,
    ) -> RewriteResult<'a> {
        let &Cell::Mul(ref a, ref b) = cell else {
            return RewriteResult::None;
        };
        let mut value = Value::zero(a.len());
        for (index, bit) in b.iter().enumerate() {
            value = rewriter.add_cell(Cell::Adc(
                value,
                Value::zero(index).concat(rewriter.add_cell(Cell::Mux(bit, a.clone(), Value::zero(a.len())))),
                Net::ZERO,
            ));
        }
        value.slice(..a.len()).into()
    }
}

// TODO: Div (all kinds)

pub struct LowerShift;

impl RewriteRuleset for LowerShift {
    fn rewrite<'a>(
        &self,
        cell: &Cell,
        _meta: MetaItemRef<'a>,
        _output: Option<&Value>,
        rewriter: &Rewriter<'a>,
    ) -> RewriteResult<'a> {
        enum Mode {
            Shl,
            UShr,
            SShr,
            XShr,
        }
        let (value, amount, stride, mode, overflow) = match cell {
            &Cell::Shl(ref a, ref b, stride) => (a, b, stride, Mode::Shl, Value::zero(a.len())),
            &Cell::UShr(ref a, ref b, stride) => (a, b, stride, Mode::UShr, Value::zero(a.len())),
            &Cell::SShr(ref a, ref b, stride) => (a, b, stride, Mode::SShr, a.msb().repeat(a.len())),
            &Cell::XShr(ref a, ref b, stride) => (a, b, stride, Mode::XShr, Value::undef(a.len())),
            _ => return RewriteResult::None,
        };
        let mut stride = stride as usize;
        let mut value = value.clone();
        for (index, bit) in amount.iter().enumerate() {
            if stride < value.len() {
                let shifted = match mode {
                    Mode::Shl => Value::zero(stride).concat(value.slice(..value.len() - stride)),
                    Mode::UShr => value.slice(stride..).zext(value.len()),
                    Mode::SShr => value.slice(stride..).sext(value.len()),
                    Mode::XShr => value.slice(stride..).concat(Value::undef(stride)),
                };
                value = rewriter.add_cell(Cell::Mux(bit, shifted, value));
                stride *= 2;
            } else {
                let rest = amount.slice(index..);
                let rest_len = rest.len();
                let no_overflow = rewriter.add_cell(Cell::Eq(rest, Value::zero(rest_len)));
                value = rewriter.add_cell(Cell::Mux(no_overflow[0], value, overflow));
                break;
            }
        }
        value.into()
    }
}
