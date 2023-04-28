use prjunnamed_entity::EntityBitVec;

#[cfg(doc)]
use crate::model::cells::{Instance, InstanceOutput};

use crate::model::{
    annotations::CellAnnotation,
    cells::{BitOpKind, CellKind, CmpKind, ExtKind, MuxKind, ParamType, ShiftKind, SwizzleChunk},
    CellId, CellPlane, CellRef, CellType, Design, ModuleId, ModuleRef,
};

type Error = (Option<ModuleId>, Option<CellId>, String);

struct CellValidator<'a> {
    cell: CellRef<'a>,
    errs: &'a mut Vec<Error>,
}

impl CellValidator<'_> {
    fn err(&mut self, err: impl Into<String>) {
        self.errs.push((
            Some(self.cell.module().id()),
            Some(self.cell.id()),
            err.into(),
        ));
    }

    fn get_input_width(&mut self, val: CellId, plane: CellPlane, inp: &str) -> Option<u32> {
        let scell = self.cell.sibling(val);
        if let CellType::BitVec(width, _) = scell.typ() {
            let splane = scell.plane();
            if splane > plane {
                self.err(format!(
                    "{inp} requires {plane:?} input, got {splane:?} input"
                ));
            }
            Some(width)
        } else {
            self.err(format!("{inp} does not refer to a bitvec"));
            None
        }
    }

    fn check_input(&mut self, val: CellId, width: u32, plane: CellPlane, inp: &str) {
        if let Some(awidth) = self.get_input_width(val, plane, inp) {
            if awidth != width {
                self.err(format!(
                    "{inp} should refer to {width}-bit value, refers to {awidth}-bit value"
                ));
            }
        }
    }

    fn get_bus_width(&mut self, val: CellId, inp: &str) -> Option<u32> {
        let scell = self.cell.sibling(val);
        if let CellType::BitVec(width, true) = scell.typ() {
            Some(width)
        } else {
            self.err(format!("{inp} does not refer to a bus"));
            None
        }
    }

    fn validate(&mut self) {
        match self.cell.contents() {
            CellKind::Void => (),
            CellKind::Param(param) => {
                if self.cell.module().params().get(param.id) != Some(&self.cell.id()) {
                    self.err("parameter not included in parameter list");
                }
            }
            CellKind::PortIn(port) => {
                if self.cell.module().ports_in().get(port.id) != Some(&self.cell.id()) {
                    self.err("port not included in port list");
                }
                if port.width.is_none() && !self.cell.module().blackbox() {
                    self.err("unknown-width ports are only allowed in blackbox modules");
                }
            }
            CellKind::PortOut(port) => {
                if self.cell.module().ports_out().get(port.id) != Some(&self.cell.id()) {
                    self.err("port not included in port list");
                }
                if let Some(val) = port.val {
                    if let Some(width) = port.width {
                        self.check_input(val, width, CellPlane::Main, "output value");
                    } else {
                        self.err("unknown-width output ports cannot have a value");
                    }
                } else if !self.cell.module().blackbox() {
                    self.err("output ports without a value are only allowed in blackbox modules");
                }
            }
            CellKind::PortBus(port) => {
                if self.cell.module().ports_bus().get(port.id) != Some(&self.cell.id()) {
                    self.err("port not included in port list");
                }
                if port.width.is_none() && !self.cell.module().blackbox() {
                    self.err("unknown-width ports are only allowed in blackbox modules");
                }
            }
            CellKind::ConstBits(_) => (),
            CellKind::ConstInt(_) => (),
            CellKind::ConstFloat(_) => (),
            CellKind::ConstString(_) => (),
            CellKind::Swizzle(swizzle) => {
                let mut width: u32 = 0;
                for chunk in &swizzle.chunks {
                    let cw = match *chunk {
                        SwizzleChunk::Const(ref bits) => bits.width(),
                        SwizzleChunk::Value {
                            val,
                            val_start,
                            val_len,
                            sext_len,
                        } => {
                            if let Some(width) =
                                self.get_input_width(val, self.cell.plane(), "swizzle input")
                            {
                                if let Some(min_width) = val_start.checked_add(val_len) {
                                    if width < min_width {
                                        self.err(format!("swizzle input too short: extracting bits {val_start}..{min_width} from {width}-bits input"));
                                    }
                                } else {
                                    self.err("chunk width overflow");
                                }
                            }
                            if val_len > sext_len {
                                self.err("sext length smaller than val length");
                            }
                            if val_len == 0 && sext_len != 0 {
                                self.err("sext input cannot be 0-width");
                            }
                            sext_len
                        }
                    };
                    let Some(nw) = width.checked_add(cw) else {
                        self.err("swizzle width overflow");
                        break;
                    };
                    width = nw;
                }
                if width != swizzle.width {
                    self.err("swizzle width doesn't match sum of swizzle chunk widths");
                }
            }
            CellKind::BusSwizzle(swizzle) => {
                let mut width: u32 = 0;
                for chunk in &swizzle.chunks {
                    if let Some(width) = self.get_bus_width(chunk.val, "swizzle input") {
                        if let Some(min_width) = chunk.val_start.checked_add(chunk.val_len) {
                            if width < min_width {
                                self.err(format!("swizzle input too short: extracting bits {start}..{min_width} from {width}-bits input", start = chunk.val_start));
                            }
                        } else {
                            self.err("chunk width overflow");
                        }
                    }
                    let Some(nw) = width.checked_add(chunk.val_len) else {
                        self.err("swizzle width overflow");
                        break;
                    };
                    width = nw;
                }
                if width != swizzle.width {
                    self.err("swizzle width doesn't match sum of swizzle chunk widths");
                }
            }
            CellKind::Slice(slice) => {
                if let Some(inp_width) =
                    self.get_input_width(slice.val, self.cell.plane(), "slice input")
                {
                    if let Some(min_width) = slice.pos.checked_add(slice.width) {
                        if inp_width < min_width {
                            self.err(format!("slice input too short: extracting bits {pos}..{min_width} from {inp_width}-bits input", pos = slice.pos));
                        }
                    } else {
                        self.err("slice width overflow");
                    }
                }
            }
            CellKind::Ext(ext) => {
                if let Some(inp_width) =
                    self.get_input_width(ext.val, self.cell.plane(), "ext input")
                {
                    if ext.width < inp_width {
                        self.err(format!(
                            "extension width {ew} smaller than input width {inp_width}",
                            ew = ext.width
                        ));
                    }
                    if inp_width == 0 && ext.kind == ExtKind::Sext {
                        self.err("sext input cannot be 0-width");
                    }
                }
            }
            CellKind::Buf(buf) => {
                self.check_input(buf.val, buf.width, self.cell.plane(), "buf input");
            }
            CellKind::BitOp(bitop) => {
                self.check_input(bitop.val_a, bitop.width, self.cell.plane(), "left input");
                self.check_input(bitop.val_b, bitop.width, self.cell.plane(), "right input");
                if self.cell.async_() && matches!(bitop.kind, BitOpKind::Xor | BitOpKind::Xnor) {
                    self.err("async does not apply to xor/xnor");
                }
            }
            CellKind::UnaryXor(uxor) => {
                self.get_input_width(uxor.val, self.cell.plane(), "uxor input");
            }
            CellKind::Mux(mux) => {
                for &val in &mux.vals {
                    self.check_input(val, mux.width, self.cell.plane(), "mux input");
                }
                if let Some(sel_width) =
                    self.get_input_width(mux.val_sel, self.cell.plane(), "mux sel")
                {
                    let exp_inps = match mux.kind {
                        MuxKind::Binary => 1u32.checked_shl(sel_width),
                        MuxKind::Parallel | MuxKind::Priority => sel_width.checked_add(1),
                    };
                    if let Some(exp_inps) = exp_inps {
                        if mux.vals.len() != exp_inps.try_into().unwrap() {
                            self.err(format!(
                                "mux has {inps} inputs, should have {exp_inps} inputs",
                                inps = mux.vals.len()
                            ));
                        }
                    } else {
                        self.err("sel width overflow");
                    }
                }
            }
            CellKind::Switch(switch) => {
                let sel_width =
                    self.get_input_width(switch.val_sel, self.cell.plane(), "switch sel");
                for case in &switch.cases {
                    self.check_input(case.val, switch.width, self.cell.plane(), "switch input");
                    if let Some(w) = sel_width {
                        if case.sel.width() != w {
                            self.err(format!("case selection value is {cw}-bit, but switch selection value is {w}-bit", cw = case.sel.width()));
                        }
                    }
                }
                self.check_input(
                    switch.default,
                    switch.width,
                    self.cell.plane(),
                    "switch default",
                );
            }
            CellKind::Cmp(cmp) => {
                let width_a = self.get_input_width(cmp.val_a, self.cell.plane(), "left input");
                let width_b = self.get_input_width(cmp.val_b, self.cell.plane(), "right input");
                if let (Some(wa), Some(wb)) = (width_a, width_b) {
                    if wa != wb {
                        self.err(format!(
                            "compare left input is {wa}-bit, but right input is {wb}-bit"
                        ));
                    } else if cmp.kind == CmpKind::Slt && wa == 0 {
                        self.err("signed compare on 0-bit inputs");
                    }
                }
                if self.cell.async_() && cmp.kind != CmpKind::Eq {
                    self.err("async does not apply to slt/ult");
                }
            }
            CellKind::AddSub(addsub) => {
                self.check_input(addsub.val_a, addsub.width, self.cell.plane(), "left input");
                self.check_input(addsub.val_b, addsub.width, self.cell.plane(), "right input");
                self.check_input(addsub.val_inv, 1, self.cell.plane(), "invert input");
                self.check_input(addsub.val_carry, 1, self.cell.plane(), "carry input");
            }
            CellKind::Mul(mul) => {
                self.check_input(mul.val_a, mul.width, self.cell.plane(), "left input");
                self.check_input(mul.val_b, mul.width, self.cell.plane(), "right input");
            }
            CellKind::Shift(shift) => {
                let inp_width = self.get_input_width(shift.val, self.cell.plane(), "shift input");
                let shamt_width =
                    self.get_input_width(shift.val_shamt, self.cell.plane(), "shift amount");
                if inp_width == Some(0) && shift.kind == ShiftKind::Signed {
                    self.err("signed shift input is 0-bit");
                }
                if shamt_width == Some(0) && shift.shamt_signed {
                    self.err("signed shift amount is 0-bit");
                }
            }
            CellKind::Register(reg) => {
                self.check_input(reg.init, reg.width, CellPlane::Param, "initial value");
                for trig in &reg.async_trigs {
                    self.check_input(trig.cond, 1, CellPlane::Main, "async trigger cond");
                    self.check_input(trig.data, reg.width, CellPlane::Main, "async trigger data");
                }
                if let Some(ref clk) = reg.clock_trig {
                    self.check_input(clk.clk, 1, CellPlane::Main, "clock");
                    for rule in &clk.rules {
                        self.check_input(rule.cond, 1, CellPlane::Main, "sync rule cond");
                        self.check_input(rule.data, reg.width, CellPlane::Main, "sync rule data");
                    }
                }
            }
            CellKind::Instance(inst) => {
                if let Some(module) = self.cell.design().module(inst.module) {
                    let params = module.params();
                    if params.len() != inst.params.len() {
                        self.err("parameter list length mismatch");
                    } else {
                        for ((id, &cparam), &mparam) in inst.params.iter().zip(params.values()) {
                            let cparam = self.cell.sibling(cparam);
                            if cparam.plane() != CellPlane::Param {
                                self.err(format!("parameter {id} should be a const"));
                            }
                            let ctyp = cparam.typ();
                            if let Some(mparam) = module.cell(mparam).get_param() {
                                match mparam.typ {
                                    ParamType::BitVec(width) => {
                                        if let CellType::BitVec(cw, _) = ctyp {
                                            if cw != width {
                                                self.err(format!("parameter {id} should be {width}-bit bitvec, is {cw}-bit bitvec"));
                                            }
                                        } else {
                                            self.err(format!("parameter {id} should be a bitvec"));
                                        }
                                    }
                                    ParamType::BitVecAny => {
                                        if !matches!(
                                            ctyp,
                                            CellType::BitVec(_, _) | CellType::BitVecAny
                                        ) {
                                            self.err(format!("parameter {id} should be a bitvec"));
                                        }
                                    }
                                    ParamType::String => {
                                        if !matches!(ctyp, CellType::String) {
                                            self.err(format!("parameter {id} should be a string"));
                                        }
                                    }
                                    ParamType::Int => {
                                        if !matches!(ctyp, CellType::Int) {
                                            self.err(format!("parameter {id} should be an int"));
                                        }
                                    }
                                    ParamType::Float => {
                                        if !matches!(ctyp, CellType::Float) {
                                            self.err(format!("parameter {id} should be a float"));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let ports_in = module.ports_in();
                    if ports_in.len() != inst.ports_in.len() {
                        self.err("input port list length mismatch");
                    } else {
                        for (&cport, &mport) in inst.ports_in.values().zip(ports_in.values()) {
                            if let Some(mport) = module.cell(mport).get_port_in() {
                                if let Some(width) = mport.width {
                                    self.check_input(cport, width, CellPlane::Main, "input port");
                                } else {
                                    self.get_input_width(cport, CellPlane::Main, "input port");
                                }
                            }
                        }
                    }
                    let ports_out = module.ports_out();
                    if ports_out.len() != inst.ports_out.len() {
                        self.err("output port list length mismatch");
                    } else {
                        for ((id, &cport), &mport) in inst.ports_out.iter().zip(ports_out.values())
                        {
                            if let Some(instout) = self.cell.sibling(cport).get_instout() {
                                if instout.inst != self.cell.id() || instout.out != id {
                                    self.err(format!(
                                        "output port {id} connected to wrong instout cell"
                                    ));
                                }
                                if let Some(mport) = module.cell(mport).get_port_out() {
                                    if let Some(width) = mport.width {
                                        if width != instout.width {
                                            self.err(format!("output port {id} should be {width}-bit wide, is {cw}-bit wide", cw = instout.width));
                                        }
                                    }
                                }
                            } else {
                                self.err(format!("output port {id} not connected to instout cell"));
                            }
                        }
                    }
                    let ports_bus = module.ports_bus();
                    if ports_bus.len() != inst.ports_bus.len() {
                        self.err("bus port list length mismatch");
                    } else {
                        for ((id, &cport), &mport) in inst.ports_bus.iter().zip(ports_bus.values())
                        {
                            if let Some(mport) = module.cell(mport).get_port_bus() {
                                let cw = self.get_bus_width(cport, "bus port");
                                if let Some(width) = mport.width {
                                    if let Some(cw) = cw {
                                        if cw != width {
                                            self.err(format!("bus port {id} should be {width}-bit wide, is {cw}-bit wide"));
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    self.err("invalid module referenced");
                }
            }
            CellKind::UnresolvedInstance(inst) => {
                for &(_, val) in &inst.params {
                    if self.cell.sibling(val).plane() != CellPlane::Param {
                        self.err("parameter must be a const");
                    }
                }
                for &(_, val) in &inst.ports_in {
                    self.get_input_width(val, CellPlane::Main, "input port");
                }
                for (id, &(_, val)) in &inst.ports_out {
                    if let Some(instout) = self.cell.sibling(val).get_instout() {
                        if instout.inst != self.cell.id() || instout.out != id {
                            self.err(format!("output port {id} connected to wrong instout cell"));
                        }
                    } else {
                        self.err(format!("output port {id} not connected to instout cell"));
                    }
                }
                for &(_, val) in &inst.ports_bus {
                    self.get_bus_width(val, "bus port");
                }
            }
            CellKind::InstanceOutput(instout) => {
                let inst = self.cell.sibling(instout.inst);
                if let Some(inst) = inst.get_instance() {
                    if inst.ports_out.get(instout.out) != Some(&self.cell.id()) {
                        self.err("instout not referenced by instance");
                    }
                } else if let Some(inst) = inst.get_uinstance() {
                    if inst.ports_out.get(instout.out).map(|x| x.1) != Some(self.cell.id()) {
                        self.err("instout not referenced by instance");
                    }
                } else {
                    self.err("instout must reference instance or uinstance");
                }
            }
            CellKind::Bus(_) => (),
            CellKind::BusJoiner(joiner) => {
                let width_a = self.get_bus_width(joiner.bus_a, "left bus");
                let width_b = self.get_bus_width(joiner.bus_b, "right bus");
                if let (Some(wa), Some(wb)) = (width_a, width_b) {
                    if wa != wb {
                        self.err("joined bus width mismatch");
                    }
                }
            }
            CellKind::BusDriver(driver) => {
                if let Some(width) = self.get_bus_width(driver.bus, "bus driver output") {
                    self.check_input(driver.val, width, CellPlane::Main, "bus driver input");
                }
                self.check_input(driver.cond, 1, CellPlane::Main, "bus driver cond");
            }
            CellKind::BlackboxBuf(buf) => {
                self.check_input(buf.val, buf.width, CellPlane::Main, "buf input");
            }
            CellKind::Wire(wire) => {
                self.check_input(
                    wire.val,
                    wire.optimized_out.width(),
                    CellPlane::Debug,
                    "wire",
                );
            }
        }
        if self.cell.keep()
            && !self.cell.is_comb()
            && !matches!(
                self.cell.contents(),
                CellKind::Register(_)
                    | CellKind::Instance(_)
                    | CellKind::UnresolvedInstance(_)
                    | CellKind::BlackboxBuf(_)
                    | CellKind::Wire(_)
            )
        {
            self.err("keep not allowed on this cell");
        }
        if self.cell.no_merge()
            && !self.cell.is_comb()
            && !matches!(
                self.cell.contents(),
                CellKind::Register(_)
                    | CellKind::Instance(_)
                    | CellKind::UnresolvedInstance(_)
                    | CellKind::BlackboxBuf(_)
            )
        {
            self.err("no_merge not allowed on this cell");
        }
        if self.cell.no_flatten()
            && !matches!(
                self.cell.contents(),
                CellKind::Instance(_) | CellKind::UnresolvedInstance(_)
            )
        {
            self.err("no_flatten only allowed on instances");
        }
        if self.cell.async_()
            && !matches!(
                self.cell.contents(),
                CellKind::BitOp(_) | CellKind::Mux(_) | CellKind::Cmp(_) | CellKind::Register(_),
            )
        {
            self.err("no_merge not allowed on this cell");
        }
        if self.cell.lax_x()
            && !matches!(
                self.cell.contents(),
                CellKind::Mux(_)
                    | CellKind::Switch(_)
                    | CellKind::Cmp(_)
                    | CellKind::AddSub(_)
                    | CellKind::Mul(_),
            )
        {
            self.err("no_merge not allowed on this cell");
        }
        if self.cell.flags_plane() != CellPlane::Main
            && !self.cell.is_comb()
            && !self.cell.is_swizzle()
        {
            self.err("debug and param only allowed on combinatorial cells and swizzles");
        }
        let mut got_bit_indexing = false;
        for ann in self.cell.annotations() {
            match ann {
                CellAnnotation::Name(_) => {
                    if self.cell.is_const()
                        || self.cell.is_swizzle()
                        || matches!(
                            self.cell.contents(),
                            CellKind::BusSwizzle(_)
                                | CellKind::InstanceOutput(_)
                                | CellKind::BusJoiner(_)
                        )
                    {
                        self.err("name not allowed on this cell kind")
                    }
                }
                CellAnnotation::Position(_) => {
                    if !matches!(
                        self.cell.contents(),
                        CellKind::Param(_)
                            | CellKind::PortIn(_)
                            | CellKind::PortOut(_)
                            | CellKind::PortBus(_)
                    ) {
                        self.err("position not allowed on this cell kind");
                    }
                }
                CellAnnotation::Attribute(_) => {
                    if self.cell.is_const()
                        || self.cell.is_swizzle()
                        || matches!(
                            self.cell.contents(),
                            CellKind::BusSwizzle(_)
                                | CellKind::InstanceOutput(_)
                                | CellKind::BusJoiner(_)
                        )
                    {
                        self.err("attributes not allowed on this cell kind")
                    }
                }
                CellAnnotation::BitIndexing(_, _) => {
                    if !matches!(
                        self.cell.contents(),
                        CellKind::Param(_)
                            | CellKind::PortIn(_)
                            | CellKind::PortOut(_)
                            | CellKind::PortBus(_)
                            | CellKind::Wire(_)
                    ) {
                        self.err("bit indexing not allowed on this cell kind");
                    }
                    if got_bit_indexing {
                        self.err("bit indexing can only be specified once per cell");
                    }
                    got_bit_indexing = true;
                }
            }
        }
    }
}

struct CellCycleChecker<'a> {
    module: ModuleRef<'a>,
    entered: EntityBitVec<CellId>,
    checked: EntityBitVec<CellId>,
}

impl CellCycleChecker<'_> {
    fn check(&mut self, cid: CellId, errs: &mut Vec<Error>) {
        if self.checked[cid] {
            return;
        }
        let cell = self.module.cell(cid);
        if !(cell.is_comb() || cell.is_swizzle()) || cell.flags_plane() == CellPlane::Main {
            self.checked.set(cid, true);
            return;
        }
        if self.entered[cid] {
            errs.push((
                Some(self.module.id()),
                Some(cid),
                format!(
                    "cell is part of a {}-plane combinatorial cycle",
                    if cell.flags_plane() == CellPlane::Debug {
                        "debug"
                    } else {
                        "param"
                    }
                ),
            ));
            self.checked.set(cid, true);
            return;
        }
        self.entered.set(cid, true);
        cell.for_each_val(|cid| self.check(cid, errs));
        self.checked.set(cid, true);
    }
}

impl ModuleRef<'_> {
    fn validate(self, res: &mut Vec<Error>) {
        if self.blackbox() {
            if self.inline() {
                res.push((
                    Some(self.id()),
                    None,
                    "blackbox module cannot be inline".into(),
                ));
            }
            if self.top() {
                res.push((
                    Some(self.id()),
                    None,
                    "top module cannot be a blackbox".into(),
                ));
            }
        }
        if self.inline() && self.no_flatten() {
            res.push((
                Some(self.id()),
                None,
                "inline module cannot be no_flatten".into(),
            ));
        }
        for (id, &cid) in self.params() {
            let cell = self.cell(cid);
            let Some(param) = cell.get_param() else {
                res.push((
                    Some(self.id()),
                    Some(cid),
                    format!("parameter {id} doesn't point to parameter cell")
                ));
                continue;
            };
            if param.id != id {
                res.push((
                    Some(self.id()),
                    Some(cid),
                    format!("parameter {id} points to cell with mismatched index"),
                ));
            }
        }
        for (id, &cid) in self.ports_in() {
            let cell = self.cell(cid);
            let Some(port) = cell.get_port_in() else {
                res.push((
                    Some(self.id()),
                    Some(cid),
                    format!("input port {id} doesn't point to input port cell")
                ));
                continue;
            };
            if port.id != id {
                res.push((
                    Some(self.id()),
                    Some(cid),
                    format!("input port {id} points to cell with mismatched index"),
                ));
            }
        }
        for (id, &cid) in self.ports_out() {
            let cell = self.cell(cid);
            let Some(port) = cell.get_port_out() else {
                res.push((
                    Some(self.id()),
                    Some(cid),
                    format!("output port {id} doesn't point to output port cell")
                ));
                continue;
            };
            if port.id != id {
                res.push((
                    Some(self.id()),
                    Some(cid),
                    format!("output port {id} points to cell with mismatched index"),
                ));
            }
        }
        for (id, &cid) in self.ports_bus() {
            let cell = self.cell(cid);
            let Some(port) = cell.get_port_bus() else {
                res.push((
                    Some(self.id()),
                    Some(cid),
                    format!("bus port {id} doesn't point to bus port cell")
                ));
                continue;
            };
            if port.id != id {
                res.push((
                    Some(self.id()),
                    Some(cid),
                    format!("bus port {id} points to cell with mismatched index"),
                ));
            }
        }
        for cid in self.cell_ids() {
            CellValidator {
                cell: self.cell(cid),
                errs: res,
            }
            .validate();
        }
        let mut checker = CellCycleChecker {
            module: self,
            entered: EntityBitVec::repeat(false, self.cell_ids().len()),
            checked: EntityBitVec::repeat(false, self.cell_ids().len()),
        };
        for cid in self.cell_ids() {
            checker.check(cid, res);
        }
    }
}

struct ModRecursionChecker<'a> {
    design: &'a Design,
    entered: EntityBitVec<ModuleId>,
    checked: EntityBitVec<ModuleId>,
}

impl ModRecursionChecker<'_> {
    fn check(&mut self, mid: ModuleId, errs: &mut Vec<Error>) {
        if self.checked[mid] {
            return;
        }
        if self.entered[mid] {
            errs.push((
                Some(mid),
                None,
                "module recursively instantiates itself".into(),
            ));
            self.checked.set(mid, true);
            return;
        }
        self.entered.set(mid, true);
        if let Some(module) = self.design.module(mid) {
            for cell in module.cells() {
                let Some(inst) = cell.get_instance() else {
                    continue;
                };
                self.check(inst.module, errs);
            }
        }
        self.checked.set(mid, true);
    }
}

impl Design {
    /// Validates the design, returns a list of errors found.  The design is considered valid iff the list is empty.
    ///
    /// Design validity rules are as follows:
    ///
    /// 1. A design must be valid before running a native pass.  Passes can assume this and panic or infinite loop when
    ///    given an invalid design.
    /// 2. A native pass must leave the design in valid state upon successful completion.
    /// 3. A design does not have to be valid at all times — native passes will, in normal course of operation,
    ///    create temporarily invalid designs.
    /// 4. A design can be operated on by external scripts, which may leave it in invalid state.  Thus, the validation
    ///    function here must be called before running native passes on a design that has been manipulated by scripts.
    /// 5. Emitting the design in text IR format does not require the design to be valid.
    ///    This allows the text IR to be used for problem diagnosis.
    ///
    /// The validity rules checked here are roughly as follows:
    ///
    /// 1. Every CellId in the IR refers to a cell of the correct type and width and on the correct plane, as appropriate.
    /// 2. The cell is internally consistent according to its rules (eg. number of mux inputs must be consistent with select input width).
    /// 3. [`Instance`] must refer to a valid module and have matching port and parameter lists.
    /// 4. The flags and annotations on a cell must be valid for its kind.
    /// 5. Module and cell flags must not conflict with one another.
    /// 6. Parameter and port cells must be correctly cross-referenced with module parameter and port lists.
    /// 7. [`InstanceOutput`] cells must be correctly cross-referenced with their instance cell.
    /// 8. There must be no infinite recursion of module instances.
    /// 9. There must be no cycles in combinatorial computations on the parameter and debug planes (but cycles on main plane are allowed).
    ///
    /// TODO: Once target-specific stuff lands, there will also be target-specific validation for target-specific cells/annotations/...
    /// TODO: At some point, there will be many optional validity constraints, to be used in later stages of synthesis (eg. fine cells only).
    pub fn validate_raw(&self) -> Vec<(Option<ModuleId>, Option<CellId>, String)> {
        let mut res = vec![];
        for mid in self.module_ids() {
            if let Some(module) = self.module(mid) {
                module.validate(&mut res);
            }
        }
        let mut checker = ModRecursionChecker {
            design: self,
            entered: EntityBitVec::repeat(false, self.module_ids().len()),
            checked: EntityBitVec::repeat(false, self.module_ids().len()),
        };
        for mid in self.module_ids() {
            checker.check(mid, &mut res);
        }
        res
    }
}
