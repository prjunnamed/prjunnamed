use std::{
    collections::{btree_map, BTreeMap, BTreeSet},
    sync::Arc,
};

use prjunnamed_netlist::{
    Const, ControlNet, Design, FlipFlop, Instance, IoBuffer, IoNet, IoValue, Net, ParamValue, Target, Trit, Value,
    Memory, MemoryWritePort, MemoryReadPort, MemoryReadFlipFlop, MemoryPortRelation, WithMetadataGuard, SourcePosition,
    MetaItem, MetaItemRef,
};

use crate::yosys;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(jzon::Error),
    Syntax(yosys::SyntaxError),
    MetaDataType(yosys::MetadataTypeError),
    Semantic,
    Unsupported(String),
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<jzon::Error> for Error {
    fn from(error: jzon::Error) -> Self {
        Self::Json(error)
    }
}

impl From<yosys::SyntaxError> for Error {
    fn from(error: yosys::SyntaxError) -> Self {
        Self::Syntax(error)
    }
}

impl From<yosys::MetadataTypeError> for Error {
    fn from(error: yosys::MetadataTypeError) -> Self {
        Self::MetaDataType(error)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(error) => write!(f, "I/O error: {}", error),
            Error::Json(error) => write!(f, "JSON parse error: {}", error),
            Error::Syntax(error) => write!(f, "{}", error),
            Error::MetaDataType(error) => write!(f, "{}", error),
            Error::Semantic => write!(f, "semantic error"),
            Error::Unsupported(feature) => write!(f, "unsupported feature: {}", feature),
        }
    }
}

impl std::error::Error for Error {}

struct ModuleImporter<'a> {
    module: &'a yosys::Module,
    design_io_ports: &'a BTreeSet<(&'a str, &'a str)>,
    io_nets: BTreeMap<usize, IoNet>,
    driven_nets: BTreeSet<usize>,
    nets: BTreeMap<usize, Net>,
    init: BTreeMap<usize, Trit>,
    design: &'a Design,
    scope_top: MetaItemRef<'a>,
}

