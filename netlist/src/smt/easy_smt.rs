use std::cell::{Ref, RefCell, RefMut};
use std::collections::BTreeMap;

use easy_smt::{Response, SExpr, SExprData};

use crate::{Trit, Const};
use super::{SmtEngine, SmtResponse};

pub struct EasySmtEngine(RefCell<EasySmtState>);
struct EasySmtState {
    context: easy_smt::Context,
    declares: BTreeMap<String, SExpr>,
}

impl EasySmtEngine {
    pub fn z3() -> Result<Self, std::io::Error> {
        Ok(Self::new(easy_smt::ContextBuilder::new().solver("z3").solver_args(["-smt2", "-in"]).build()?))
    }

    pub fn new(context: easy_smt::Context) -> Self {
        Self(RefCell::new(EasySmtState { context, declares: BTreeMap::new() }))
    }

    fn context(&self) -> Ref<'_, easy_smt::Context> {
        Ref::map(self.0.borrow(), |r| &r.context)
    }

    fn context_mut(&self) -> RefMut<'_, easy_smt::Context> {
        RefMut::map(self.0.borrow_mut(), |r| &mut r.context)
    }
}

impl SmtEngine for EasySmtEngine {
    type Bool = SExpr;
    type BitVec = SExpr;

    fn build_bool_lit(&self, value: bool) -> Self::Bool {
        if value {
            self.context().true_()
        } else {
            self.context().false_()
        }
    }

    fn build_bool_eq(&self, arg1: Self::Bool, arg2: Self::Bool) -> Self::Bool {
        self.context().eq(arg1, arg2)
    }

    fn build_bool_ite(&self, cond: Self::Bool, if_true: Self::Bool, if_false: Self::Bool) -> Self::Bool {
        self.context().ite(cond, if_true, if_false)
    }

    fn build_bool_let(&self, var: &str, expr: Self::Bool, body: impl FnOnce(Self::Bool) -> Self::Bool) -> Self::Bool {
        let ctx = self.context();
        let body = body(ctx.atom(format!("|{var}|")));
        ctx.let_(var, expr, body)
    }

    fn build_not(&self, arg: Self::Bool) -> Self::Bool {
        self.context().not(arg)
    }

    fn build_and(&self, args: &[Self::Bool]) -> Self::Bool {
        self.context().and_many(args.iter().cloned())
    }

    fn build_or(&self, args: &[Self::Bool]) -> Self::Bool {
        self.context().or_many(args.iter().cloned())
    }

    fn build_xor(&self, args: &[Self::Bool]) -> Self::Bool {
        self.context().xor_many(args.iter().cloned())
    }

    fn build_bitvec_lit(&self, value: &Const) -> Self::BitVec {
        assert!(!value.iter().any(|trit| trit == Trit::Undef));
        self.context().atom(format!("#b{value}"))
    }

