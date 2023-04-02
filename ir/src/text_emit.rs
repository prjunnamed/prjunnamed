use std::io::{self, Write};

use prjunnamed_entity::{EntityBitVec, EntityId, EntityVec};

use crate::model::{
    annotations::{
        Attribute, AttributeValue, BitIndexingKind, CellAnnotation, DesignAnnotation, HierName,
        HierNameChunk, ModuleAnnotation,
    },
    bits::Bit,
    cells::{
        BitOpKind, BusKind, CellKind, ClockEdge, CmpKind, ExtKind, MuxKind, ParamType, PortBinding,
        ShiftKind, SwitchKind, SwizzleChunk,
    },
    CellId, CellPlane, CellRef, CellType, Design, ModuleRef,
};

fn write_string(f: &mut impl std::fmt::Write, s: &str) -> std::fmt::Result {
    write!(f, "\"")?;
    for c in s.chars() {
        if c.is_ascii_graphic() || c == ' ' {
            write!(f, "{c}")?;
        } else if c <= '\x7f' {
            write!(f, "\\x{c:02x}", c = c as u32)?;
        } else {
            write!(f, "\\u{{{c:04x}}}", c = c as u32)?;
        }
    }
    write!(f, "\"")?;
    Ok(())
}

struct ValPrintHelper<'a, 'b> {
    printer: &'a ValPrinter<'b>,
    val: CellId,
}

impl std::fmt::Display for ValPrintHelper<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cell = &self.printer.module.cell(self.val);
        match cell.contents() {
            CellKind::ConstBits(v) if !self.printer.raw => write!(f, "{v}",)?,
            CellKind::ConstInt(v) if !self.printer.raw => write!(f, "{v}")?,
            CellKind::ConstFloat(v) if !self.printer.raw => write!(f, "{v}")?,
            CellKind::ConstString(v) if !self.printer.raw => {
                write_string(f, self.printer.module.design().string(*v))?
            }
            _ => write!(f, "{n}", n = self.printer.cell_names[self.val])?,
        }
        Ok(())
    }
}

struct ValPrinter<'a> {
    module: ModuleRef<'a>,
    cell_names: &'a EntityVec<CellId, String>,
    raw: bool,
}

impl<'b> ValPrinter<'b> {
    fn val<'a>(&'a self, val: CellId) -> ValPrintHelper<'a, 'b> {
        ValPrintHelper { printer: self, val }
    }
}

fn print_hier_name(design: &Design, name: &HierName) -> String {
    use std::fmt::Write;
    let mut res = String::new();
    for (i, chunk) in name.chunks.iter().enumerate() {
        if i != 0 {
            write!(res, " ").unwrap();
        }
        match *chunk {
            HierNameChunk::String(s) => write_string(&mut res, design.string(s)).unwrap(),
            HierNameChunk::Index(v) => write!(res, "[{v}]").unwrap(),
        }
    }
    res
}

fn print_port_binding(design: &Design, binding: &PortBinding) -> String {
    use std::fmt::Write;
    let mut res = String::new();
    match *binding {
        PortBinding::Name(ref n) => {
            if n.chunks.len() == 1 && matches!(n.chunks[0], HierNameChunk::String(_)) {
                let HierNameChunk::String(s) = n.chunks[0] else { unreachable!() };
                write_string(&mut res, design.string(s)).unwrap();
            } else {
                write!(res, "({})", print_hier_name(design, n)).unwrap();
            }
        }
        PortBinding::Position(i) => write!(res, "{i}").unwrap(),
    }
    res
}

fn print_attr(design: &Design, attr: &Attribute, raw: bool) -> String {
    use std::fmt::Write;
    let mut res = String::new();
    write!(res, "attr(").unwrap();
    write_string(&mut res, design.string(attr.key)).unwrap();
    write!(res, " = ").unwrap();
    match attr.val {
        AttributeValue::String(s) => write_string(&mut res, design.string(s)).unwrap(),
        AttributeValue::Bits(ref v) => write!(res, "{v}").unwrap(),
        AttributeValue::Int(v) => write!(res, "{v}").unwrap(),
        AttributeValue::Float(v) => {
            if raw {
                write!(res, "{v:#}").unwrap();
            } else {
                write!(res, "{v}").unwrap();
            }
        }
    }
    write!(res, ")").unwrap();
    res
}