impl ModuleImporter<'_> {
    fn drive(&mut self, bits: &yosys::BitVector, value: impl Into<Value>) {
        let value = value.into();
        assert_eq!(bits.len(), value.len());
        for (&bit, net) in bits.iter().zip(value.iter()) {
            let yosys::Bit::Net(ynet) = bit else { unreachable!() };
            assert!(!self.driven_nets.contains(&ynet));
            match self.nets.entry(ynet) {
                btree_map::Entry::Occupied(e) => {
                    self.design.replace_net(*e.get(), net);
                }
                btree_map::Entry::Vacant(e) => {
                    e.insert(net);
                }
            }
            if let Some(&io_net) = self.io_nets.get(&ynet) {
                self.design.add_iobuf(IoBuffer {
                    enable: ControlNet::Pos(Net::ONE),
                    output: Value::from(net),
                    io: IoValue::from(io_net),
                });
            }
            self.driven_nets.insert(ynet);
        }
    }

    fn port_drive(&mut self, cell: &yosys::CellDetails, port: &str, value: impl Into<Value>) {
        self.drive(cell.connections.get(port).unwrap(), value);
    }

    fn value(&mut self, bits: &yosys::BitVector) -> Value {
        let mut nets = vec![];
        for &bit in bits.iter() {
            let net = match bit {
                yosys::Bit::HiZ | yosys::Bit::Undef => Net::UNDEF,
                yosys::Bit::Zero => Net::ZERO,
                yosys::Bit::One => Net::ONE,
                yosys::Bit::Net(ynet) => *self.nets.entry(ynet).or_insert_with(|| self.design.add_void(1).unwrap_net()),
            };
            nets.push(net);
        }
        Value::from_iter(nets)
    }

    fn io_value(&self, bits: &yosys::BitVector) -> IoValue {
        let mut io = vec![];
        for &bit in bits.iter() {
            let yosys::Bit::Net(ynet) = bit else { unreachable!() };
            let io_net = *self.io_nets.get(&ynet).unwrap();
            io.push(io_net);
        }
        IoValue::from_iter(io)
    }

    fn port_value(&mut self, cell: &yosys::CellDetails, port: &str) -> Value {
        self.value(cell.connections.get(port).unwrap())
    }

    fn port_control_net(&mut self, cell: &yosys::CellDetails, port: &str) -> ControlNet {
        let net = self.port_value(cell, port).unwrap_net();
        let polarity = cell.parameters.get(&format!("{port}_POLARITY")).unwrap().as_bool().unwrap();
        if polarity { ControlNet::Pos(net) } else { ControlNet::Neg(net) }
    }

    fn init_value(&self, cell: &yosys::CellDetails, port: &str) -> Const {
        let bits = cell.connections.get(port).unwrap();
        let mut res = vec![];
        for &bit in bits.iter() {
            let yosys::Bit::Net(ynet) = bit else { unreachable!() };
            res.push(match self.init.get(&ynet) {
                None => Trit::Undef,
                Some(&val) => val,
            });
        }
        Const::from_iter(res)
    }

    fn value_ext(&mut self, cell: &yosys::CellDetails, port: &str, width: usize, signed: bool) -> Value {
        if signed { self.port_value(cell, port).sext(width) } else { self.port_value(cell, port).zext(width) }
    }

    fn handle_init(&mut self) -> Result<(), Error> {
        for net_details in self.module.netnames.0.values() {
            let Some(init) = net_details.attributes.get("init") else { continue };
            let init = init.as_const()?;
            if init.len() != net_details.bits.len() {
                Err(yosys::MetadataTypeError)?;
            }
            for (&bit, binit) in net_details.bits.iter().zip(init.iter()) {
                if binit == Trit::Undef {
                    continue;
                }
                let yosys::Bit::Net(ynet) = bit else { Err(yosys::MetadataTypeError)? };
                match self.init.entry(ynet) {
                    btree_map::Entry::Occupied(e) => {
                        assert_eq!(*e.get(), binit);
                    }
                    btree_map::Entry::Vacant(e) => {
                        e.insert(binit);
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_ports(&mut self) -> Result<(), Error> {
        // determine set of nets that need to be IOs from cell port connections.
        let mut io_net_conns = BTreeSet::new();
        for cell in self.module.cells.0.values() {
            for (port_name, bits) in cell.connections.iter() {
                if self.design_io_ports.contains(&(&cell.type_, port_name)) {
                    for &bit in bits.iter() {
                        let yosys::Bit::Net(net) = bit else { return Err(Error::Semantic) };
                        io_net_conns.insert(net);
                    }
                }
            }
        }

        // now create all IoValues we're going to need.
        let mut io_ports = BTreeSet::new();
        for (port_name, port) in self.module.ports.iter() {
            let mut is_inout = port.direction == yosys::PortDirection::Inout;
            for &bit in port.bits.iter() {
                if let yosys::Bit::Net(net) = bit {
                    if io_net_conns.contains(&net) {
                        is_inout = true;
                    }
                }
            }
            if is_inout {
                let ioval = self.design.add_io(port_name.clone(), port.bits.0.len());
                for (index, &bit) in port.bits.iter().enumerate() {
                    let yosys::Bit::Net(net) = bit else { return Err(Error::Semantic) };
                    match self.io_nets.entry(net) {
                        btree_map::Entry::Occupied(_) => {
                            return Err(Error::Semantic);
                        }
                        btree_map::Entry::Vacant(e) => {
                            e.insert(ioval[index]);
                        }
                    }
                }
                io_ports.insert(port_name);
            }
        }
        // handle all other ports
        for (port_name, port) in self.module.ports.iter() {
            if io_ports.contains(port_name) {
                continue;
            }
            match port.direction {
                yosys::PortDirection::Input => {
                    let value = self.design.add_input(port_name, port.bits.len());
                    self.drive(&port.bits, value);
                }
                yosys::PortDirection::Output => {
                    let value = self.value(&port.bits);
                    self.design.add_output(port_name, value);
                }
                yosys::PortDirection::Inout => unreachable!(),
            }
        }
        Ok(())
    }

    fn handle_names(&mut self) -> Result<(), Error> {
        for (name, details) in self.module.netnames.iter() {
            if details.hide_name {
                continue;
            }
            if details
                .bits
                .iter()
                .any(|bit| if let yosys::Bit::Net(net) = bit { self.io_nets.contains_key(net) } else { false })
            {
                continue;
            }
            let value = self.value(&details.bits);
            self.design.add_name(name, value);
        }
        Ok(())
    }

    fn use_attribute_metadata<'a>(
        &self,
        design: &'a Design,
        name: &str,
        attributes: &yosys::Metadata,
    ) -> WithMetadataGuard<'a> {
        fn parse_position(text: &str) -> Option<SourcePosition> {
            if let Some((line, column)) = text.split_once(".") {
                Some(SourcePosition { line: line.parse::<u32>().ok()?, column: column.parse::<u32>().ok()? })
            } else {
                Some(SourcePosition { line: text.parse::<u32>().ok()?, column: 0 })
            }
        }

        let mut items: Vec<MetaItemRef> = Vec::new();

        // TODO: add a counter for failed src parsing, and emit warning if nonzero at the end of import
        if let Some(yosys::MetadataValue::String(string)) = attributes.get("src") {
            for piece in string.split("|") {
                let Some((filename, loc_text)) = piece.rsplit_once(":") else {
                    continue;
                };
                let (start_text, end_text) = loc_text.split_once("-").unwrap_or((loc_text, loc_text));
                let Some(start) = parse_position(start_text) else {
                    continue;
                };
                let Some(end) = parse_position(end_text) else {
                    continue;
                };

                let meta_filename = design.add_metadata_string(filename);
                items.push(design.add_metadata_item(&MetaItem::Source { file: meta_filename, start, end }));
            }
        }

        let meta_none = MetaItemRef::from_iter(design, []);

        if let Some(yosys::MetadataValue::String(string)) = attributes.get("hdlname") {
            let mut scope = self.scope_top;
            let mut parts = string.split(' ');
            if let Some(last) = parts.next_back() {
                for name in parts {
                    let name = design.add_metadata_string(name);
                    scope = design.add_metadata_item(&MetaItem::NamedScope { name, source: meta_none, parent: scope });
                }
                let name = design.add_metadata_string(last);
                let ident = design.add_metadata_item(&MetaItem::Ident { name, scope });
                items.push(ident);
            }
        } else if !name.starts_with('$') {
            let name = design.add_metadata_string(name);
            let ident = design.add_metadata_item(&MetaItem::Ident { name, scope: self.scope_top });
            items.push(ident);
        }

        design.use_metadata(MetaItemRef::from_iter(design, items))
    }

    fn handle_cell(&mut self, name: &str, cell: &yosys::CellDetails) -> Result<(), Error> {
        let _guard = self.use_attribute_metadata(&self.design, name, &cell.attributes);

        match &cell.type_[..] {
            "$not" | "$pos" | "$neg" => {
                let a_width = cell.parameters.get("A_WIDTH").unwrap().as_i32()? as usize;
                let y_width = cell.parameters.get("Y_WIDTH").unwrap().as_i32()? as usize;
                let width = std::cmp::max(a_width, y_width);
                let a_signed = cell.parameters.get("A_SIGNED").unwrap().as_bool()?;
                let a = self.value_ext(cell, "A", width, a_signed);
                let value = match &cell.type_[..] {
                    "$not" => self.design.add_not(a),
                    "$pos" => a,
                    "$neg" => {
                        let value = self.design.add_not(a);
                        self.design.add_adc(value, Value::zero(width), Net::ONE)[..width].into()
                    }
                    _ => unreachable!(),
                };
                self.port_drive(cell, "Y", &value[..y_width]);
            }
            "$reduce_and" | "$reduce_or" | "$reduce_xor" | "$reduce_xnor" | "$reduce_bool" | "$logic_not" => {
                let width = cell.parameters.get("Y_WIDTH").unwrap().as_i32()? as usize;
                let a = self.port_value(cell, "A");
                let net = match &cell.type_[..] {
                    "$reduce_and" => self.design.add_eq(Value::ones(a.len()), a),
                    "$reduce_or" | "$reduce_bool" => self.design.add_ne(Value::zero(a.len()), a),
                    "$reduce_xor" => {
                        let mut net = Net::ZERO;
                        for bit in &a {
                            net = self.design.add_xor1(net, bit);
                        }
                        net
                    }
                    "$reduce_xnor" => {
                        let mut net = Net::ONE;
                        for bit in &a {
                            net = self.design.add_xor1(net, bit);
                        }
                        net
                    }
                    "$logic_not" => self.design.add_eq(Value::zero(a.len()), a),
                    _ => unreachable!(),
                };
                let mut value = Value::from(net);
                if width == 0 {
                    value = Value::new();
                } else if width > 1 {
                    value = value.zext(width);
                }
                self.port_drive(cell, "Y", value);
            }
            "$and" | "$or" | "$xor" | "$xnor" | "$add" | "$sub" | "$mul" | "$div" | "$mod" | "$divfloor"
            | "$modfloor" => {
                let a_width = cell.parameters.get("A_WIDTH").unwrap().as_i32()? as usize;
                let b_width = cell.parameters.get("B_WIDTH").unwrap().as_i32()? as usize;
                let y_width = cell.parameters.get("Y_WIDTH").unwrap().as_i32()? as usize;
                let width = std::cmp::max(std::cmp::max(a_width, b_width), y_width);
                let a_signed = cell.parameters.get("A_SIGNED").unwrap().as_bool()?;
                let b_signed = cell.parameters.get("B_SIGNED").unwrap().as_bool()?;
                assert_eq!(a_signed, b_signed);
                let a = self.value_ext(cell, "A", width, a_signed);
                let b = self.value_ext(cell, "B", width, b_signed);
                let value = match &cell.type_[..] {
                    "$and" => self.design.add_and(a, b),
                    "$or" => self.design.add_or(a, b),
                    "$xor" => self.design.add_xor(a, b),
                    "$xnor" => {
                        let val = self.design.add_xor(a, b);
                        self.design.add_not(val)
                    }
                    "$add" => self.design.add_adc(a, b, Net::ZERO)[..width].into(),
                    "$sub" => {
                        let inv_b = self.design.add_not(b);
                        self.design.add_adc(a, inv_b, Net::ONE)[..width].into()
                    }
                    "$mul" => self.design.add_mul(a, b),
                    "$div" | "$divfloor" if !a_signed => self.design.add_udiv(a, b),
                    "$mod" | "$modfloor" if !a_signed => self.design.add_umod(a, b),
                    "$div" => self.design.add_sdiv_trunc(a, b),
                    "$mod" => self.design.add_smod_trunc(a, b),
                    "$divfloor" => self.design.add_sdiv_floor(a, b),
                    "$modfloor" => self.design.add_smod_floor(a, b),
                    _ => unreachable!(),
                };
                self.port_drive(cell, "Y", &value[..y_width]);
            }
            "$alu" => {
                let a_width = cell.parameters.get("A_WIDTH").unwrap().as_i32()? as usize;
                let b_width = cell.parameters.get("B_WIDTH").unwrap().as_i32()? as usize;
                let y_width = cell.parameters.get("Y_WIDTH").unwrap().as_i32()? as usize;
                let width = std::cmp::max(std::cmp::max(a_width, b_width), y_width);
                let bi = self.port_value(cell, "BI").unwrap_net();
                let ci = self.port_value(cell, "CI").unwrap_net();
                let a_signed = cell.parameters.get("A_SIGNED").unwrap().as_bool()?;
                let b_signed = cell.parameters.get("B_SIGNED").unwrap().as_bool()?;
                let a = self.value_ext(cell, "A", width, a_signed);
                let b = self.value_ext(cell, "B", width, b_signed);
                let b_inv = self.design.add_not(&b);
                let b = self.design.add_mux(bi, b_inv, &b);
                let x = self.design.add_xor(&a, &b);
                self.port_drive(cell, "X", &x[..y_width]);
                let y = self.design.add_adc(&a, &b, ci);
                self.port_drive(cell, "Y", &y[..y_width]);
                let xor = self.design.add_xor(&a[1..], &b[1..]);
                let co = self.design.add_xor(xor, &y[1..width]).concat(y[width]);
                self.port_drive(cell, "CO", &co[..y_width]);
            }
            "$shl" | "$sshl" | "$shr" | "$sshr" | "$shift" | "$shiftx" => {
                let a_width = cell.parameters.get("A_WIDTH").unwrap().as_i32()? as usize;
                let y_width = cell.parameters.get("Y_WIDTH").unwrap().as_i32()? as usize;
                let width = std::cmp::max(a_width, y_width);
                let a_signed = cell.parameters.get("A_SIGNED").unwrap().as_bool()?;
                let b_signed = cell.parameters.get("B_SIGNED").unwrap().as_bool()?;
                let mut a = self.port_value(cell, "A");
                if a.len() < width {
                    if cell.type_ == "$shiftx" {
                        let a_width = a.len();
                        a = a.concat(Value::undef(width - a_width))
                    } else if a_signed {
                        a = a.sext(width);
                    } else {
                        a = a.zext(width);
                    }
                }
                let b = self.port_value(cell, "B");
                let value = match &cell.type_[..] {
                    "$shl" | "$sshl" => self.design.add_shl(a, b, 1),
                    "$shr" => self.design.add_ushr(a, b, 1),
                    "$sshr" => {
                        if a_signed {
                            self.design.add_sshr(a, b, 1)
                        } else {
                            self.design.add_ushr(a, b, 1)
                        }
                    }
                    "$shift" | "$shiftx" => {
                        let val_shr = if cell.type_ == "$shiftx" {
                            self.design.add_xshr(&a, &b, 1)
                        } else if a_signed {
                            self.design.add_sshr(&a, &b, 1)
                        } else {
                            self.design.add_ushr(&a, &b, 1)
                        };
                        if b_signed {
                            let b_inv = self.design.add_not(&b);
                            let b_neg =
                                Value::from(&self.design.add_adc(Value::zero(b.len()), &b_inv, Net::ONE)[..b.len()]);
                            let val_shl = self.design.add_shl(&a, b_neg, 1);
                            self.design.add_mux(b[b.len() - 1], val_shl, val_shr)
                        } else {
                            val_shr
                        }
                    }
                    _ => unreachable!(),
                };
                self.port_drive(cell, "Y", &value[..y_width]);
            }
            "$lt" | "$le" | "$gt" | "$ge" | "$eq" | "$ne" => {
                let y_width = cell.parameters.get("Y_WIDTH").unwrap().as_i32()? as usize;
                let a_width = cell.parameters.get("A_WIDTH").unwrap().as_i32()? as usize;
                let b_width = cell.parameters.get("B_WIDTH").unwrap().as_i32()? as usize;
                let a_signed = cell.parameters.get("A_SIGNED").unwrap().as_bool()?;
                let b_signed = cell.parameters.get("B_SIGNED").unwrap().as_bool()?;
                let width = std::cmp::max(a_width, b_width);
                assert_eq!(a_signed, b_signed);
                let a = self.value_ext(cell, "A", width, a_signed);
                let b = self.value_ext(cell, "B", width, b_signed);
                let (mut net, inv) = match &cell.type_[..] {
                    "$lt" if !a_signed => (self.design.add_ult(a, b), false),
                    "$gt" if !a_signed => (self.design.add_ult(b, a), false),
                    "$le" if !a_signed => (self.design.add_ult(b, a), true),
                    "$ge" if !a_signed => (self.design.add_ult(a, b), true),
                    "$lt" if a_signed => (self.design.add_slt(a, b), false),
                    "$gt" if a_signed => (self.design.add_slt(b, a), false),
                    "$le" if a_signed => (self.design.add_slt(b, a), true),
                    "$ge" if a_signed => (self.design.add_slt(a, b), true),
                    "$eq" => (self.design.add_eq(a, b), false),
                    "$ne" => (self.design.add_eq(a, b), true),
                    _ => unreachable!(),
                };
                if inv {
                    net = self.design.add_not1(net);
                }
                let mut value = Value::from(net);
                if y_width == 0 {
                    value = Value::new();
                } else if y_width > 1 {
                    value = value.zext(width);
                }
                self.port_drive(cell, "Y", value);
            }
            "$logic_and" | "$logic_or" => {
                let y_width = cell.parameters.get("Y_WIDTH").unwrap().as_i32()? as usize;
                let a = self.port_value(cell, "A");
                let a = self.design.add_eq(Value::zero(a.len()), a);
                let a = self.design.add_not(a);
                let b = self.port_value(cell, "B");
                let b = self.design.add_eq(Value::zero(b.len()), b);
                let b = self.design.add_not(b);
                let mut value = match &cell.type_[..] {
                    "$logic_and" => self.design.add_and(a, b),
                    "$logic_or" => self.design.add_or(a, b),
                    _ => unreachable!(),
                };
                if y_width == 0 {
                    value = Value::new();
                } else if y_width > 1 {
                    value = value.zext(y_width);
                }
                self.port_drive(cell, "Y", value);
            }
            "$mux" => {
                let a = self.port_value(cell, "A");
                let b = self.port_value(cell, "B");
                let s = self.port_value(cell, "S");
                assert_eq!(a.len(), b.len());
                let y = self.design.add_mux(s.unwrap_net(), b, a);
                self.port_drive(cell, "Y", y);
            }
            "$bmux" => {
                let mut value = self.port_value(cell, "A");
                let sel = self.port_value(cell, "S");
                for s in sel.iter().rev() {
                    assert_eq!(value.len() % 2, 0);
                    let lo = Value::from(&value[..(value.len() / 2)]);
                    let hi = Value::from(&value[(value.len() / 2)..]);
                    value = self.design.add_mux(s, hi, lo);
                }
                self.port_drive(cell, "Y", value);
            }
            "$pmux" => {
                let mut value = self.port_value(cell, "A");
                let b = self.port_value(cell, "B");
                let sel = self.port_value(cell, "S");
                assert_eq!(b.len(), value.len() * sel.len());
                for (i, s) in sel.iter().enumerate() {
                    value = self.design.add_mux(s, &b[(i * value.len())..((i + 1) * value.len())], value);
                }
                self.port_drive(cell, "Y", value);
            }
            "$tribuf" => {
                let output = self.port_value(cell, "A");
                let enable = self.port_value(cell, "EN");
                let enable = ControlNet::Pos(enable.unwrap_net());
                let bits = cell.connections.get("Y").unwrap();
                let io = self.io_value(bits);
                let value = self.design.add_iobuf(IoBuffer { output, enable, io });
                for (&bit, net) in bits.iter().zip(value.iter()) {
                    let yosys::Bit::Net(ynet) = bit else { unreachable!() };
                    assert!(!self.driven_nets.contains(&ynet));
                    match self.nets.entry(ynet) {
                        btree_map::Entry::Occupied(e) => {
                            self.design.replace_net(*e.get(), net);
                        }
                        btree_map::Entry::Vacant(e) => {
                            e.insert(net);
                        }
                    }
                    self.driven_nets.insert(ynet);
                }
            }
            "$dff" | "$dffe" | "$adff" | "$adffe" | "$sdff" | "$sdffe" | "$sdffce" | "$aldff" => {
                let data = self.port_value(cell, "D");
                                let enable = if cell.connections.contains_key("EN") {
                    self.port_control_net(cell, "EN")
                } else {
                    ControlNet::Pos(Net::ONE)
                };
                let (clear, clear_value) = if cell.connections.contains_key("ARST") {
                    (self.port_control_net(cell, "ARST"), cell.parameters.get("ARST_VALUE").unwrap().as_const()?)
                } else {
                    (ControlNet::Pos(Net::ZERO), Const::undef(data.len()))
                };
                let (reset, reset_value) = if cell.connections.contains_key("SRST") {
                    (self.port_control_net(cell, "SRST"), cell.parameters.get("SRST_VALUE").unwrap().as_const()?)
                } else {
                    (ControlNet::Pos(Net::ZERO), Const::undef(data.len()))
                };

                let (load, load_data) = if cell.connections.contains_key("ALOAD") {
                    (self.port_control_net(cell, "ALOAD"), self.port_value(cell, "AD"))
                } else {
                    (ControlNet::Pos(Net::ZERO), Value::undef(data.len()))
                };

                let clock = self.port_control_net(cell, "CLK");
                let init_value = self.init_value(cell, "Q");
                let q = self.design.add_dff(FlipFlop {
                    data,
                    clock,
                    enable,
                    reset,
                    reset_over_enable: cell.type_ != "$sdffce",
                    clear,
                    load,
                    load_data,
                    init_value,
                    reset_value,
                    clear_value,
                });
                self.port_drive(cell, "Q", q);
            }
            "$mem_v2" => {
                let offset = usize::try_from(cell.parameters.get("OFFSET").unwrap().as_i32()?).unwrap();
                let size = usize::try_from(cell.parameters.get("SIZE").unwrap().as_i32()?).unwrap();
                let depth = offset + size;
                let abits = usize::try_from(cell.parameters.get("ABITS").unwrap().as_i32()?).unwrap();
                let width = usize::try_from(cell.parameters.get("WIDTH").unwrap().as_i32()?).unwrap();
                let mut init_value = cell.parameters.get("INIT").unwrap().as_const().unwrap();
                assert_eq!(init_value.len(), size * width);
                init_value = Const::undef(offset * width).concat(init_value);

                let mut memory = Memory { depth, width, init_value, write_ports: vec![], read_ports: vec![] };

                let wr_ports = usize::try_from(cell.parameters.get("WR_PORTS").unwrap().as_i32()?).unwrap();
                let wr_clk_enable = cell.parameters.get("WR_CLK_ENABLE").unwrap().as_const().unwrap();
                let wr_clk_polarity = cell.parameters.get("WR_CLK_POLARITY").unwrap().as_const().unwrap();
                let wr_priority_mask = cell.parameters.get("WR_PRIORITY_MASK").unwrap().as_const().unwrap();
                let wr_wide_continuation = cell.parameters.get("WR_WIDE_CONTINUATION").unwrap().as_const().unwrap();
                let wr_clk = self.port_value(cell, "WR_CLK");
                let wr_en = self.port_value(cell, "WR_EN");
                let wr_addr = self.port_value(cell, "WR_ADDR");
                let wr_data = self.port_value(cell, "WR_DATA");
                let mut wr_port_indices = vec![];
                if wr_ports != 0 {
                    assert!(wr_priority_mask.iter().all(|x| x == Trit::Zero));
                    assert!(wr_clk_enable.iter().all(|x| x == Trit::One));
                }
                let mut index = 0;
                while index < wr_ports {
                    let mut end_index = index + 1;
                    while end_index < wr_ports && wr_wide_continuation[end_index] == Trit::One {
                        end_index += 1;
                    }
                    let wide = end_index - index;
                    let wide_log2 = wide.ilog2() as usize;
                    assert!(wide.is_power_of_two());
                    wr_port_indices.push(index);
                    let clock = if wr_clk_polarity[index] == Trit::One {
                        ControlNet::Pos(wr_clk[index])
                    } else {
                        ControlNet::Neg(wr_clk[index])
                    };
                    let addr = wr_addr.slice(index * abits..(index + 1) * abits).slice(wide_log2..);
                    let data = wr_data.slice(index * width..end_index * width);
                    let mask = wr_en.slice(index * width..end_index * width);
                    memory.write_ports.push(MemoryWritePort { clock, addr, data, mask });
                    index = end_index;
                }

                let rd_ports = usize::try_from(cell.parameters.get("RD_PORTS").unwrap().as_i32()?).unwrap();
                let rd_clk_enable = cell.parameters.get("RD_CLK_ENABLE").unwrap().as_const().unwrap();
                let rd_clk_polarity = cell.parameters.get("RD_CLK_POLARITY").unwrap().as_const().unwrap();
                let rd_transparency_mask = cell.parameters.get("RD_TRANSPARENCY_MASK").unwrap().as_const().unwrap();
                let rd_collision_x_mask = cell.parameters.get("RD_COLLISION_X_MASK").unwrap().as_const().unwrap();
                let rd_wide_continuation = cell.parameters.get("RD_WIDE_CONTINUATION").unwrap().as_const().unwrap();
                let rd_ce_over_srst = cell.parameters.get("RD_CE_OVER_SRST").unwrap().as_const().unwrap();
                let rd_arst_value = cell.parameters.get("RD_ARST_VALUE").unwrap().as_const().unwrap();
                let rd_srst_value = cell.parameters.get("RD_SRST_VALUE").unwrap().as_const().unwrap();
                let rd_init_value = cell.parameters.get("RD_INIT_VALUE").unwrap().as_const().unwrap();
                let rd_clk = self.port_value(cell, "RD_CLK");
                let rd_en = self.port_value(cell, "RD_EN");
                let rd_srst = self.port_value(cell, "RD_SRST");
                let rd_arst = self.port_value(cell, "RD_ARST");
                let rd_addr = self.port_value(cell, "RD_ADDR");
                let mut index = 0;
                while index < rd_ports {
                    let mut end_index = index + 1;
                    while end_index < wr_ports && rd_wide_continuation[end_index] == Trit::One {
                        end_index += 1;
                    }
                    let wide = end_index - index;
                    let wide_log2 = wide.ilog2() as usize;
                    assert!(wide.is_power_of_two());
                    let flip_flop = if rd_clk_enable[index] == Trit::One {
                        let clock = if rd_clk_polarity[index] == Trit::One {
                            ControlNet::Pos(rd_clk[index])
                        } else {
                            ControlNet::Neg(rd_clk[index])
                        };
                        let mut relations = vec![];
                        for (new_index, &orig_index) in wr_port_indices.iter().enumerate() {
                            let mask_index = index * wr_ports + orig_index;
                            let relation = if memory.write_ports[new_index].clock != clock
                                || rd_collision_x_mask[mask_index] == Trit::One
                            {
                                MemoryPortRelation::Undefined
                            } else if rd_transparency_mask[mask_index] == Trit::One {
                                MemoryPortRelation::Transparent
                            } else {
                                MemoryPortRelation::ReadBeforeWrite
                            };
                            relations.push(relation);
                        }
                        Some(MemoryReadFlipFlop {
                            clock,
                            clear: rd_arst[index].into(),
                            load: ControlNet::Pos(Net::ZERO),
                            reset: rd_srst[index].into(),
                            enable: rd_en[index].into(),
                            reset_over_enable: rd_ce_over_srst[index] != Trit::One,
                            load_data: Value::undef(wide * width),
                            clear_value: rd_arst_value.slice(index * width..end_index * width),
                            reset_value: rd_srst_value.slice(index * width..end_index * width),
                            init_value: rd_init_value.slice(index * width..end_index * width),
                            relations,
                        })
                    } else {
                        None
                    };
                    let addr = rd_addr.slice(index * abits..(index + 1) * abits).slice(wide_log2..);
                    let data_len = wide * width;
                    memory.read_ports.push(MemoryReadPort { addr, data_len, flip_flop });
                    index = end_index;
                }

                let mem_out = self.design.add_memory(memory);
                self.port_drive(cell, "RD_DATA", mem_out);
            }
            "$scopeinfo" => {
                // not quite yet
            }
            "$memrd" | "$memwr" | "$meminit" | "$memrd_v2" | "$memwr_v2" | "$meminit_v2" => {
                return Err(Error::Unsupported(format!(
                    "{} cell; run the Yosys `memory_collect` pass before writing JSON",
                    cell.type_
                )));
            }
            _ => {
                if cell.type_.starts_with('$') {
                    return Err(Error::Unsupported(format!("{} cell", cell.type_)));
                }
                // instance
                let mut out_bits = vec![];
                let mut next_out = 0;
                let mut inputs = BTreeMap::new();
                let mut outputs = BTreeMap::new();
                let mut ios = BTreeMap::new();
                for (name, bits) in cell.connections.iter() {
                    let direction = *cell.port_directions.get(name).unwrap();
                    if self.design_io_ports.contains(&(&cell.type_, name)) || direction == yosys::PortDirection::Inout {
                        ios.insert(name.clone(), self.io_value(bits));
                    } else if direction == yosys::PortDirection::Output {
                        let range = next_out..(next_out + bits.len());
                        next_out += bits.len();
                        out_bits.push((bits, range.clone()));
                        outputs.insert(name.clone(), range);
                    } else {
                        inputs.insert(name.clone(), self.value(bits));
                    }
                }
                let mut parameters = BTreeMap::new();
                for (name, val) in cell.parameters.iter() {
                    let val = match val {
                        yosys::MetadataValue::Const(val) => ParamValue::Const(val.clone()),
                        yosys::MetadataValue::String(val) => ParamValue::String(val.clone()),
                    };
                    parameters.insert(name.clone(), val);
                }
                let output = self.design.add_other(Instance {
                    kind: cell.type_.strip_prefix('\\').unwrap_or(cell.type_.as_str()).to_string(),
                    params: parameters,
                    inputs,
                    outputs,
                    ios,
                });
                for (bits, range) in out_bits {
                    self.drive(bits, &output[range]);
                }
            }
        }
        Ok(())
    }

    fn handle_undriven_nets(&mut self) {
        for (&ynet, &io_net) in &self.io_nets {
            if self.driven_nets.contains(&ynet) {
                continue;
            }
            let Some(&net) = self.nets.get(&ynet) else { continue };
            self.design.replace_net(
                net,
                self.design
                    .add_iobuf(IoBuffer {
                        output: Value::undef(1),
                        enable: ControlNet::Pos(Net::ZERO),
                        io: io_net.into(),
                    })
                    .unwrap_net(),
            );
            self.driven_nets.insert(ynet);
        }
        for (&ynet, &net) in &self.nets {
            if self.driven_nets.contains(&ynet) {
                continue;
            }
            self.design.replace_net(net, Net::UNDEF);
        }
    }
}

fn import_module(
    target: Option<Arc<dyn Target>>,
    module: &yosys::Module,
    design_io_ports: &BTreeSet<(&str, &str)>,
) -> Result<Option<Design>, Error> {
    if let Some(val) = module.attributes.get("blackbox") {
        if val.as_bool()? {
            return Ok(None);
        }
    }

    let mut design = Design::with_target(target);
    let meta_none = MetaItemRef::from_iter(&design, []);
    let scope_top = design.add_metadata_item(&MetaItem::NamedScope {
        name: design.add_metadata_string("top"),
        source: meta_none,
        parent: meta_none,
    });

    let mut importer = ModuleImporter {
        module,
        design_io_ports,
        io_nets: BTreeMap::new(),
        driven_nets: BTreeSet::new(),
        nets: BTreeMap::new(),
        init: BTreeMap::new(),
        design: &design,
        scope_top,
    };

    importer.handle_init()?;
    importer.handle_ports()?;
    importer.handle_names()?;
    for (name, cell) in &module.cells.0 {
        importer.handle_cell(name, cell)?;
    }
    importer.handle_undriven_nets();
    design.compact();

    Ok(Some(design))
}

fn index_io_ports(design: &yosys::Design) -> Result<BTreeSet<(&str, &str)>, Error> {
    let mut io_ports: BTreeSet<(&str, &str)> = BTreeSet::new();
    io_ports.insert(("$tribuf", "Y"));
    for (mod_name, module) in design.modules.iter() {
        for (port_name, port) in module.ports.iter() {
            if port.direction == yosys::PortDirection::Inout {
                io_ports.insert((mod_name, port_name));
                continue;
            }
            let Some(net_details) = module.netnames.get(port_name) else { continue };
            if let Some(val) = net_details.attributes.get("iopad_external_pin") {
                if val.as_bool()? == true {
                    io_ports.insert((mod_name, port_name));
                }
            }
        }
    }
    Ok(io_ports)
}

pub fn import(
    target: Option<Arc<dyn Target>>,
    reader: &mut impl std::io::Read,
) -> Result<BTreeMap<String, Design>, Error> {
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    let json = jzon::parse(text.as_str())?;
    let yosys_design = yosys::Design::try_from(json)?;

    let io_ports = index_io_ports(&yosys_design)?;
    let mut designs = BTreeMap::new();
    for (name, module) in yosys_design.modules.iter() {
        if let Some(design) = import_module(target.clone(), module, &io_ports)? {
            designs.insert(name.clone(), design);
        }
    }
    Ok(designs)
}