    fn build_bitvec_eq(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::Bool {
        self.context().eq(arg1, arg2)
    }

    fn build_bitvec_ite(&self, cond: Self::Bool, if_true: Self::BitVec, if_false: Self::BitVec) -> Self::BitVec {
        self.context().ite(cond, if_true, if_false)
    }

    fn build_bitvec_let(
        &self,
        var: &str,
        expr: Self::BitVec,
        body: impl FnOnce(Self::BitVec) -> Self::BitVec,
    ) -> Self::BitVec {
        let ctx = self.context();
        let body = body(ctx.atom(format!("|{var}|")));
        ctx.let_(var, expr, body)
    }

    fn build_concat(&self, arg_msb: Self::BitVec, arg_lsb: Self::BitVec) -> Self::BitVec {
        self.context().concat(arg_msb, arg_lsb)
    }

    fn build_extract(&self, index_msb: usize, index_lsb: usize, arg: Self::BitVec) -> Self::BitVec {
        self.context().extract(index_msb.try_into().unwrap(), index_lsb.try_into().unwrap(), arg)
    }

    fn build_bvnot(&self, arg1: Self::BitVec) -> Self::BitVec {
        self.context().bvnot(arg1)
    }

    fn build_bvand(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvand(arg1, arg2)
    }

    fn build_bvor(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvor(arg1, arg2)
    }

    fn build_bvxor(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvxor(arg1, arg2)
    }

    fn build_bvadd(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvadd(arg1, arg2)
    }

    fn build_bvcomp(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvcomp(arg1, arg2)
    }

    fn build_bvult(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::Bool {
        self.context().bvult(arg1, arg2)
    }

    fn build_bvslt(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::Bool {
        self.context().bvslt(arg1, arg2)
    }

    fn build_bvshl(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvshl(arg1, arg2)
    }

    fn build_bvlshr(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvlshr(arg1, arg2)
    }

    fn build_bvashr(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvashr(arg1, arg2)
    }

    fn build_bvmul(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvmul(arg1, arg2)
    }

    fn build_bvudiv(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvudiv(arg1, arg2)
    }

    fn build_bvurem(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvurem(arg1, arg2)
    }

    fn build_bvsdiv(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvsdiv(arg1, arg2)
    }

    fn build_bvsrem(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec {
        self.context().bvsrem(arg1, arg2)
    }

    type Error = std::io::Error;

    fn declare_bool_const(&self, name: &str) -> Result<Self::Bool, Self::Error> {
        let mut state = self.0.borrow_mut();
        if let Some(sexpr) = state.declares.get(name) {
            return Ok(*sexpr);
        }
        let sort = state.context.bool_sort();
        let sexpr = state.context.declare_const(name, sort)?;
        state.declares.insert(name.to_owned(), sexpr);
        Ok(sexpr)
    }

    fn declare_bitvec_const(&self, name: &str, width: usize) -> Result<Self::BitVec, Self::Error> {
        let mut state = self.0.borrow_mut();
        if let Some(sexpr) = state.declares.get(name) {
            return Ok(*sexpr);
        }
        let sort = state.context.bit_vec_sort(state.context.numeral(width));
        let sexpr = state.context.declare_const(name, sort)?;
        state.declares.insert(name.to_owned(), sexpr);
        Ok(sexpr)
    }

    fn assert(&mut self, term: Self::Bool) -> Result<(), Self::Error> {
        let mut ctx = self.context_mut();
        ctx.assert(term)
    }

    fn check(&mut self) -> Result<SmtResponse, Self::Error> {
        let mut ctx = self.context_mut();
        match ctx.check()? {
            Response::Sat => Ok(SmtResponse::Sat),
            Response::Unsat => Ok(SmtResponse::Unsat),
            Response::Unknown => Ok(SmtResponse::Unknown),
        }
    }

    fn get_bool(&self, term: &Self::Bool) -> Result<bool, Self::Error> {
        let value = self.get_value(*term)?;
        let ctx = self.context();
        if value == ctx.atom("true") {
            return Ok(true);
        } else if value == ctx.atom("false") {
            return Ok(false);
        }
        panic!("illegal Bool value: {:?}", ctx.get(value))
    }

    fn get_bitvec(&self, term: &Self::BitVec) -> Result<Const, Self::Error> {
        let value = self.get_value(*term)?;
        let ctx = self.context();
        if let SExprData::Atom(data) = ctx.get(value) {
            if data.starts_with("#b") {
                return Ok(Const::lit(&data[2..]));
            }
        }
        panic!("illegal BitVec value: {:?}", ctx.get(value))
    }
}

impl EasySmtEngine {
    fn get_value(&self, term: SExpr) -> Result<SExpr, std::io::Error> {
        let mut ctx = self.context_mut();
        let response = ctx.get_value(vec![term])?;
        assert_eq!(response.len(), 1);
        assert_eq!(response[0].0, term);
        Ok(response[0].1)
    }
}
