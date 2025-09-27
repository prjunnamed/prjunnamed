use std::{borrow::Cow, cell::RefCell, collections::BTreeMap};

use crate::{AssignCell, Cell, Const, ControlNet, Design, MatchCell, Net, Trit, Value};

#[cfg(feature = "easy-smt")]
pub mod easy_smt;

#[derive(Debug, Clone)]
pub enum SmtResponse {
    Sat,
    Unsat,
    Unknown,
}

struct SmtTritVec<SMT: SmtEngine> {
    x: SMT::BitVec, // is undef?
    y: SMT::BitVec, // if not undef, is one?
}

pub trait SmtEngine {
    type Bool: Clone;
    type BitVec: Clone;

    fn build_bool_lit(&self, value: bool) -> Self::Bool;
    fn build_bool_eq(&self, arg1: Self::Bool, arg2: Self::Bool) -> Self::Bool;
    fn build_bool_ite(&self, cond: Self::Bool, if_true: Self::Bool, if_false: Self::Bool) -> Self::Bool;
    fn build_bool_let(&self, var: &str, expr: Self::Bool, body: impl FnOnce(Self::Bool) -> Self::Bool) -> Self::Bool;
    fn build_not(&self, arg: Self::Bool) -> Self::Bool;
    fn build_and(&self, args: &[Self::Bool]) -> Self::Bool;
    fn build_or(&self, args: &[Self::Bool]) -> Self::Bool;
    fn build_xor(&self, args: &[Self::Bool]) -> Self::Bool;

