use std::collections::{hash_map, HashMap};

use pest::{error::ErrorVariant, Parser, Span};
use prjunnamed_entity::{EntityId, EntityPartVec, EntityVec};
use smallvec::{smallvec, SmallVec};

use crate::model::{
    annotations::{
        Attribute, AttributeValue, BitIndexingKind, CellAnnotation, DesignAnnotation, HierName,
        HierNameChunk, ModuleAnnotation,
    },
    bits::{Bit, Bits},
    cells::{
        AddSub, BitOp, BitOpKind, BlackboxBuf, Buf, Bus, BusDriver, BusJoiner, BusKind, BusSwizzle,
        BusSwizzleChunk, CellKind, ClockEdge, ClockTrigger, Cmp, CmpKind, Ext, ExtKind, Instance,
        InstanceOutput, Mul, Mux, MuxKind, Param, ParamType, PortBinding, PortBus, PortIn, PortOut,
        Register, RegisterRule, Shift, ShiftKind, Slice, Switch, SwitchCase, SwitchKind, Swizzle,
        SwizzleChunk, UnaryXor, UnresolvedInstance, Wire,
    },
    float::F64BitEq,
    CellId, CellPlane, CellRefMut, CellType, Design, ModuleId, ModuleRef, ModuleRefMut, ParamId,
    PortBusId, PortInId, PortOutId, StrId,
};

#[derive(pest_derive::Parser)]
#[grammar = "text.pest"]
struct TextParser;

pub type Error = pest::error::Error<Rule>;
type Pair<'a> = pest::iterators::Pair<'a, Rule>;
type Pairs<'a> = pest::iterators::Pairs<'a, Rule>;

fn error(span: Span, msg: impl Into<String>) -> Box<Error> {
    Error::new_from_span(
        ErrorVariant::CustomError {
            message: msg.into(),
        },
        span,
    )
    .into()
}

trait Interner {
    fn string(&self, id: StrId) -> &str;
    fn intern(&mut self, s: &str) -> StrId;
}

macro_rules! impl_interner {
    ($t: ty) => {
        impl Interner for $t {
            fn string(&self, id: StrId) -> &str {
                self.string(id)
            }

            fn intern(&mut self, s: &str) -> StrId {
                self.intern(s)
            }
        }
    };
}

impl_interner!(Design);
impl_interner!(ModuleRefMut<'_>);
impl_interner!(CellRefMut<'_>);

fn parse_string_raw(pair: Pair) -> Result<String, Box<Error>> {
    assert_eq!(pair.as_rule(), Rule::string);
    let mut res = String::new();
    for pair in pair.into_inner() {
        let s = pair.as_str();
        if !s.starts_with('\\') {
            res += s;
        } else if s == "\\\\" {
            res.push('\\');
        } else if s == "\\\"" {
            res.push('"');
        } else if let Some(s) = s.strip_prefix("\\x") {
            let n = u32::from_str_radix(s, 16).unwrap();
            let c = char::try_from(n).unwrap();
            res.push(c);
        } else if let Some(s) = s.strip_prefix("\\u{") {
            let s = s.strip_suffix('}').unwrap();
            let n = u32::from_str_radix(s, 16)
                .map_err(|_| error(pair.as_span(), "invalid unicode escape"))?;
            let c =
                char::try_from(n).map_err(|_| error(pair.as_span(), "invalid unicode escape"))?;
            res.push(c);
        }
    }
    Ok(res)
}

fn parse_string(int: &mut impl Interner, pair: Pair) -> Result<StrId, Box<Error>> {
    Ok(int.intern(&parse_string_raw(pair)?))
}

fn parse_global_id<'a>(pair: &Pair<'a>) -> &'a str {
    assert_eq!(pair.as_rule(), Rule::global_id);
    &pair.as_str()[1..]
}

fn parse_local_id<'a>(pair: &Pair<'a>) -> &'a str {
    assert_eq!(pair.as_rule(), Rule::local_id);
    &pair.as_str()[1..]
}

fn parse_uint(pair: Pair) -> Result<u32, Box<Error>> {
    assert_eq!(pair.as_rule(), Rule::uint);
    pair.as_str()
        .parse()
        .map_err(|_| error(pair.as_span(), "integer too large"))
}

fn parse_int(pair: Pair) -> Result<i32, Box<Error>> {
    assert_eq!(pair.as_rule(), Rule::int);
    pair.as_str()
        .parse()
        .map_err(|_| error(pair.as_span(), "integer too large"))
}

fn parse_float(pair: Pair) -> Result<F64BitEq, Box<Error>> {
    assert_eq!(pair.as_rule(), Rule::float);
    let s = pair.as_str();
    let f = if let Some(digits) = s.strip_prefix("f64'h") {
        f64::from_bits(u64::from_str_radix(digits, 16).unwrap())
    } else {
        pair.as_str()
            .parse()
            .map_err(|_| error(pair.as_span(), "integer too large"))?
    };
    Ok(F64BitEq(f))
}

fn parse_width(pair: Pair) -> Result<u32, Box<Error>> {
    assert_eq!(pair.as_rule(), Rule::width);
    parse_uint(pair.into_inner().next().unwrap())
}