fn emit_cell_annotations(f: &mut impl Write, cell: CellRef, raw: bool) -> io::Result<()> {
    for (c, n) in [
        (cell.keep(), "keep"),
        (cell.no_merge(), "no_merge"),
        (cell.no_flatten(), "no_flatten"),
        (cell.async_(), "async"),
        (cell.lax_x(), "lax_x"),
        (cell.flags_plane() == CellPlane::Param, "param"),
        (cell.flags_plane() == CellPlane::Debug, "debug"),
    ] {
        if c {
            write!(f, " {n}")?;
        }
    }
    for ann in cell.annotations() {
        match ann {
            CellAnnotation::Attribute(a) => {
                write!(f, " {v}", v = print_attr(cell.design(), a, raw))?
            }
            CellAnnotation::Name(n) => {
                write!(f, " name({v})", v = print_hier_name(cell.design(), n))?
            }
            CellAnnotation::Position(n) => write!(f, " position({n})")?,
            CellAnnotation::BitIndexing(BitIndexingKind::Downto, i) => write!(f, " downto({i})")?,
            CellAnnotation::BitIndexing(BitIndexingKind::Upto, i) => write!(f, " upto({i})")?,
        }
    }
    Ok(())
}

impl Design {
    /// Dumps the design as text.
    ///
    /// If the `raw` flag is set, the output will be roundtrippable exactly, preserving
    /// all module and cell indices.  Otherwise, tombstones will be skipped, and consts
    /// will be inlined for better readability.
    pub fn emit_text(&self, f: &mut impl Write, raw: bool) -> io::Result<()> {
        writeln!(f, "version \"0.1\";")?;
        for ann in self.annotations() {
            match ann {
                DesignAnnotation::Attribute(a) => {
                    let v = print_attr(self, a, raw);
                    writeln!(f, "{v}")?;
                }
            }
        }
        let mod_names: EntityVec<_, _> = self
            .module_ids()
            .map(|mid| self.module(mid).as_ref().map(|_| format!("@{mid}")))
            .collect();
        for mid in self.module_ids() {
            let Some(module) = self.module(mid) else {
                if raw {
                    writeln!(f, "void;")?;
                }
                continue;
            };
            writeln!(f)?;
            write!(f, "module {n}", n = mod_names[mid].as_ref().unwrap())?;
            for (c, n) in [
                (module.keep(), "keep"),
                (module.no_merge(), "no_merge"),
                (module.no_flatten(), "no_flatten"),
                (module.inline(), "inline"),
                (module.blackbox(), "blackbox"),
                (module.top(), "top"),
            ] {
                if c {
                    write!(f, " {n}")?;
                }
            }
            for ann in module.annotations() {
                match ann {
                    ModuleAnnotation::Attribute(a) => {
                        write!(f, " {v}", v = print_attr(self, a, raw))?
                    }
                    ModuleAnnotation::Name(n) => {
                        write!(f, " name({v})", v = print_hier_name(self, n))?
                    }
                }
            }
            writeln!(f, " {{")?;
            let cell_names: EntityVec<_, _> =
                module.cell_ids().map(|cid| format!("%{cid}")).collect();
            let mut cell_name_used = EntityBitVec::repeat(false, cell_names.len());
            let mut use_cell = |cid, also_const| {
                if also_const || raw || !module.cell(cid).is_const() {
                    cell_name_used.set(cid, true);
                }
            };
            for cid in module.cell_ids() {
                let cell = module.cell(cid);
                cell.for_each_val(|val| {
                    use_cell(
                        val,
                        matches!(
                            cell.contents(),
                            CellKind::Swizzle(_) | CellKind::BusSwizzle(_)
                        ),
                    );
                });
            }
            let vp = ValPrinter {
                module,
                cell_names: &cell_names,
                raw,
            };
            for cid in module.cell_ids() {
                let cell = module.cell(cid);
                if !raw
                    && !cell_name_used[cid]
                    && (cell.is_const() || matches!(cell.contents(), CellKind::Void))
                {
                    continue;
                }
                write!(f, "    ")?;
                if cell_name_used[cid] {
                    write!(f, "{n} = ", n = cell_names[cid])?;
                }
                match cell.contents() {
                    CellKind::Void => {
                        write!(f, "void")?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Param(param) => {
                        write!(f, "param {id} ", id = param.id)?;
                        match param.typ {
                            ParamType::BitVec(w) => write!(f, "[{w}]")?,
                            ParamType::BitVecAny => write!(f, "bitvec")?,
                            ParamType::String => write!(f, "string")?,
                            ParamType::Int => write!(f, "int")?,
                            ParamType::Float => write!(f, "float")?,
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::PortIn(port) => {
                        write!(f, "input {id}", id = port.id)?;
                        if let Some(w) = port.width {
                            write!(f, " [{w}]")?
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::PortOut(port) => {
                        write!(f, "output {id}", id = port.id)?;
                        if let Some(w) = port.width {
                            write!(f, " [{w}]")?
                        }
                        if let Some(v) = port.val {
                            write!(f, " {v}", v = vp.val(v))?;
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::PortBus(port) => {
                        write!(f, "busport {id}", id = port.id)?;
                        if let Some(w) = port.width {
                            write!(f, " [{w}]")?
                        }
                        match port.kind {
                            BusKind::Plain => (),
                            BusKind::Pulldown => write!(f, " pulldown")?,
                            BusKind::Pullup => write!(f, " pullup")?,
                            BusKind::WireAnd => write!(f, " wireand")?,
                            BusKind::WireOr => write!(f, " wireor")?,
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::ConstBits(v) => {
                        write!(f, "const {v}")?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::ConstInt(v) => {
                        write!(f, "const {v}")?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::ConstFloat(v) => {
                        if raw {
                            write!(f, "const {v:#}")?;
                        } else {
                            write!(f, "const {v}")?;
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::ConstString(v) => {
                        let mut s = String::new();
                        write_string(&mut s, self.string(*v)).unwrap();
                        write!(f, "const {s}")?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Swizzle(swizzle) => {
                        write!(f, "swizzle [{w}]", w = swizzle.width)?;
                        for (i, chunk) in swizzle.chunks.iter().enumerate() {
                            if i == 0 {
                                write!(f, " ")?;
                            } else {
                                write!(f, ", ")?;
                            }
                            match *chunk {
                                SwizzleChunk::Const(ref val) => write!(f, "{val}")?,
                                SwizzleChunk::Value {
                                    val,
                                    val_start,
                                    val_len,
                                    sext_len,
                                } => {
                                    write!(f, "{n}", n = cell_names[val])?;
                                    let mut skip_slice = false;
                                    if let CellType::BitVec(width, _) = module.cell(val).typ() {
                                        skip_slice = width == val_len && val_start == 0 && !raw;
                                    }
                                    if !skip_slice {
                                        if val_len == 1 {
                                            write!(f, "[{val_start}]",)?;
                                        } else {
                                            write!(
                                                f,
                                                "[{val_start}..{end}]",
                                                end = val_start + val_len
                                            )?;
                                        }
                                    }
                                    if val_len != sext_len {
                                        write!(f, " sext {sext_len}")?;
                                    }
                                }
                            }
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::BusSwizzle(swizzle) => {
                        write!(f, "busswizzle [{w}]", w = swizzle.width)?;
                        for (i, chunk) in swizzle.chunks.iter().enumerate() {
                            if i == 0 {
                                write!(f, " ")?;
                            } else {
                                write!(f, ", ")?;
                            }
                            write!(f, "{n}", n = cell_names[chunk.val])?;
                            let mut skip_slice = false;
                            if let CellType::BitVec(width, _) = module.cell(chunk.val).typ() {
                                skip_slice = width == chunk.val_len && chunk.val_start == 0 && !raw;
                            }
                            if !skip_slice {
                                if chunk.val_len == 1 {
                                    write!(f, "[{start}]", start = chunk.val_start)?;
                                } else {
                                    write!(
                                        f,
                                        "[{start}..{end}]",
                                        start = chunk.val_start,
                                        end = chunk.val_start + chunk.val_len
                                    )?;
                                }
                            }
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Slice(slice) => {
                        write!(
                            f,
                            "slice [{w}] {v}, {p}",
                            w = slice.width,
                            v = vp.val(slice.val),
                            p = slice.pos,
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Ext(ext) => {
                        write!(
                            f,
                            "{k} [{w}] {v}",
                            k = match ext.kind {
                                ExtKind::Zext => "zext",
                                ExtKind::Sext => "sext",
                            },
                            w = ext.width,
                            v = vp.val(ext.val),
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Buf(buf) => {
                        write!(
                            f,
                            "{k} [{w}] {v}",
                            k = if buf.inv { "inv" } else { "buf" },
                            w = buf.width,
                            v = vp.val(buf.val),
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::BitOp(bitop) => {
                        write!(
                            f,
                            "{k} [{w}] {va}, {vb}",
                            k = match bitop.kind {
                                BitOpKind::And => "and",
                                BitOpKind::Or => "or",
                                BitOpKind::AndNot => "andnot",
                                BitOpKind::OrNot => "ornot",
                                BitOpKind::Nand => "nand",
                                BitOpKind::Nor => "nor",
                                BitOpKind::Xor => "xor",
                                BitOpKind::Xnor => "xnor",
                            },
                            w = bitop.width,
                            va = vp.val(bitop.val_a),
                            vb = vp.val(bitop.val_b),
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::UnaryXor(uxor) => {
                        write!(
                            f,
                            "{k} {v}",
                            k = if uxor.inv { "uxnor" } else { "uxor" },
                            v = vp.val(uxor.val),
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Mux(mux) => {
                        write!(
                            f,
                            "{k} [{w}] {v}, (",
                            k = match mux.kind {
                                MuxKind::Binary => "mux",
                                MuxKind::Parallel => "parmux",
                                MuxKind::Priority => "priomux",
                            },
                            w = mux.width,
                            v = vp.val(mux.val_sel),
                        )?;
                        for (i, &val) in mux.vals.iter().enumerate() {
                            if i != 0 {
                                write!(f, ", ")?;
                            }
                            write!(f, "{v}", v = vp.val(val))?;
                        }
                        write!(f, ")")?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Switch(switch) => {
                        write!(
                            f,
                            "{k} [{w}] {v}, ",
                            k = match switch.kind {
                                SwitchKind::Priority => "switch",
                                SwitchKind::Parallel => "parswitch",
                            },
                            w = switch.width,
                            v = vp.val(switch.val_sel),
                        )?;
                        for case in &switch.cases {
                            write!(f, "{b}: {v}, ", b = case.sel, v = vp.val(case.val))?;
                        }
                        write!(f, "{v}", v = vp.val(switch.default))?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Cmp(cmp) => {
                        write!(
                            f,
                            "{k} {va}, {vb}",
                            k = match (cmp.kind, cmp.inv) {
                                (CmpKind::Eq, false) => "eq",
                                (CmpKind::Eq, true) => "ne",
                                (CmpKind::Ult, false) => "ult",
                                (CmpKind::Ult, true) => "uge",
                                (CmpKind::Slt, false) => "slt",
                                (CmpKind::Slt, true) => "sge",
                            },
                            va = vp.val(cmp.val_a),
                            vb = vp.val(cmp.val_b),
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::AddSub(addsub) => {
                        let mut is_add = false;
                        let mut is_sub = false;
                        if !raw {
                            let mut is_inv_0 = false;
                            let mut is_inv_1 = false;
                            let mut is_carry_0 = false;
                            let mut is_carry_1 = false;
                            if let Some(c) = module.cell(addsub.val_inv).get_const_bits() {
                                if c.bits.len() == 1 && c.bits[0] == Bit::_0 {
                                    is_inv_0 = true;
                                }
                                if c.bits.len() == 1 && c.bits[0] == Bit::_1 {
                                    is_inv_1 = true;
                                }
                            }
                            if let Some(c) = module.cell(addsub.val_carry).get_const_bits() {
                                if c.bits.len() == 1 && c.bits[0] == Bit::_0 {
                                    is_carry_0 = true;
                                }
                                if c.bits.len() == 1 && c.bits[0] == Bit::_1 {
                                    is_carry_1 = true;
                                }
                            }
                            is_add = is_inv_0 && is_carry_0;
                            is_sub = is_inv_1 && is_carry_1;
                        }
                        if is_add || is_sub {
                            write!(
                                f,
                                "{k} [{w}] {va}, {vb}",
                                k = if is_add { "add" } else { "sub" },
                                w = addsub.width,
                                va = vp.val(addsub.val_a),
                                vb = vp.val(addsub.val_b),
                            )?;
                        } else {
                            write!(
                                f,
                                "addsub [{w}] {va}, {vb}, {vi}, {vc}",
                                w = addsub.width,
                                va = vp.val(addsub.val_a),
                                vb = vp.val(addsub.val_b),
                                vi = vp.val(addsub.val_inv),
                                vc = vp.val(addsub.val_carry),
                            )?;
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Mul(mul) => {
                        write!(
                            f,
                            "mul [{w}] {va}, {vb}",
                            w = mul.width,
                            va = vp.val(mul.val_a),
                            vb = vp.val(mul.val_b),
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Shift(shift) => {
                        let is_shl = shift.shamt_scale < 0;
                        write!(
                            f,
                            "{d}{k} [{w}] {va}, {vbs}{vb}",
                            d = if is_shl { "shl" } else { "shr" },
                            k = match shift.kind {
                                ShiftKind::Unsigned => "",
                                ShiftKind::Signed => " signed",
                                ShiftKind::FillX => " fill_x",
                            },
                            w = shift.width,
                            va = vp.val(shift.val),
                            vbs = if shift.shamt_signed { "signed " } else { "" },
                            vb = vp.val(shift.val_shamt),
                        )?;
                        if shift.shamt_scale.abs() != 1 {
                            write!(
                                f,
                                " scale {s}",
                                s = if is_shl {
                                    -shift.shamt_scale
                                } else {
                                    shift.shamt_scale
                                }
                            )?;
                        }
                        if shift.shamt_bias != 0 {
                            write!(
                                f,
                                " bias {b}",
                                b = if is_shl {
                                    -shift.shamt_bias
                                } else {
                                    shift.shamt_bias
                                }
                            )?;
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Register(reg) => {
                        write!(f, "register [{w}]", w = reg.width)?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, " {{")?;
                        let mut skip_init = false;
                        if !raw {
                            if let Some(c) = module.cell(reg.init).get_const_bits() {
                                skip_init =
                                    c.width() == reg.width && c.bits.iter().all(|&x| x == Bit::X);
                            }
                        }
                        if !skip_init {
                            writeln!(f, "        init {v};", v = vp.val(reg.init))?;
                        }
                        for rule in &reg.async_trigs {
                            if rule.data == cid {
                                writeln!(
                                    f,
                                    "        async {inv}{c}, noop;",
                                    inv = if rule.cond_inv { "inv " } else { "" },
                                    c = vp.val(rule.cond),
                                )?;
                            } else {
                                writeln!(
                                    f,
                                    "        async {inv}{c}, {v};",
                                    inv = if rule.cond_inv { "inv " } else { "" },
                                    c = vp.val(rule.cond),
                                    v = vp.val(rule.data)
                                )?;
                            }
                        }
                        if let Some(ref sync) = reg.clock_trig {
                            writeln!(
                                f,
                                "        sync {edge} {clk} {{",
                                edge = match sync.edge {
                                    ClockEdge::Posedge => "posedge",
                                    ClockEdge::Negedge => "negedge",
                                    ClockEdge::Dualedge => "dualedge",
                                },
                                clk = vp.val(sync.clk),
                            )?;
                            for (i, rule) in sync.rules.iter().enumerate() {
                                if rule.data == cid {
                                    writeln!(
                                        f,
                                        "            cond {inv}{c}, noop;",
                                        inv = if rule.cond_inv { "inv " } else { "" },
                                        c = vp.val(rule.cond),
                                    )?;
                                } else {
                                    let mut is_default = false;
                                    if !raw && !rule.cond_inv && i == sync.rules.len() - 1 {
                                        if let Some(c) = module.cell(rule.cond).get_const_bits() {
                                            if c.bits.len() == 1 && c.bits[0] == Bit::_1 {
                                                is_default = true;
                                            }
                                        }
                                    }
                                    if is_default {
                                        writeln!(
                                            f,
                                            "            default {v};",
                                            v = vp.val(rule.data)
                                        )?;
                                    } else {
                                        writeln!(
                                            f,
                                            "            cond {inv}{c}, {v};",
                                            inv = if rule.cond_inv { "inv " } else { "" },
                                            c = vp.val(rule.cond),
                                            v = vp.val(rule.data)
                                        )?;
                                    }
                                }
                            }
                            writeln!(f, "        }}")?;
                        }
                        writeln!(f, "    }}")?;
                    }
                    CellKind::Instance(inst) => {
                        write!(
                            f,
                            "instance {n}",
                            n = mod_names[inst.module].as_ref().unwrap()
                        )?;
                        if !inst.params.is_empty() {
                            write!(f, " [")?;
                            for (i, &v) in &inst.params {
                                if i.to_idx() != 0 {
                                    write!(f, ", ")?;
                                }
                                write!(f, "{v}", v = vp.val(v))?;
                            }
                            write!(f, "]")?;
                        }
                        for (i, &v) in &inst.ports_in {
                            if i.to_idx() != 0 {
                                write!(f, ", ")?;
                            } else {
                                write!(f, " ")?;
                            }
                            write!(f, "{v}", v = vp.val(v))?;
                        }
                        if !inst.ports_out.is_empty() {
                            write!(f, " output")?;
                            for (i, &v) in &inst.ports_out {
                                if i.to_idx() != 0 {
                                    write!(f, ", ")?;
                                } else {
                                    write!(f, " ")?;
                                }
                                write!(f, "{v}", v = vp.val(v))?;
                            }
                        }
                        if !inst.ports_bus.is_empty() {
                            write!(f, " bus")?;
                            for (i, &v) in &inst.ports_bus {
                                if i.to_idx() != 0 {
                                    write!(f, ", ")?;
                                } else {
                                    write!(f, " ")?;
                                }
                                write!(f, "{v}", v = vp.val(v))?;
                            }
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::UnresolvedInstance(inst) => {
                        write!(f, "uinstance {s}", s = print_hier_name(self, &inst.name))?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, " {{")?;
                        for &(ref n, v) in &inst.params {
                            writeln!(
                                f,
                                "        param {s}, {v};",
                                v = vp.val(v),
                                s = print_port_binding(self, n)
                            )?;
                        }
                        for &(ref n, v) in &inst.ports_in {
                            writeln!(
                                f,
                                "        input {s}, {v};",
                                v = vp.val(v),
                                s = print_port_binding(self, n)
                            )?;
                        }
                        for (i, &(ref n, v)) in &inst.ports_out {
                            writeln!(
                                f,
                                "        output {i}, {s}, {v};",
                                v = vp.val(v),
                                s = print_port_binding(self, n)
                            )?;
                        }
                        for &(ref n, v) in &inst.ports_bus {
                            writeln!(
                                f,
                                "        bus {s}, {v};",
                                v = vp.val(v),
                                s = print_port_binding(self, n)
                            )?;
                        }
                        writeln!(f, "    }}")?;
                    }
                    CellKind::InstanceOutput(instout) => {
                        write!(
                            f,
                            "instout [{w}] {v}, {i}",
                            w = instout.width,
                            v = vp.val(instout.inst),
                            i = instout.out
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Bus(bus) => {
                        write!(f, "bus [{w}]", w = bus.width)?;
                        match bus.kind {
                            BusKind::Plain => (),
                            BusKind::Pulldown => write!(f, " pulldown")?,
                            BusKind::Pullup => write!(f, " pullup")?,
                            BusKind::WireAnd => write!(f, " wireand")?,
                            BusKind::WireOr => write!(f, " wireor")?,
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::BusJoiner(joiner) => {
                        write!(
                            f,
                            "busjoiner {va}, {vb}",
                            va = vp.val(joiner.bus_a),
                            vb = vp.val(joiner.bus_b),
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::BusDriver(driver) => {
                        write!(
                            f,
                            "busdriver {b}, {inv}{c}, {v}",
                            b = vp.val(driver.bus),
                            inv = if driver.cond_inv { "inv " } else { "" },
                            c = vp.val(driver.cond),
                            v = vp.val(driver.val),
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::BlackboxBuf(buf) => {
                        write!(
                            f,
                            "blackbox_buf [{w}] {v}",
                            w = buf.width,
                            v = vp.val(buf.val)
                        )?;
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                    CellKind::Wire(wire) => {
                        write!(f, "wire {v}", v = vp.val(wire.val))?;
                        let mut skip = false;
                        if !raw {
                            if let CellType::BitVec(width, _) = module.cell(wire.val).typ() {
                                if width == wire.optimized_out.width()
                                    && wire.optimized_out.bits.iter().all(|&x| x == Bit::_0)
                                {
                                    skip = true;
                                }
                            }
                        }
                        if !skip {
                            write!(f, " optimized_out {v}", v = wire.optimized_out)?;
                        }
                        emit_cell_annotations(f, cell, raw)?;
                        writeln!(f, ";")?;
                    }
                }
            }
            writeln!(f, "}}")?;
        }
        Ok(())
    }
}