    fn build_bitvec_lit(&self, value: &Const) -> Self::BitVec;
    fn build_bitvec_eq(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::Bool;
    fn build_bitvec_ite(&self, cond: Self::Bool, if_true: Self::BitVec, if_false: Self::BitVec) -> Self::BitVec;
    fn build_bitvec_let(
        &self,
        var: &str,
        expr: Self::BitVec,
        body: impl FnOnce(Self::BitVec) -> Self::BitVec,
    ) -> Self::BitVec;
    fn build_concat(&self, arg_msb: Self::BitVec, arg_lsb: Self::BitVec) -> Self::BitVec;
    fn build_extract(&self, index_msb: usize, index_lsb: usize, arg: Self::BitVec) -> Self::BitVec;
    fn build_bvnot(&self, arg1: Self::BitVec) -> Self::BitVec;
    fn build_bvand(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvor(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvxor(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvadd(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvcomp(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvult(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::Bool;
    fn build_bvslt(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::Bool;
    fn build_bvshl(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvlshr(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvashr(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvmul(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvudiv(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvurem(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvsdiv(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;
    fn build_bvsrem(&self, arg1: Self::BitVec, arg2: Self::BitVec) -> Self::BitVec;

    type Error;

    fn declare_bool_const(&self, name: &str) -> Result<Self::Bool, Self::Error>;
    fn declare_bitvec_const(&self, name: &str, width: usize) -> Result<Self::BitVec, Self::Error>;
    fn assert(&mut self, term: Self::Bool) -> Result<(), Self::Error>;

    fn check(&mut self) -> Result<SmtResponse, Self::Error>;
    fn get_bool(&self, term: &Self::Bool) -> Result<bool, Self::Error>;
    fn get_bitvec(&self, term: &Self::BitVec) -> Result<Const, Self::Error>;
}

impl<SMT: SmtEngine> Clone for SmtTritVec<SMT> {
    fn clone(&self) -> Self {
        Self { x: self.x.clone(), y: self.y.clone() }
    }
}

pub struct SmtBuilder<'a, SMT: SmtEngine> {
    design: &'a Design,
    engine: SMT,
    temp: usize,
    curr: RefCell<BTreeMap<Net, SmtTritVec<SMT>>>,
    past: RefCell<BTreeMap<Net, SmtTritVec<SMT>>>,
    eqs: RefCell<Vec<SMT::Bool>>,
}

impl<'a, SMT: SmtEngine> SmtBuilder<'a, SMT> {
    pub fn new(design: &'a Design, engine: SMT) -> Self {
        Self {
            design,
            engine,
            temp: 0,
            curr: RefCell::new(BTreeMap::new()),
            past: RefCell::new(BTreeMap::new()),
            eqs: RefCell::new(Vec::new()),
        }
    }

    fn bv_lit<'b>(&self, value: impl Into<Cow<'b, Const>>) -> SMT::BitVec {
        self.engine.build_bitvec_lit(&*value.into())
    }

    fn bv_const(&self, prefix: &str, index: usize, suffix: &str, width: usize) -> Result<SMT::BitVec, SMT::Error> {
        self.engine.declare_bitvec_const(&format!("{prefix}{index}{suffix}"), width)
    }

    fn bv_bind(&mut self, bv_value: SMT::BitVec, width: usize) -> Result<SMT::BitVec, SMT::Error> {
        let bv_temp = self.engine.declare_bitvec_const(&format!("t{}", self.temp), width)?;
        self.temp += 1;
        self.engine.assert(self.engine.build_bitvec_eq(bv_temp.clone(), bv_value))?;
        Ok(bv_temp)
    }

    fn bv_concat(&self, elems: impl IntoIterator<IntoIter: DoubleEndedIterator<Item = SMT::BitVec>>) -> SMT::BitVec {
        let mut bv_result = None;
        for elem in elems.into_iter().rev() {
            bv_result = Some(match bv_result {
                Some(bv_msb) => self.engine.build_concat(bv_msb, elem),
                None => elem,
            })
        }
        bv_result.expect("SMT bit vectors cannot be empty")
    }

    fn bv_is_zero(&self, bv_value: SMT::BitVec, width: usize) -> SMT::Bool {
        self.engine.build_bitvec_eq(bv_value, self.bv_lit(Const::zero(width)))
    }

    fn bv_to_bool(&self, bv_value: SMT::BitVec) -> SMT::Bool {
        self.engine.build_bitvec_eq(bv_value, self.bv_lit(Trit::One))
    }

    fn bool_to_bv(&self, bool_value: SMT::Bool) -> SMT::BitVec {
        self.engine.build_bitvec_ite(bool_value, self.bv_lit(Trit::One), self.bv_lit(Trit::Zero))
    }

    fn tv_lit<'b>(&self, value: impl Into<Cow<'b, Const>>) -> SmtTritVec<SMT> {
        let value = value.into();
        let is_undef = Const::from_iter(value.iter().map(|t| if t == Trit::Undef { Trit::One } else { Trit::Zero }));
        let is_one = Const::from_iter(value.iter().map(|t| if t == Trit::One { Trit::One } else { Trit::Zero }));
        SmtTritVec { x: self.bv_lit(is_undef), y: self.bv_lit(is_one) }
    }

    fn tv_const(&self, prefix: &str, index: usize, width: usize) -> Result<SmtTritVec<SMT>, SMT::Error> {
        Ok(SmtTritVec { x: self.bv_const(prefix, index, "x", width)?, y: self.bv_const(prefix, index, "y", width)? })
    }

    fn tv_bind(&mut self, tv_value: SmtTritVec<SMT>, width: usize) -> Result<SmtTritVec<SMT>, SMT::Error> {
        let bv_temp_x = self.engine.declare_bitvec_const(&format!("t{}x", self.temp), width)?;
        let bv_temp_y = self.engine.declare_bitvec_const(&format!("t{}y", self.temp), width)?;
        self.temp += 1;
        self.engine.assert(self.engine.build_bitvec_eq(bv_temp_x.clone(), tv_value.x))?;
        self.engine.assert(self.engine.build_bitvec_eq(bv_temp_y.clone(), tv_value.y))?;
        Ok(SmtTritVec { x: bv_temp_x, y: bv_temp_y })
    }

    fn tv_concat<I>(&self, elems: I) -> SmtTritVec<SMT>
    where
        I: IntoIterator<IntoIter: DoubleEndedIterator<Item = SmtTritVec<SMT>>>,
    {
        let mut bv_result = None;
        for elem in elems.into_iter().rev() {
            bv_result = Some(match bv_result {
                Some((bv_x_msb, bv_y_msb)) => {
                    (self.engine.build_concat(bv_x_msb, elem.x), self.engine.build_concat(bv_y_msb, elem.y))
                }
                None => (elem.x, elem.y),
            })
        }
        bv_result.map(|(x, y)| SmtTritVec { x, y }).expect("SMT bit vectors cannot be empty")
    }

    fn tv_extract(&self, index_msb: usize, index_lsb: usize, tv: SmtTritVec<SMT>) -> SmtTritVec<SMT> {
        SmtTritVec {
            x: self.engine.build_extract(index_msb, index_lsb, tv.x),
            y: self.engine.build_extract(index_msb, index_lsb, tv.y),
        }
    }

    fn tv_is0(&self, tv: SmtTritVec<SMT>) -> SMT::BitVec {
        self.engine.build_bvand(self.engine.build_bvnot(tv.y), self.engine.build_bvnot(tv.x))
    }

    fn tv_is1(&self, tv: SmtTritVec<SMT>) -> SMT::BitVec {
        self.engine.build_bvand(tv.y, self.engine.build_bvnot(tv.x))
    }

    fn tv_eq(&self, tv_a: SmtTritVec<SMT>, tv_b: SmtTritVec<SMT>) -> SMT::Bool {
        let x_eq = self.engine.build_bitvec_eq(tv_a.x, tv_b.x);
        let y_eq = self.engine.build_bitvec_eq(tv_a.y, tv_b.y);
        self.engine.build_and(&[x_eq, y_eq])
    }

    fn tv_refines(&self, from: SmtTritVec<SMT>, to: SmtTritVec<SMT>) -> SMT::Bool {
        let from_def = self.engine.build_bvnot(from.x.clone());
        let x_refine = self.engine.build_bitvec_eq(
            to.x.clone(),                          // if a bit is X in `to`...
            self.engine.build_bvand(to.x, from.x), // ... it should be X in `from`.
        );
        let y_refine = self.engine.build_bitvec_eq(
            self.engine.build_bvand(from_def.clone(), from.y), // if a bit was 0/1 in `from`...
            self.engine.build_bvand(from_def.clone(), to.y),   // ... it should be that same value in `to`.
        );
        self.engine.build_and(&[x_refine, y_refine])
    }

    fn tv_not(&self, tv_a: SmtTritVec<SMT>) -> SmtTritVec<SMT> {
        SmtTritVec { y: self.engine.build_bvnot(tv_a.y), x: tv_a.x }
    }

    fn tv_and(
        &mut self,
        tv_a: SmtTritVec<SMT>,
        tv_b: SmtTritVec<SMT>,
        width: usize,
    ) -> Result<SmtTritVec<SMT>, SMT::Error> {
        //   0 1 X
        // 0 0 0 0
        // 1 0 1 X
        // X 0 X X
        let (tv_a, tv_b) = (self.tv_bind(tv_a, width)?, self.tv_bind(tv_b, width)?);
        let bv_arg1_is0 = self.tv_is0(tv_a.clone());
        let bv_arg2_is0 = self.tv_is0(tv_b.clone());
        Ok(SmtTritVec {
            y: self.engine.build_bvand(tv_a.y, tv_b.y),
            x: self.engine.build_bvand(
                self.engine.build_bvor(tv_a.x, tv_b.x),
                self.engine.build_bvnot(self.engine.build_bvor(bv_arg1_is0, bv_arg2_is0)),
            ),
        })
    }

    fn tv_or(
        &mut self,
        tv_a: SmtTritVec<SMT>,
        tv_b: SmtTritVec<SMT>,
        width: usize,
    ) -> Result<SmtTritVec<SMT>, SMT::Error> {
        //   0 1 X
        // 0 0 1 X
        // 1 1 1 1
        // X X 1 1
        let (tv_a, tv_b) = (self.tv_bind(tv_a, width)?, self.tv_bind(tv_b, width)?);
        let bv_arg1_is1 = self.tv_is1(tv_a.clone());
        let bv_arg2_is1 = self.tv_is1(tv_b.clone());
        Ok(SmtTritVec {
            y: self.engine.build_bvor(tv_a.y, tv_b.y),
            x: self.engine.build_bvand(
                self.engine.build_bvor(tv_a.x, tv_b.x),
                self.engine.build_bvnot(self.engine.build_bvor(bv_arg1_is1, bv_arg2_is1)),
            ),
        })
    }

    fn tv_xor(&self, tv_a: SmtTritVec<SMT>, tv_b: SmtTritVec<SMT>) -> SmtTritVec<SMT> {
        //   0 1 X
        // 0 0 1 X
        // 1 1 0 X
        // X X X X
        SmtTritVec { y: self.engine.build_bvxor(tv_a.y, tv_b.y), x: self.engine.build_bvor(tv_a.x, tv_b.x) }
    }

    fn tv_mux(
        &mut self,
        tv_s: SmtTritVec<SMT>,
        tv_a: SmtTritVec<SMT>,
        tv_b: SmtTritVec<SMT>,
        width: usize,
    ) -> Result<SmtTritVec<SMT>, SMT::Error> {
        // S A B O    S A B O    S A B O
        // 0 ? 0 0    1 0 ? 0    X 0 0 0
        // 0 ? 1 1    1 1 ? 1    X 1 1 1
        // 0 ? X X    1 X ? X    X ? ? X
        let (tv_s, tv_a, tv_b) = (self.tv_bind(tv_s, 1)?, self.tv_bind(tv_a, width)?, self.tv_bind(tv_b, width)?);
        let (bv_s_is0, bv_s_is1) = (self.tv_is0(tv_s.clone()), self.tv_is1(tv_s));
        let (bool_s_is0, bool_s_is1) = (self.bv_to_bool(bv_s_is0), self.bv_to_bool(bv_s_is1));
        let bv_x_sx = self.engine.build_bvxor(tv_a.y.clone(), tv_b.y.clone());
        let bv_x_sx = self.engine.build_bvor(bv_x_sx, tv_a.x.clone());
        let bv_x_sx = self.engine.build_bvor(bv_x_sx, tv_b.x.clone());
        let bv_x_s1 = self.engine.build_bitvec_ite(bool_s_is1.clone(), tv_a.x, bv_x_sx);
        let bv_x_s0 = self.engine.build_bitvec_ite(bool_s_is0.clone(), tv_b.x, bv_x_s1);
        let bv_y_sx = self.engine.build_bvand(tv_a.y.clone(), tv_b.y.clone());
        let bv_y_s1 = self.engine.build_bitvec_ite(bool_s_is1, tv_a.y, bv_y_sx);
        let bv_y_s0 = self.engine.build_bitvec_ite(bool_s_is0, tv_b.y, bv_y_s1);
        Ok(SmtTritVec { y: bv_y_s0, x: bv_x_s0 })
    }

    fn curr_net(&self, net: Net) -> Result<SmtTritVec<SMT>, SMT::Error> {
        match net.as_cell_index() {
            Err(trit) => Ok(self.tv_lit(trit)),
            Ok(cell_index) => {
                let tv_net = self.tv_const("n", cell_index, 1)?;
                self.curr.borrow_mut().insert(net, tv_net.clone());
                Ok(tv_net)
            }
        }
    }

    fn curr_value(&self, value: &Value) -> Result<SmtTritVec<SMT>, SMT::Error> {
        Ok(self.tv_concat(value.iter().map(|net| self.curr_net(net)).collect::<Result<Vec<_>, _>>()?.into_iter()))
    }

    fn past_net(&self, net: Net) -> Result<SmtTritVec<SMT>, SMT::Error> {
        match net.as_cell_index() {
            Err(trit) => Ok(self.tv_lit(trit)),
            Ok(cell_index) => {
                let tv_net = self.tv_const("p", cell_index, 1)?;
                self.curr.borrow_mut().insert(net, tv_net.clone());
                Ok(tv_net)
            }
        }
    }

    fn past_value(&self, value: &Value) -> Result<SmtTritVec<SMT>, SMT::Error> {
        Ok(self.tv_concat(value.iter().map(|net| self.past_net(net)).collect::<Result<Vec<_>, _>>()?.into_iter()))
    }

    fn net(&self, net: Net) -> Result<SmtTritVec<SMT>, SMT::Error> {
        if let Ok(index) = net.as_cell_index() {
            if !self.design.is_valid_cell_index(index) {
                return self.curr_net(net); // FIXME: is this even sound?
            }
        }
        match self.design.find_cell(net) {
            Ok((cell_ref, _offset)) if matches!(&*cell_ref.get(), Cell::Dff(_)) => self.past_net(net),
            _ => self.curr_net(net),
        }
    }

    fn value(&self, value: &Value) -> Result<SmtTritVec<SMT>, SMT::Error> {
        Ok(self.tv_concat(value.iter().map(|net| self.net(net)).collect::<Result<Vec<_>, _>>()?.into_iter()))
    }

    fn invert_net(&self, control_net: ControlNet, tv_net: SmtTritVec<SMT>) -> Result<SmtTritVec<SMT>, SMT::Error> {
        let active = if control_net.is_positive() { Const::ones(1) } else { Const::zero(1) };
        Ok(SmtTritVec { y: self.engine.build_bvcomp(tv_net.y, self.engine.build_bitvec_lit(&active)), x: tv_net.x })
    }

    fn control_net(&self, control_net: ControlNet) -> Result<SmtTritVec<SMT>, SMT::Error> {
        self.invert_net(control_net, self.net(control_net.net())?)
    }

    fn clock_net(&mut self, control_net: ControlNet) -> Result<SmtTritVec<SMT>, SMT::Error> {
        self.tv_and(
            self.invert_net(control_net, self.curr_net(control_net.net())?)?,
            self.tv_not(self.invert_net(control_net, self.past_net(control_net.net())?)?),
            1,
        )
    }

    fn cell(&mut self, output: &Value, cell: &Cell) -> Result<SmtTritVec<SMT>, SMT::Error> {
        let prepare_shift = |value: &Value, amount: &Value, stride: u32, signed: bool| -> Result<_, SMT::Error> {
            let width = value.len().max(amount.len() + stride.max(1).ilog2() as usize + 1);
            let value_ext = if signed { value.sext(width) } else { value.zext(width) };
            let amount_ext = amount.zext(width);
            let (tv_value_ext, tv_amount_ext) = (self.value(&value_ext)?, self.value(&amount_ext)?);
            let bv_stride = self.engine.build_bitvec_lit(&Const::from_uint(stride as u128, width));
            let bv_amount_mul = self.engine.build_bvmul(tv_amount_ext.y.clone(), bv_stride);
            Ok((width, tv_value_ext, tv_amount_ext, bv_amount_mul))
        };

        let bv_cell = match &*cell {
            Cell::Buf(a) => self.value(a)?,
            Cell::Not(a) => self.tv_not(self.value(a)?),
            Cell::And(a, b) => self.tv_and(self.value(a)?, self.value(b)?, output.len())?,
            Cell::Or(a, b) => self.tv_or(self.value(a)?, self.value(b)?, output.len())?,
            Cell::Xor(a, b) => self.tv_xor(self.value(a)?, self.value(b)?),
            Cell::Mux(s, a, b) => self.tv_mux(self.net(*s)?, self.value(a)?, self.value(b)?, output.len())?,
            Cell::Adc(a, b, c) => {
                if a.is_empty() {
                    self.net(*c)?
                } else {
                    let (tv_a, tv_b, tv_c) = (self.value(a)?, self.value(b)?, self.net(*c)?);
                    let (tv_a, tv_b) = (self.tv_bind(tv_a, a.len())?, self.tv_bind(tv_b, b.len())?);
                    let mut bv_x = vec![];
                    let mut bv_carry_x = tv_c.x;
                    for index in 0..a.len() {
                        bv_carry_x = self.bv_bind(
                            self.engine.build_bvor(
                                bv_carry_x,
                                self.engine.build_bvor(
                                    self.engine.build_extract(index, index, tv_a.x.clone()),
                                    self.engine.build_extract(index, index, tv_b.x.clone()),
                                ),
                            ),
                            1,
                        )?;
                        bv_x.push(bv_carry_x.clone());
                    }
                    bv_x.push(bv_carry_x.clone());
                    SmtTritVec {
                        x: self.bv_concat(bv_x),
                        y: self.engine.build_bvadd(
                            self.engine.build_bvadd(
                                self.engine.build_concat(self.bv_lit(Trit::Zero), tv_a.y),
                                self.engine.build_concat(self.bv_lit(Trit::Zero), tv_b.y),
                            ),
                            self.engine.build_concat(self.bv_lit(Const::zero(a.len())), tv_c.y),
                        ),
                    }
                }
            }
            Cell::Aig(a, b) => self.tv_and(self.control_net(*a)?, self.control_net(*b)?, 1)?,
            Cell::Eq(a, b) => {
                let (tv_a, tv_b) = (self.value(a)?, self.value(b)?);
                let bv_a_xor_b = self.engine.build_bvxor(tv_a.y.clone(), tv_b.y.clone());
                let bv_unequal = self.engine.build_bvand(
                    bv_a_xor_b,
                    self.engine.build_bvnot(self.engine.build_bvor(tv_a.x.clone(), tv_b.x.clone())),
                );
                let bool_any_unequal = self.engine.build_not(self.bv_is_zero(bv_unequal, a.len()));
                SmtTritVec {
                    x: self.bool_to_bv(self.engine.build_bool_ite(
                        bool_any_unequal,
                        self.engine.build_bool_lit(false),
                        self.engine.build_not(self.engine.build_and(&[
                            self.bv_is_zero(tv_a.x.clone(), a.len()),
                            self.bv_is_zero(tv_b.x.clone(), b.len()),
                        ])),
                    )),
                    y: self.engine.build_bvcomp(tv_a.y, tv_b.y),
                }
            }
            Cell::ULt(a, b) => {
                let (tv_a, tv_b) = (self.value(a)?, self.value(b)?);
                SmtTritVec {
                    x: self.bool_to_bv(self.engine.build_not(
                        self.engine.build_and(&[self.bv_is_zero(tv_a.x, a.len()), self.bv_is_zero(tv_b.x, b.len())]),
                    )),
                    y: self.bool_to_bv(self.engine.build_bvult(tv_a.y, tv_b.y)),
                }
            }
            Cell::SLt(a, b) => {
                let (tv_a, tv_b) = (self.value(a)?, self.value(b)?);
                SmtTritVec {
                    x: self.bool_to_bv(self.engine.build_not(
                        self.engine.build_and(&[self.bv_is_zero(tv_a.x, a.len()), self.bv_is_zero(tv_b.x, b.len())]),
                    )),
                    y: self.bool_to_bv(self.engine.build_bvslt(tv_a.y, tv_b.y)),
                }
            }
            Cell::Shl(value, amount, stride) => {
                let (width, tv_value_ext, tv_amount_ext, bv_amount_mul) = prepare_shift(value, amount, *stride, false)?;
                let tv_result = SmtTritVec {
                    x: self.engine.build_bitvec_ite(
                        self.bv_is_zero(tv_amount_ext.x, width),
                        self.engine.build_bvshl(tv_value_ext.x, bv_amount_mul.clone()),
                        self.engine.build_bitvec_lit(&Const::ones(width)),
                    ),
                    y: self.engine.build_bvshl(tv_value_ext.y, bv_amount_mul),
                };
                self.tv_extract(value.len() - 1, 0, tv_result)
            }
            Cell::UShr(value, amount, stride) => {
                let (width, tv_value_ext, tv_amount_ext, bv_amount_mul) = prepare_shift(value, amount, *stride, false)?;
                let tv_result = SmtTritVec {
                    x: self.engine.build_bitvec_ite(
                        self.bv_is_zero(tv_amount_ext.x, width),
                        self.engine.build_bvlshr(tv_value_ext.x, bv_amount_mul.clone()),
                        self.engine.build_bitvec_lit(&Const::ones(width)),
                    ),
                    y: self.engine.build_bvlshr(tv_value_ext.y, bv_amount_mul),
                };
                self.tv_extract(value.len() - 1, 0, tv_result)
            }
            Cell::SShr(value, amount, stride) => {
                let (width, tv_value_ext, tv_amount_ext, bv_amount_mul) = prepare_shift(value, amount, *stride, true)?;
                let tv_result = SmtTritVec {
                    x: self.engine.build_bitvec_ite(
                        self.bv_is_zero(tv_amount_ext.x, width),
                        self.engine.build_bvashr(tv_value_ext.x, bv_amount_mul.clone()),
                        self.engine.build_bitvec_lit(&Const::ones(width)),
                    ),
                    y: self.engine.build_bvashr(tv_value_ext.y, bv_amount_mul),
                };
                self.tv_extract(value.len() - 1, 0, tv_result)
            }
            Cell::XShr(value, amount, stride) => {
                let (width, tv_value_ext, tv_amount_ext, bv_amount_mul) =
                    prepare_shift(&value.concat(Value::undef(1)), amount, *stride, true)?;
                let tv_result = SmtTritVec {
                    x: self.engine.build_bitvec_ite(
                        self.bv_is_zero(tv_amount_ext.x, width),
                        self.engine.build_bvashr(tv_value_ext.x, bv_amount_mul.clone()),
                        self.engine.build_bitvec_lit(&Const::ones(width)),
                    ),
                    y: self.engine.build_bvlshr(tv_value_ext.y, bv_amount_mul),
                };
                self.tv_extract(value.len() - 1, 0, tv_result)
            }
            Cell::Mul(a, b) => {
                let (tv_a, tv_b) = (self.value(a)?, self.value(b)?);
                SmtTritVec {
                    x: self.engine.build_bitvec_ite(
                        self.engine.build_and(&[self.bv_is_zero(tv_a.x, a.len()), self.bv_is_zero(tv_b.x, b.len())]),
                        self.engine.build_bitvec_lit(&Const::zero(a.len())),
                        self.engine.build_bitvec_lit(&Const::ones(a.len())),
                    ),
                    y: self.engine.build_bvmul(tv_a.y, tv_b.y),
                }
            }
            Cell::UDiv(..) => unimplemented!("lowering of udiv to SMT-LIB is not implemented"),
            Cell::UMod(..) => unimplemented!("lowering of umod to SMT-LIB is not implemented"),
            Cell::SDivTrunc(..) => unimplemented!("lowering of sdiv_trunc to SMT-LIB is not implemented"),
            Cell::SDivFloor(..) => unimplemented!("lowering of sdiv_floor to SMT-LIB is not implemented"),
            Cell::SModTrunc(..) => unimplemented!("lowering of smod_trunc to SMT-LIB is not implemented"),
            Cell::SModFloor(..) => unimplemented!("lowering of smod_floor to SMT-LIB is not implemented"),
            Cell::Match(MatchCell { value, enable, patterns }) => {
                let mut tv_matches = vec![];
                for alternates in patterns {
                    let mut tv_sum = self.tv_lit(Trit::Zero);
                    for pattern in alternates {
                        let mut tv_prd = self.tv_lit(Trit::One);
                        for (index, mask) in pattern.iter().enumerate().filter(|(_index, mask)| *mask != Trit::Undef) {
                            let tv_eq = self.tv_not(self.tv_xor(self.net(value[index])?, self.net(mask.into())?));
                            tv_prd = self.tv_and(tv_prd, tv_eq, 1)?;
                        }
                        tv_sum = self.tv_or(tv_sum, tv_prd, 1)?;
                    }
                    tv_matches.push(tv_sum);
                }
                let tv_matches = self.tv_bind(self.tv_concat(tv_matches), patterns.len())?;
                let tv_enable = self.net(*enable)?;
                let tv_all_cold = self.tv_lit(&Const::zero(patterns.len()));
                let mut tv_result = tv_all_cold.clone();
                for index in (0..patterns.len()).rev() {
                    tv_result = self.tv_mux(
                        self.tv_extract(index, index, tv_matches.clone()),
                        self.tv_lit(&Const::one_hot(patterns.len(), index)),
                        tv_result,
                        patterns.len(),
                    )?;
                }
                self.tv_mux(tv_enable.clone(), tv_result, tv_all_cold, patterns.len())?
            }
            Cell::Assign(AssignCell { value, enable, update, offset }) => self.tv_mux(
                self.net(*enable)?,
                self.value(&{
                    let mut nets = Vec::from_iter(value.iter());
                    nets[*offset..(*offset + update.len())].copy_from_slice(&update[..]);
                    Value::from_iter(nets)
                })?,
                self.value(value)?,
                output.len(),
            )?,
            Cell::Dff(flip_flop) => {
                let mut data = self.value(&flip_flop.data)?;
                let clear = self.control_net(flip_flop.clear)?;
                let reset = self.control_net(flip_flop.reset)?;
                let enable = self.control_net(flip_flop.enable)?;
                if flip_flop.has_load() {
                    panic!("load-enabled flip-flops are not supported yet");
                }
                if flip_flop.reset_over_enable {
                    data = self.tv_mux(enable, data, self.past_value(&output)?, output.len())?;
                    data = self.tv_mux(reset, self.tv_lit(&flip_flop.reset_value), data, output.len())?;
                } else {
                    data = self.tv_mux(reset, self.tv_lit(&flip_flop.reset_value), data, output.len())?;
                    data = self.tv_mux(enable, data, self.past_value(&output)?, output.len())?;
                }
                let active_edge = self.clock_net(flip_flop.clock)?;
                let value = self.tv_mux(active_edge, data, self.past_value(&output)?, output.len())?;
                if flip_flop.has_clear() {
                    self.tv_mux(clear, self.tv_lit(&flip_flop.clear_value), value, output.len())?
                } else {
                    value
                }
            }
            Cell::Memory(_memory) => unimplemented!("memories are not lowered to SMT-LIB yet"),
            Cell::IoBuf(_io_buffer) => self.value(output)?, // i/en/o treated as POs/PIs
            Cell::Target(_target_cell) => unimplemented!("target cells cannot be lowered to SMT-LIB yet"),
            Cell::Other(_) => unreachable!("instances cannot be lowered to SMT-LIB"),
            Cell::Input(..) | Cell::Output(..) | Cell::Name(..) | Cell::Debug(..) => unreachable!(),
        };

        Ok(bv_cell)
    }

    pub fn add_cell(&mut self, output: &Value, cell: &Cell) -> Result<(), SMT::Error> {
        // Declare the nets used by the cell so that it is present in the counterexample even if unused.
        if let Cell::Input(..) = cell {
            self.curr_value(output)?;
            return Ok(());
        }
        let tv_cell = self.cell(output, cell)?;
        self.engine.assert(self.tv_eq(self.curr_value(output)?, tv_cell))
    }

    pub fn replace_cell(&mut self, output: &Value, old_cell: &Cell, new_cell: &Cell) -> Result<(), SMT::Error> {
        self.add_cell(output, old_cell)?;
        let tv_new_cell = self.cell(output, new_cell)?;
        self.eqs.borrow_mut().push(self.tv_refines(self.curr_value(output)?, tv_new_cell));
        Ok(())
    }

    pub fn replace_net(&mut self, from_net: Net, to_net: Net) -> Result<(), SMT::Error> {
        self.eqs.borrow_mut().push(self.tv_refines(self.curr_net(from_net)?, self.net(to_net)?));
        Ok(())
    }

    pub fn replace_void_net(&mut self, from_net: Net, to_net: Net) -> Result<(), SMT::Error> {
        self.engine.assert(self.tv_eq(self.curr_net(from_net)?, self.net(to_net)?))
    }

    pub fn replace_dff_net(&mut self, from_net: Net, to_net: Net) -> Result<(), SMT::Error> {
        // Essentially a single induction step.
        self.engine.assert(self.tv_eq(self.past_net(from_net)?, self.past_net(to_net)?))?;
        self.eqs.borrow_mut().push(self.tv_refines(self.curr_net(from_net)?, self.curr_net(to_net)?));
        Ok(())
    }

    pub fn check(&mut self) -> Result<Option<SmtExample>, SMT::Error> {
        if self.eqs.borrow().is_empty() {
            return Ok(None);
        }
        let not_and_eqs = self.engine.build_not(self.engine.build_and(&self.eqs.borrow()[..]));
        self.engine.assert(not_and_eqs)?;
        match self.engine.check()? {
            SmtResponse::Unknown => panic!("SMT solver returned unknown"),
            SmtResponse::Unsat => Ok(None),
            SmtResponse::Sat => {
                let get_trit = |tv_net: &SmtTritVec<SMT>| -> Result<Trit, SMT::Error> {
                    if self.engine.get_bitvec(&tv_net.x)?[0] == Trit::One {
                        Ok(Trit::Undef)
                    } else {
                        Ok(self.engine.get_bitvec(&tv_net.y)?[0])
                    }
                };
                let (mut curr, mut past) = (BTreeMap::new(), BTreeMap::new());
                for (net, tv_net) in self.curr.borrow().iter() {
                    curr.insert(*net, get_trit(tv_net)?);
                }
                for (net, tv_net) in self.past.borrow().iter() {
                    past.insert(*net, get_trit(tv_net)?);
                }
                Ok(Some(SmtExample { curr, past }))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtExample {
    curr: BTreeMap<Net, Trit>,
    past: BTreeMap<Net, Trit>,
}

impl SmtExample {
    pub fn get_net(&self, net: Net) -> Option<Trit> {
        self.curr.get(&net).cloned()
    }

    pub fn get_value<'a>(&self, value: impl Into<Cow<'a, Value>>) -> Option<Const> {
        let mut result = Const::new();
        for net in &*value.into() {
            result.push(self.get_net(net)?);
        }
        Some(result)
    }

    pub fn get_past_net(&self, net: Net) -> Option<Trit> {
        self.past.get(&net).cloned()
    }

    pub fn get_past_value<'a>(&self, value: impl Into<Cow<'a, Value>>) -> Option<Const> {
        let mut result = Const::new();
        for net in &*value.into() {
            result.push(self.get_past_net(net)?);
        }
        Some(result)
    }
}