fn parse_bits(pair: Pair) -> Result<Bits, Box<Error>> {
    let mut res = Bits {
        bits: SmallVec::new(),
    };
    let s = pair.as_str();
    let (width, digits) = s.split_once('\'').unwrap();
    let width: usize = width
        .parse()
        .map_err(|_| error(pair.as_span(), "bit vector too large"))?;
    if let Some(digits) = digits.strip_prefix('b') {
        for c in digits.chars().rev() {
            match c {
                '0' => res.bits.push(Bit::_0),
                '1' => res.bits.push(Bit::_1),
                'x' | 'X' => res.bits.push(Bit::X),
                _ => unreachable!(),
            }
        }
    } else if let Some(digits) = digits.strip_prefix('h') {
        for c in digits.chars().rev() {
            if matches!(c, 'x' | 'X') {
                for _ in 0..4 {
                    res.bits.push(Bit::X);
                }
            } else {
                let d = c.to_digit(16).unwrap();
                for i in 0..4 {
                    let bit = if (d >> i & 1) != 0 { Bit::_1 } else { Bit::_0 };
                    res.bits.push(bit);
                }
            }
        }
        while res.bits.len() < width {
            res.bits.push(Bit::_0);
        }
        while res.bits.len() > width {
            let bit = res.bits.pop().unwrap();
            if bit == Bit::_1 {
                return Err(error(pair.as_span(), "hex value doesn't fit in width"));
            }
        }
    } else if digits == "x" {
        res.bits = smallvec![Bit::X; width];
    } else {
        unreachable!()
    }
    if res.bits.len() != width {
        return Err(error(pair.as_span(), "bit vector length mismatch"));
    }
    Ok(res)
}

fn parse_inv(pairs: &mut Pairs) -> bool {
    if let Some(pair) = pairs.peek() {
        if pair.as_rule() == Rule::kw_inv {
            pairs.next();
            return true;
        }
    }
    false
}

fn parse_bus_kind(pairs: &mut Pairs) -> BusKind {
    if let Some(pair) = pairs.peek() {
        if pair.as_rule() == Rule::kw_bus_kind {
            pairs.next();
            match pair.as_str() {
                "pulldown" => BusKind::Pulldown,
                "pullup" => BusKind::Pullup,
                "wireand" => BusKind::WireAnd,
                "wireor" => BusKind::WireOr,
                _ => unreachable!(),
            }
        } else {
            BusKind::Plain
        }
    } else {
        BusKind::Plain
    }
}

fn parse_attr_val(int: &mut impl Interner, pair: Pair) -> Result<AttributeValue, Box<Error>> {
    match pair.as_rule() {
        Rule::string => Ok(AttributeValue::String(parse_string(int, pair)?)),
        Rule::int => Ok(AttributeValue::Int(parse_int(pair)?)),
        Rule::float => Ok(AttributeValue::Float(parse_float(pair)?)),
        Rule::bits => Ok(AttributeValue::Bits(parse_bits(pair)?)),
        _ => unreachable!(),
    }
}

fn parse_hier_name(int: &mut impl Interner, pair: Pair) -> Result<HierName, Box<Error>> {
    assert_eq!(pair.as_rule(), Rule::hier_name);
    let mut res = HierName { chunks: vec![] };
    for pair in pair.into_inner() {
        match pair.as_rule() {
            Rule::string => {
                res.chunks
                    .push(HierNameChunk::String(parse_string(int, pair)?));
            }
            Rule::int => {
                res.chunks.push(HierNameChunk::Index(parse_int(pair)?));
            }
            _ => unreachable!(),
        }
    }
    Ok(res)
}

fn parse_port_binding(int: &mut impl Interner, pair: Pair) -> Result<PortBinding, Box<Error>> {
    match pair.as_rule() {
        Rule::hier_name => Ok(PortBinding::Name(parse_hier_name(int, pair)?)),
        Rule::string => {
            let val = parse_string(int, pair)?;
            Ok(PortBinding::Name(HierName {
                chunks: vec![HierNameChunk::String(val)],
            }))
        }
        Rule::uint => Ok(PortBinding::Position(parse_uint(pair)?)),
        _ => unreachable!(),
    }
}

fn parse_cell_annotation(cell: &mut CellRefMut, pair: Pair) -> Result<(), Box<Error>> {
    assert_eq!(pair.as_rule(), Rule::cell_annotation);
    let span = pair.as_span();
    let mut pairs = pair.into_inner();
    let kw = pairs.next().unwrap();
    match kw.as_rule() {
        Rule::kw_keep => {
            if cell.keep() {
                return Err(error(span, "keep specified twice"));
            }
            cell.set_keep(true);
        }
        Rule::kw_no_merge => {
            if cell.no_merge() {
                return Err(error(span, "no_merge specified twice"));
            }
            cell.set_no_merge(true);
        }
        Rule::kw_no_flatten => {
            if cell.no_flatten() {
                return Err(error(span, "no_flatten specified twice"));
            }
            cell.set_no_flatten(true);
        }
        Rule::kw_async => {
            if cell.async_() {
                return Err(error(span, "async specified twice"));
            }
            cell.set_async(true);
        }
        Rule::kw_lax_x => {
            if cell.lax_x() {
                return Err(error(span, "lax_x specified twice"));
            }
            cell.set_lax_x(true);
        }
        Rule::kw_param => {
            if cell.flags_plane() != CellPlane::Main {
                return Err(error(span, "plane specified twice"));
            }
            cell.set_flags_plane(CellPlane::Param);
        }
        Rule::kw_debug => {
            if cell.flags_plane() != CellPlane::Main {
                return Err(error(span, "plane specified twice"));
            }
            cell.set_flags_plane(CellPlane::Debug);
        }
        Rule::kw_name => {
            let hn = parse_hier_name(cell, pairs.next().unwrap())?;
            cell.add_annotation(CellAnnotation::Name(hn));
        }
        Rule::kw_position => {
            cell.add_annotation(CellAnnotation::Position(parse_uint(pairs.next().unwrap())?));
        }
        Rule::kw_attr => {
            let key = parse_string(cell, pairs.next().unwrap())?;
            let val = parse_attr_val(cell, pairs.next().unwrap())?;
            cell.add_annotation(CellAnnotation::Attribute(Attribute { key, val }));
        }
        Rule::kw_downto => {
            cell.add_annotation(CellAnnotation::BitIndexing(
                BitIndexingKind::Downto,
                parse_int(pairs.next().unwrap())?,
            ));
        }
        Rule::kw_upto => {
            cell.add_annotation(CellAnnotation::BitIndexing(
                BitIndexingKind::Upto,
                parse_int(pairs.next().unwrap())?,
            ));
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn parse_mod_annotation(mut module: ModuleRefMut, pair: Pair) -> Result<(), Box<Error>> {
    let span = pair.as_span();
    let mut pairs = pair.into_inner();
    let kw = pairs.next().unwrap();
    match kw.as_rule() {
        Rule::kw_keep => {
            if module.keep() {
                return Err(error(span, "keep specified twice"));
            }
            module.set_keep(true);
        }
        Rule::kw_no_merge => {
            if module.no_merge() {
                return Err(error(span, "no_merge specified twice"));
            }
            module.set_no_merge(true);
        }
        Rule::kw_no_flatten => {
            if module.no_flatten() {
                return Err(error(span, "no_flatten specified twice"));
            }
            module.set_no_flatten(true);
        }
        Rule::kw_inline => {
            if module.inline() {
                return Err(error(span, "inline specified twice"));
            }
            module.set_inline(true);
        }
        Rule::kw_blackbox => {
            if module.blackbox() {
                return Err(error(span, "blackbox specified twice"));
            }
            module.set_blackbox(true);
        }
        Rule::kw_top => {
            if module.top() {
                return Err(error(span, "no_merge specified twice"));
            }
            module.set_top(true);
        }
        Rule::kw_name => {
            let hn = parse_hier_name(&mut module, pairs.next().unwrap())?;
            module.add_annotation(ModuleAnnotation::Name(hn));
        }
        Rule::kw_attr => {
            let key = parse_string(&mut module, pairs.next().unwrap())?;
            let val = parse_attr_val(&mut module, pairs.next().unwrap())?;
            module.add_annotation(ModuleAnnotation::Attribute(Attribute { key, val }));
        }
        _ => unreachable!(),
    }
    Ok(())
}

struct ModuleParser<'a, 's> {
    module_names: &'a HashMap<&'s str, ModuleId>,
    cell_names: HashMap<&'s str, CellId>,
    cell_spans: EntityVec<CellId, Span<'s>>,
    consts_bits: HashMap<Bits, CellId>,
    consts_int: HashMap<i32, CellId>,
    consts_float: HashMap<F64BitEq, CellId>,
    consts_str: HashMap<StrId, CellId>,
    swizzles: Vec<(CellId, Vec<Pair<'s>>)>,
    busswizzles: Vec<(CellId, Vec<Pair<'s>>)>,
    wire_optimized_out_fixups: Vec<(CellId, CellId)>,
}

impl<'s> ModuleParser<'_, 's> {
    fn get_bits_const(&mut self, mut module: ModuleRefMut, val: Bits) -> CellId {
        match self.consts_bits.entry(val) {
            hash_map::Entry::Occupied(e) => *e.get(),
            hash_map::Entry::Vacant(e) => {
                let res = module.add_cell(e.key().clone());
                e.insert(res);
                res
            }
        }
    }

    fn get_bit_const(&mut self, module: ModuleRefMut, val: Bit) -> CellId {
        let val = Bits {
            bits: [val].into_iter().collect(),
        };
        self.get_bits_const(module, val)
    }

    fn parse_val(&mut self, mut module: ModuleRefMut, pair: Pair) -> Result<CellId, Box<Error>> {
        assert_eq!(pair.as_rule(), Rule::val);
        let mut pairs = pair.into_inner();
        let pair = pairs.next().unwrap();
        match pair.as_rule() {
            Rule::local_id => {
                let lid = parse_local_id(&pair);
                if let Some(&cid) = self.cell_names.get(&lid) {
                    Ok(cid)
                } else {
                    Err(error(pair.as_span(), "undefined cell"))
                }
            }
            Rule::bits => {
                let bits = parse_bits(pair)?;
                Ok(self.get_bits_const(module, bits))
            }
            Rule::int => {
                let val = parse_int(pair)?;
                match self.consts_int.entry(val) {
                    hash_map::Entry::Occupied(e) => Ok(*e.get()),
                    hash_map::Entry::Vacant(e) => {
                        let res = module.add_cell(val);
                        e.insert(res);
                        Ok(res)
                    }
                }
            }
            Rule::float => {
                let val = parse_float(pair)?;
                match self.consts_float.entry(val) {
                    hash_map::Entry::Occupied(e) => Ok(*e.get()),
                    hash_map::Entry::Vacant(e) => {
                        let res = module.add_cell(val);
                        e.insert(res);
                        Ok(res)
                    }
                }
            }
            Rule::string => {
                let val = parse_string(&mut module.reborrow(), pair)?;
                match self.consts_str.entry(val) {
                    hash_map::Entry::Occupied(e) => Ok(*e.get()),
                    hash_map::Entry::Vacant(e) => {
                        let res = module.add_cell(val);
                        e.insert(res);
                        Ok(res)
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn parse_cell(&mut self, mut cell: CellRefMut, mut pairs: Pairs<'s>) -> Result<(), Box<Error>> {
        let kw = pairs.next().unwrap();
        match kw.as_rule() {
            Rule::kw_void => (),
            Rule::kw_param => {
                let idx = parse_uint(pairs.next().unwrap())?;
                let id = ParamId::from_idx(idx as usize);
                let pair = pairs.next().unwrap();
                let typ = match pair.as_rule() {
                    Rule::width => ParamType::BitVec(parse_width(pair)?),
                    Rule::kw_bitvec => ParamType::BitVecAny,
                    Rule::kw_int => ParamType::Int,
                    Rule::kw_float => ParamType::Float,
                    Rule::kw_string => ParamType::String,
                    _ => unreachable!(),
                };
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(Param { id, typ });
            }
            Rule::kw_input => {
                let idx = parse_uint(pairs.next().unwrap())?;
                let id = PortInId::from_idx(idx as usize);
                let mut width = None;
                if let Some(pair) = pairs.peek() {
                    if pair.as_rule() == Rule::width {
                        pairs.next();
                        width = Some(parse_width(pair)?);
                    }
                }
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(PortIn { id, width });
            }
            Rule::kw_output => {
                let idx = parse_uint(pairs.next().unwrap())?;
                let id = PortOutId::from_idx(idx as usize);
                let mut width = None;
                if let Some(pair) = pairs.peek() {
                    if pair.as_rule() == Rule::width {
                        pairs.next();
                        width = Some(parse_width(pair)?);
                    }
                }
                let mut val = None;
                if let Some(pair) = pairs.peek() {
                    if pair.as_rule() == Rule::val {
                        pairs.next();
                        val = Some(self.parse_val(cell.module_mut(), pair)?);
                    }
                }
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(PortOut { id, width, val });
            }
            Rule::kw_busport => {
                let idx = parse_uint(pairs.next().unwrap())?;
                let id = PortBusId::from_idx(idx as usize);
                let mut width = None;
                if let Some(pair) = pairs.peek() {
                    if pair.as_rule() == Rule::width {
                        pairs.next();
                        width = Some(parse_width(pair)?);
                    }
                }
                let kind = parse_bus_kind(&mut pairs);
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(PortBus { id, width, kind });
            }
            Rule::kw_const => {
                let cv = pairs.next().unwrap();
                match cv.as_rule() {
                    Rule::bits => {
                        cell.set_contents(parse_bits(cv)?);
                    }
                    Rule::int => {
                        cell.set_contents(parse_int(cv)?);
                    }
                    Rule::float => {
                        cell.set_contents(parse_float(cv)?);
                    }
                    Rule::string => {
                        let val = parse_string(&mut cell, cv)?;
                        cell.set_contents(val);
                    }
                    _ => unreachable!(),
                }
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
            }
            Rule::kw_swizzle => {
                let width = parse_width(pairs.next().unwrap())?;
                let mut chunks = vec![];
                for pair in pairs {
                    if pair.as_rule() == Rule::swizzle_chunk {
                        chunks.push(pair);
                    } else {
                        parse_cell_annotation(&mut cell, pair)?;
                    }
                }
                self.swizzles.push((cell.id(), chunks));
                cell.set_contents(Swizzle {
                    width,
                    chunks: vec![],
                });
            }
            Rule::kw_busswizzle => {
                let width = parse_width(pairs.next().unwrap())?;
                let mut chunks = vec![];
                for pair in pairs {
                    if pair.as_rule() == Rule::busswizzle_chunk {
                        chunks.push(pair);
                    } else {
                        parse_cell_annotation(&mut cell, pair)?;
                    }
                }
                self.busswizzles.push((cell.id(), chunks));
                cell.set_contents(BusSwizzle {
                    width,
                    chunks: vec![],
                });
            }
            Rule::kw_slice => {
                let width = parse_width(pairs.next().unwrap())?;
                let val = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let pos = parse_uint(pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(Slice { width, val, pos });
            }
            Rule::kw_extop => {
                let kind = match kw.as_str() {
                    "zext" => ExtKind::Zext,
                    "sext" => ExtKind::Sext,
                    _ => unreachable!(),
                };
                let width = parse_width(pairs.next().unwrap())?;
                let val = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(Ext { kind, width, val });
            }
            Rule::kw_bufop => {
                let inv = match kw.as_str() {
                    "buf" => false,
                    "inv" => true,
                    _ => unreachable!(),
                };
                let width = parse_width(pairs.next().unwrap())?;
                let val = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(Buf { inv, width, val });
            }
            Rule::kw_bitop => {
                let kind = match kw.as_str() {
                    "and" => BitOpKind::And,
                    "or" => BitOpKind::Or,
                    "andnot" => BitOpKind::AndNot,
                    "ornot" => BitOpKind::OrNot,
                    "nand" => BitOpKind::Nand,
                    "nor" => BitOpKind::Nor,
                    "xor" => BitOpKind::Xor,
                    "xnor" => BitOpKind::Xnor,
                    _ => unreachable!(),
                };
                let width = parse_width(pairs.next().unwrap())?;
                let val_a = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let val_b = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(BitOp {
                    kind,
                    width,
                    val_a,
                    val_b,
                });
            }
            Rule::kw_uxorop => {
                let inv = match kw.as_str() {
                    "uxor" => false,
                    "uxnor" => true,
                    _ => unreachable!(),
                };
                let val = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(UnaryXor { inv, val });
            }
            Rule::kw_muxop => {
                let kind = match kw.as_str() {
                    "mux" => MuxKind::Binary,
                    "parmux" => MuxKind::Parallel,
                    "priomux" => MuxKind::Priority,
                    _ => unreachable!(),
                };
                let width = parse_width(pairs.next().unwrap())?;
                let val_sel = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let mut vals = SmallVec::new();
                for pair in pairs {
                    match pair.as_rule() {
                        Rule::cell_annotation => parse_cell_annotation(&mut cell, pair)?,
                        Rule::val => {
                            vals.push(self.parse_val(cell.module_mut(), pair)?);
                        }
                        _ => unreachable!(),
                    }
                }
                cell.set_contents(Mux {
                    kind,
                    width,
                    val_sel,
                    vals,
                });
            }
            Rule::kw_switchop => {
                let kind = match kw.as_str() {
                    "switch" => SwitchKind::Priority,
                    "parswitch" => SwitchKind::Parallel,
                    _ => unreachable!(),
                };
                let width = parse_width(pairs.next().unwrap())?;
                let val_sel = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let mut cases = vec![];
                let default;
                loop {
                    let pair = pairs.next().unwrap();
                    match pair.as_rule() {
                        Rule::switch_case => {
                            let mut ipairs = pair.into_inner();
                            let sel = parse_bits(ipairs.next().unwrap())?;
                            let val = self.parse_val(cell.module_mut(), ipairs.next().unwrap())?;
                            cases.push(SwitchCase { sel, val });
                        }
                        Rule::val => {
                            default = self.parse_val(cell.module_mut(), pair)?;
                            break;
                        }
                        _ => unreachable!(),
                    }
                }
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(Switch {
                    kind,
                    width,
                    val_sel,
                    cases,
                    default,
                });
            }
            Rule::kw_cmpop => {
                let (kind, inv, swap) = match kw.as_str() {
                    "eq" => (CmpKind::Eq, false, false),
                    "ne" => (CmpKind::Eq, true, false),
                    "ult" => (CmpKind::Ult, false, false),
                    "ugt" => (CmpKind::Ult, false, true),
                    "ule" => (CmpKind::Ult, true, true),
                    "uge" => (CmpKind::Ult, true, false),
                    "slt" => (CmpKind::Slt, false, false),
                    "sgt" => (CmpKind::Slt, false, true),
                    "sle" => (CmpKind::Slt, true, true),
                    "sge" => (CmpKind::Slt, true, false),
                    _ => unreachable!(),
                };
                let mut val_a = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let mut val_b = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                if swap {
                    core::mem::swap(&mut val_a, &mut val_b);
                }
                cell.set_contents(Cmp {
                    kind,
                    inv,
                    val_a,
                    val_b,
                });
            }
            Rule::kw_addsub => {
                let width = parse_width(pairs.next().unwrap())?;
                let val_a = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let val_b = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let val_inv = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let val_carry = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(AddSub {
                    width,
                    val_a,
                    val_b,
                    val_inv,
                    val_carry,
                });
            }
            Rule::kw_addop => {
                let val_c = match kw.as_str() {
                    "add" => Bit::_0,
                    "sub" => Bit::_1,
                    _ => unreachable!(),
                };
                let width = parse_width(pairs.next().unwrap())?;
                let val_a = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let val_b = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                let val_c = self.get_bit_const(cell.module_mut(), val_c);
                cell.set_contents(AddSub {
                    width,
                    val_a,
                    val_b,
                    val_inv: val_c,
                    val_carry: val_c,
                });
            }
            Rule::kw_mul => {
                let width = parse_width(pairs.next().unwrap())?;
                let val_a = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let val_b = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(Mul {
                    width,
                    val_a,
                    val_b,
                });
            }
            Rule::kw_shiftop => {
                let factor = match kw.as_str() {
                    "shr" => 1,
                    "shl" => -1,
                    _ => unreachable!(),
                };
                let mut kind = ShiftKind::Unsigned;
                match pairs.peek().unwrap().as_rule() {
                    Rule::kw_unsigned => {
                        pairs.next();
                    }
                    Rule::kw_signed => {
                        pairs.next();
                        kind = ShiftKind::Signed;
                    }
                    Rule::kw_fill_x => {
                        pairs.next();
                        kind = ShiftKind::FillX;
                    }
                    _ => (),
                }
                let width = parse_width(pairs.next().unwrap())?;
                let val = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let mut shamt_signed = false;
                match pairs.peek().unwrap().as_rule() {
                    Rule::kw_unsigned => {
                        pairs.next();
                    }
                    Rule::kw_signed => {
                        pairs.next();
                        shamt_signed = true;
                    }
                    _ => (),
                }
                let val_shamt = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let mut shamt_scale = factor;
                let mut shamt_bias = 0;
                if let Some(pair) = pairs.peek() {
                    if pair.as_rule() == Rule::kw_scale {
                        pairs.next();
                        shamt_scale *= parse_int(pairs.next().unwrap())?;
                    }
                }
                if let Some(pair) = pairs.peek() {
                    if pair.as_rule() == Rule::kw_bias {
                        pairs.next();
                        shamt_bias = parse_int(pairs.next().unwrap())? * factor;
                    }
                }
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(Shift {
                    kind,
                    width,
                    val,
                    val_shamt,
                    shamt_signed,
                    shamt_scale,
                    shamt_bias,
                });
            }
            Rule::kw_register => {
                let width = parse_width(pairs.next().unwrap())?;
                let mut init = None;
                let mut async_trigs = vec![];
                let mut clock_trig = None;
                for pair in pairs {
                    match pair.as_rule() {
                        Rule::cell_annotation => parse_cell_annotation(&mut cell, pair)?,
                        Rule::reg_item => {
                            let ispan = pair.as_span();
                            let mut ipairs = pair.into_inner();
                            let ikw = ipairs.next().unwrap();
                            match ikw.as_rule() {
                                Rule::kw_init => {
                                    if init.is_some() {
                                        return Err(error(ispan, "init value already defined"));
                                    }
                                    init = Some(
                                        self.parse_val(cell.module_mut(), ipairs.next().unwrap())?,
                                    );
                                }
                                Rule::kw_async => {
                                    let cond_inv = parse_inv(&mut ipairs);
                                    let cond =
                                        self.parse_val(cell.module_mut(), ipairs.next().unwrap())?;
                                    let data = ipairs.next().unwrap();
                                    let data = match data.as_rule() {
                                        Rule::val => self.parse_val(cell.module_mut(), data)?,
                                        Rule::kw_noop => cell.id(),
                                        _ => unreachable!(),
                                    };
                                    async_trigs.push(RegisterRule {
                                        cond,
                                        cond_inv,
                                        data,
                                    });
                                }
                                Rule::kw_sync => {
                                    let edge = match ipairs.next().unwrap().as_str() {
                                        "posedge" => ClockEdge::Posedge,
                                        "negedge" => ClockEdge::Negedge,
                                        "dualedge" => ClockEdge::Dualedge,
                                        _ => unreachable!(),
                                    };
                                    let clk =
                                        self.parse_val(cell.module_mut(), ipairs.next().unwrap())?;
                                    let mut rules = vec![];
                                    for ipair in ipairs {
                                        assert_eq!(ipair.as_rule(), Rule::sync_item);
                                        let mut iipairs = ipair.into_inner();
                                        let iikw = iipairs.next().unwrap();
                                        match iikw.as_rule() {
                                            Rule::kw_default => {
                                                let data = self.parse_val(
                                                    cell.module_mut(),
                                                    iipairs.next().unwrap(),
                                                )?;
                                                rules.push(RegisterRule {
                                                    cond: self
                                                        .get_bit_const(cell.module_mut(), Bit::_1),
                                                    cond_inv: false,
                                                    data,
                                                });
                                            }
                                            Rule::kw_cond => {
                                                let cond_inv = parse_inv(&mut iipairs);
                                                let cond = self.parse_val(
                                                    cell.module_mut(),
                                                    iipairs.next().unwrap(),
                                                )?;
                                                let data = iipairs.next().unwrap();
                                                let data = match data.as_rule() {
                                                    Rule::val => {
                                                        self.parse_val(cell.module_mut(), data)?
                                                    }
                                                    Rule::kw_noop => cell.id(),
                                                    _ => unreachable!(),
                                                };
                                                rules.push(RegisterRule {
                                                    cond,
                                                    cond_inv,
                                                    data,
                                                });
                                            }
                                            _ => unreachable!(),
                                        }
                                    }
                                    clock_trig = Some(ClockTrigger { clk, edge, rules });
                                }
                                _ => unreachable!(),
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                let init = init.unwrap_or_else(|| {
                    self.get_bits_const(
                        cell.module_mut(),
                        Bits {
                            bits: smallvec![Bit::X; width as usize],
                        },
                    )
                });
                cell.set_contents(Register {
                    width,
                    init,
                    async_trigs,
                    clock_trig,
                });
            }
            Rule::kw_instance => {
                let pgid = pairs.next().unwrap();
                let gid = parse_global_id(&pgid);
                let Some(&imod) = self.module_names.get(&gid) else {
                    return Err(error(pgid.as_span(), "undefined module"));
                };
                let mut params = EntityVec::new();
                let mut ports_in = EntityVec::new();
                let mut ports_out = EntityVec::new();
                let mut ports_bus = EntityVec::new();
                let ppairs = pairs.next().unwrap();
                assert_eq!(ppairs.as_rule(), Rule::params);
                for ppair in ppairs.into_inner() {
                    params.push(self.parse_val(cell.module_mut(), ppair)?);
                }
                enum PortKind {
                    Input,
                    Output,
                    Bus,
                }
                let mut kind = PortKind::Input;
                for pair in pairs {
                    match pair.as_rule() {
                        Rule::cell_annotation => parse_cell_annotation(&mut cell, pair)?,
                        Rule::val => match kind {
                            PortKind::Input => {
                                ports_in.push(self.parse_val(cell.module_mut(), pair)?);
                            }
                            PortKind::Output => {
                                ports_out.push(self.parse_val(cell.module_mut(), pair)?);
                            }
                            PortKind::Bus => {
                                ports_bus.push(self.parse_val(cell.module_mut(), pair)?);
                            }
                        },
                        Rule::kw_output => kind = PortKind::Output,
                        Rule::kw_bus => kind = PortKind::Bus,
                        _ => unreachable!(),
                    }
                }
                cell.set_contents(Instance {
                    module: imod,
                    params,
                    ports_in,
                    ports_out,
                    ports_bus,
                });
            }
            Rule::kw_uinstance => {
                let name = parse_hier_name(&mut cell, pairs.next().unwrap())?;
                let mut params = vec![];
                let mut ports_in = vec![];
                let mut ports_out = EntityVec::new();
                let mut ports_bus = vec![];
                for pair in pairs {
                    match pair.as_rule() {
                        Rule::cell_annotation => parse_cell_annotation(&mut cell, pair)?,
                        Rule::ui_item => {
                            let ispan = pair.as_span();
                            let mut ipairs = pair.into_inner();
                            let ikw = ipairs.next().unwrap();
                            match ikw.as_rule() {
                                Rule::kw_param => {
                                    let pname =
                                        parse_port_binding(&mut cell, ipairs.next().unwrap())?;
                                    let val =
                                        self.parse_val(cell.module_mut(), ipairs.next().unwrap())?;
                                    params.push((pname, val));
                                }
                                Rule::kw_input => {
                                    let pname =
                                        parse_port_binding(&mut cell, ipairs.next().unwrap())?;
                                    let val =
                                        self.parse_val(cell.module_mut(), ipairs.next().unwrap())?;
                                    ports_in.push((pname, val));
                                }
                                Rule::kw_output => {
                                    let id = parse_uint(ipairs.next().unwrap())?;
                                    let id = PortOutId::from_idx(id as usize);
                                    let pname =
                                        parse_port_binding(&mut cell, ipairs.next().unwrap())?;
                                    let val =
                                        self.parse_val(cell.module_mut(), ipairs.next().unwrap())?;
                                    if id != ports_out.next_id() {
                                        return Err(error(ispan, "wrong output id in sequence"));
                                    }
                                    ports_out.push((pname, val));
                                }
                                Rule::kw_bus => {
                                    let pname =
                                        parse_port_binding(&mut cell, ipairs.next().unwrap())?;
                                    let val =
                                        self.parse_val(cell.module_mut(), ipairs.next().unwrap())?;
                                    ports_bus.push((pname, val));
                                }
                                _ => unreachable!(),
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                cell.set_contents(UnresolvedInstance {
                    name,
                    params,
                    ports_in,
                    ports_out,
                    ports_bus,
                });
            }
            Rule::kw_instout => {
                let width = parse_width(pairs.next().unwrap())?;
                let inst = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let out = parse_uint(pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(InstanceOutput {
                    width,
                    inst,
                    out: PortOutId::from_idx(out as usize),
                });
            }
            Rule::kw_bus => {
                let width = parse_width(pairs.next().unwrap())?;
                let kind = parse_bus_kind(&mut pairs);
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(Bus { width, kind });
            }
            Rule::kw_busjoiner => {
                let bus_a = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let bus_b = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(BusJoiner { bus_a, bus_b });
            }
            Rule::kw_busdriver => {
                let bus = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let cond_inv = parse_inv(&mut pairs);
                let cond = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let val = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(BusDriver {
                    bus,
                    cond,
                    cond_inv,
                    val,
                });
            }
            Rule::kw_blackbox_buf => {
                let width = parse_width(pairs.next().unwrap())?;
                let val = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                for pair in pairs {
                    parse_cell_annotation(&mut cell, pair)?;
                }
                cell.set_contents(BlackboxBuf { width, val });
            }
            Rule::kw_wire => {
                let val = self.parse_val(cell.module_mut(), pairs.next().unwrap())?;
                let mut optimized_out = None;
                while let Some(pair) = pairs.next() {
                    match pair.as_rule() {
                        Rule::cell_annotation => parse_cell_annotation(&mut cell, pair)?,
                        Rule::kw_optimized_out => {
                            optimized_out = Some(parse_bits(pairs.next().unwrap())?);
                        }
                        _ => unreachable!(),
                    }
                }
                if optimized_out.is_none() {
                    self.wire_optimized_out_fixups.push((cell.id(), val));
                }
                cell.set_contents(Wire {
                    val,
                    optimized_out: optimized_out.unwrap_or_else(|| Bits {
                        bits: Default::default(),
                    }),
                });
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn parse_swizzle(
        &self,
        module: ModuleRef,
        pairs: Vec<Pair<'s>>,
    ) -> Result<Vec<SwizzleChunk>, Box<Error>> {
        let mut res = vec![];
        for pair in pairs {
            assert_eq!(pair.as_rule(), Rule::swizzle_chunk);
            let span = pair.as_span();
            let mut cpairs = pair.into_inner();
            let cpair = cpairs.next().unwrap();
            match cpair.as_rule() {
                Rule::local_id => {
                    let lid = parse_local_id(&cpair);
                    let Some(&val) = self.cell_names.get(&lid) else {
                        return Err(error(cpair.as_span(), "undefined cell"));
                    };
                    let mut sl = None;
                    if let Some(cpair) = cpairs.peek() {
                        if cpair.as_rule() == Rule::uint {
                            cpairs.next();
                            let start = parse_uint(cpair)?;
                            let mut len = 1;
                            if let Some(cpair) = cpairs.peek() {
                                if cpair.as_rule() == Rule::uint {
                                    cpairs.next();
                                    let end = parse_uint(cpair)?;
                                    if end < start {
                                        return Err(error(span, "swizzle end smaller than start"));
                                    }
                                    len = end - start;
                                }
                            }
                            sl = Some((start, len));
                        }
                    }
                    let (val_start, val_len) = if let Some(x) = sl {
                        x
                    } else {
                        let CellType::BitVec(width, _) = module.cell(val).typ() else {
                            return Err(error(span, "swizzle source not a bitvec"));
                        };
                        (0, width)
                    };
                    let sext_len = if let Some(cpair) = cpairs.next() {
                        assert_eq!(cpair.as_rule(), Rule::kw_sext);
                        parse_uint(cpairs.next().unwrap())?
                    } else {
                        val_len
                    };
                    if sext_len < val_len {
                        return Err(error(span, "sign extension length shorter than value"));
                    }
                    res.push(SwizzleChunk::Value {
                        val,
                        val_start,
                        val_len,
                        sext_len,
                    });
                }
                Rule::bits => {
                    res.push(SwizzleChunk::Const(parse_bits(cpair)?));
                }
                _ => unreachable!(),
            }
        }
        Ok(res)
    }

    fn parse_busswizzle(
        &self,
        module: ModuleRef,
        pairs: Vec<Pair<'s>>,
    ) -> Result<Vec<BusSwizzleChunk>, Box<Error>> {
        let mut res = vec![];
        for pair in pairs {
            assert_eq!(pair.as_rule(), Rule::busswizzle_chunk);
            let span = pair.as_span();
            let mut cpairs = pair.into_inner();
            let cpair = cpairs.next().unwrap();
            assert_eq!(cpair.as_rule(), Rule::local_id);
            let lid = parse_local_id(&cpair);
            let Some(&val) = self.cell_names.get(&lid) else {
                return Err(error(cpair.as_span(), "undefined cell"));
            };
            let mut sl = None;
            if let Some(cpair) = cpairs.peek() {
                if cpair.as_rule() == Rule::uint {
                    cpairs.next();
                    let start = parse_uint(cpair)?;
                    let mut len = 1;
                    if let Some(cpair) = cpairs.peek() {
                        if cpair.as_rule() == Rule::uint {
                            cpairs.next();
                            let end = parse_uint(cpair)?;
                            if end < start {
                                return Err(error(span, "busswizzle end smaller than start"));
                            }
                            len = end - start;
                        }
                    }
                    sl = Some((start, len));
                }
            }
            let (val_start, val_len) = if let Some(x) = sl {
                x
            } else {
                let CellType::BitVec(width, true) = module.cell(val).typ() else {
                    return Err(error(span, "busswizzle source not a bus"));
                };
                (0, width)
            };
            res.push(BusSwizzleChunk {
                val,
                val_start,
                val_len,
            });
        }
        Ok(res)
    }
}

fn parse_module(
    span: Span,
    pairs: Pairs,
    module_names: &HashMap<&str, ModuleId>,
    mut module: ModuleRefMut,
) -> Result<(), Box<Error>> {
    let mut mp = ModuleParser {
        module_names,
        cell_names: HashMap::new(),
        cell_spans: EntityVec::new(),
        consts_bits: HashMap::new(),
        consts_int: HashMap::new(),
        consts_float: HashMap::new(),
        consts_str: HashMap::new(),
        swizzles: vec![],
        busswizzles: vec![],
        wire_optimized_out_fixups: vec![],
    };
    let mut cell_contents = vec![];
    for pair in pairs {
        match pair.as_rule() {
            Rule::module_annotation => {
                parse_mod_annotation(module.reborrow(), pair)?;
            }
            Rule::cell => {
                mp.cell_spans.push(pair.as_span());
                let mut cpairs = pair.into_inner();
                let cid = module.add_void().id();
                let plid = cpairs.peek().unwrap();
                if plid.as_rule() == Rule::local_id {
                    let lid = parse_local_id(&plid);
                    if mp.cell_names.insert(lid, cid).is_some() {
                        return Err(error(plid.as_span(), "cell redefined"));
                    }
                    cpairs.next();
                }
                cell_contents.push((cid, cpairs));
            }
            _ => unreachable!(),
        }
    }
    for (cid, pairs) in cell_contents {
        mp.parse_cell(module.cell_mut(cid), pairs)?;
    }
    for (cid, pairs) in core::mem::take(&mut mp.swizzles) {
        let chunks = mp.parse_swizzle(module.as_ref(), pairs)?;
        let mut cell = module.cell_mut(cid);
        let CellKind::Swizzle(sw) = cell.contents() else {
            unreachable!();
        };
        cell.set_contents(CellKind::Swizzle(Swizzle {
            width: sw.width,
            chunks,
        }));
    }
    for (cid, pairs) in core::mem::take(&mut mp.busswizzles) {
        let chunks = mp.parse_busswizzle(module.as_ref(), pairs)?;
        let mut cell = module.cell_mut(cid);
        let CellKind::BusSwizzle(sw) = cell.contents() else {
            unreachable!();
        };
        cell.set_contents(CellKind::BusSwizzle(BusSwizzle {
            width: sw.width,
            chunks,
        }));
    }
    for (cid, val) in mp.wire_optimized_out_fixups {
        let CellType::BitVec(width, _) = module.cell(val).typ() else {
            return Err(error(mp.cell_spans[cid], "wire value is not a bitvec"));
        };
        module.cell_mut(cid).set_contents(CellKind::Wire(Wire {
            optimized_out: Bits {
                bits: smallvec![Bit::_0; width as usize],
            },
            val,
        }));
    }
    let mut params = EntityPartVec::new();
    let mut ports_in = EntityPartVec::new();
    let mut ports_out = EntityPartVec::new();
    let mut ports_bus = EntityPartVec::new();
    for cell in module.cells() {
        match cell.contents() {
            CellKind::Param(p) => {
                if params.insert(p.id, cell.id()).is_some() {
                    return Err(error(mp.cell_spans[cell.id()], "param redefined"));
                }
            }
            CellKind::PortIn(p) => {
                if ports_in.insert(p.id, cell.id()).is_some() {
                    return Err(error(mp.cell_spans[cell.id()], "input port redefined"));
                }
            }
            CellKind::PortOut(p) => {
                if ports_out.insert(p.id, cell.id()).is_some() {
                    return Err(error(mp.cell_spans[cell.id()], "output port redefined"));
                }
            }
            CellKind::PortBus(p) => {
                if ports_bus.insert(p.id, cell.id()).is_some() {
                    return Err(error(mp.cell_spans[cell.id()], "bus port redefined"));
                }
            }
            _ => (),
        }
    }
    match params.try_into_full() {
        Ok(p) => module.set_params(p),
        Err(idx) => return Err(error(span, format!("param {idx} missing"))),
    }
    match ports_in.try_into_full() {
        Ok(p) => module.set_ports_in(p),
        Err(idx) => return Err(error(span, format!("input port {idx} missing"))),
    }
    match ports_out.try_into_full() {
        Ok(p) => module.set_ports_out(p),
        Err(idx) => return Err(error(span, format!("output port {idx} missing"))),
    }
    match ports_bus.try_into_full() {
        Ok(p) => module.set_ports_bus(p),
        Err(idx) => return Err(error(span, format!("bus port {idx} missing"))),
    }
    Ok(())
}

fn parse_design_annotation(design: &mut Design, pair: Pair) -> Result<(), Box<Error>> {
    let mut pairs = pair.into_inner();
    let kw = pairs.next().unwrap();
    match kw.as_rule() {
        Rule::kw_attr => {
            let key = parse_string(design, pairs.next().unwrap())?;
            let val = parse_attr_val(design, pairs.next().unwrap())?;
            design.add_annotation(DesignAnnotation::Attribute(Attribute { key, val }));
        }
        _ => unreachable!(),
    }
    Ok(())
}

impl Design {
    /// Parses a design in text format.
    pub fn parse_text(s: &str) -> Result<Design, Box<Error>> {
        let mut design = Design::new();
        let pairs = TextParser::parse(Rule::design, s)?;
        let mut module_contents = vec![];
        let mut module_names = HashMap::new();
        for pair in pairs {
            match pair.as_rule() {
                Rule::EOI => (),
                Rule::version => {
                    let span = pair.as_span();
                    let vers = parse_string_raw(pair.into_inner().next().unwrap())?;
                    // TODO: actual version checking
                    if vers != "0.1" {
                        return Err(error(span, "unknown version"));
                    }
                }
                Rule::design_annotation => {
                    parse_design_annotation(&mut design, pair)?;
                }
                Rule::module => {
                    let mspan = pair.as_span();
                    let mid = design.add_module().id();
                    let mut mpairs = pair.into_inner();
                    let pgid = mpairs.next().unwrap();
                    let gid = parse_global_id(&pgid);
                    if module_names.insert(gid, mid).is_some() {
                        return Err(error(pgid.as_span(), "module redefined"));
                    }
                    module_contents.push((mid, mspan, mpairs));
                }
                Rule::kw_void => {
                    let mid = design.add_module().id();
                    design.remove_module(mid);
                }
                _ => unreachable!(),
            }
        }
        for (mid, span, pairs) in module_contents {
            parse_module(span, pairs, &module_names, design.module_mut(mid).unwrap())?;
        }
        Ok(design)
    }
}
