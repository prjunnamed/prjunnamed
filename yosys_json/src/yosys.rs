use jzon::{object, JsonValue};
use std::collections::{btree_map, BTreeMap};

use prjunnamed_netlist::{Const, ParamValue, Trit};

#[derive(Debug)]
pub struct SyntaxError(JsonValue);

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "syntax error near: {}", self.0)
    }
}

impl std::error::Error for SyntaxError {}

#[derive(Debug)]
pub struct MetadataTypeError;

impl std::fmt::Display for MetadataTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "unexpected metadata type")
    }
}

impl std::error::Error for MetadataTypeError {}

#[derive(Debug, Clone, Copy)]
pub enum Bit {
    Zero,
    One,
    Undef,
    HiZ,
    Net(usize),
}

impl From<Trit> for Bit {
    fn from(value: Trit) -> Self {
        match value {
            Trit::Undef => Bit::Undef,
            Trit::Zero => Bit::Zero,
            Trit::One => Bit::One,
        }
    }
}

impl TryFrom<JsonValue> for Bit {
    type Error = SyntaxError;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        match value.as_str() {
            Some("0") => Ok(Self::Zero),
            Some("1") => Ok(Self::One),
            Some("x") => Ok(Self::Undef),
            Some("z") => Ok(Self::HiZ),
            Some(_) => Err(SyntaxError(value)),
            None => match value.as_usize() {
                Some(index) => Ok(Bit::Net(index)),
                None => Err(SyntaxError(value)),
            },
        }
    }
}

impl From<Bit> for JsonValue {
    fn from(value: Bit) -> Self {
        match value {
            Bit::Zero => "0".into(),
            Bit::One => "1".into(),
            Bit::Undef => "x".into(),
            Bit::HiZ => "z".into(),
            Bit::Net(index) => index.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BitVector(pub Vec<Bit>);

impl BitVector {
    pub fn iter(&self) -> std::slice::Iter<Bit> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn concat(&self, other: &BitVector) -> Self {
        let mut bits = self.0.clone();
        bits.extend(&other.0);
        BitVector(bits)
    }
}

impl From<Bit> for BitVector {
    fn from(value: Bit) -> Self {
        BitVector(vec![value])
    }
}

impl From<Const> for BitVector {
    fn from(value: Const) -> Self {
        BitVector(value.iter().map(Into::into).collect::<Vec<_>>())
    }
}

impl TryFrom<JsonValue> for BitVector {
    type Error = SyntaxError;

    fn try_from(mut value: JsonValue) -> Result<Self, Self::Error> {
        if value.is_array() {
            let mut bits = vec![];
            for bit_value in value.members_mut() {
                bits.push(Bit::try_from(bit_value.take())?);
            }
            Ok(BitVector(bits))
        } else {
            Err(SyntaxError(value))
        }
    }
}

impl From<BitVector> for JsonValue {
    fn from(value: BitVector) -> JsonValue {
        JsonValue::Array(value.0.iter().copied().map(JsonValue::from).collect::<Vec<_>>())
    }
}

#[derive(Debug, Clone)]
pub enum MetadataValue {
    String(String),
    Const(Const),
}

impl MetadataValue {
    pub fn as_const(&self) -> Result<Const, MetadataTypeError> {
        match self {
            Self::Const(value) => Ok(value.clone()),
            _ => Err(MetadataTypeError),
        }
    }

    pub fn as_i32(&self) -> Result<i32, MetadataTypeError> {
        match self {
            Self::Const(value) if value.len() == 32 => {
                let mut i32_value = 0;
                for (index, trit) in value.iter().enumerate() {
                    match trit {
                        Trit::One => i32_value |= 1 << index,
                        Trit::Zero => (),
                        Trit::Undef => return Err(MetadataTypeError),
                    }
                }
                Ok(i32_value)
            }
            _ => Err(MetadataTypeError),
        }
    }

    pub fn as_bool(&self) -> Result<bool, MetadataTypeError> {
        match self {
            Self::Const(value) => match value.try_into() {
                Ok(0u64) => Ok(false),
                Ok(1u64) => Ok(true),
                _ => Err(MetadataTypeError),
            },
            _ => Err(MetadataTypeError),
        }
    }
}

impl From<Const> for MetadataValue {
    fn from(value: Const) -> Self {
        MetadataValue::Const(value)
    }
}

macro_rules! metadata_value_from_integral {
    ($int:ty) => {
        impl From<$int> for MetadataValue {
            fn from(value: $int) -> Self {
                let mut trits = vec![];
                for index in 0..<$int>::BITS {
                    trits.push(if value & (1 << index) != 0 { Trit::One } else { Trit::Zero });
                }
                MetadataValue::Const(trits.into())
            }
        }
    };
}

metadata_value_from_integral!(u64);
metadata_value_from_integral!(i64);
metadata_value_from_integral!(u32);
metadata_value_from_integral!(i32);

impl From<usize> for MetadataValue {
    fn from(value: usize) -> Self {
        (value as i32).into()
    }
}

impl From<bool> for MetadataValue {
    fn from(value: bool) -> Self {
        if value { 1i32 } else { 0 }.into()
    }
}

impl From<String> for MetadataValue {
    fn from(value: String) -> Self {
        MetadataValue::String(value)
    }
}

impl From<&String> for MetadataValue {
    fn from(value: &String) -> Self {
        MetadataValue::String(value.to_owned())
    }
}

impl From<&str> for MetadataValue {
    fn from(value: &str) -> Self {
        MetadataValue::String(value.to_owned())
    }
}

impl From<ParamValue> for MetadataValue {
    fn from(value: ParamValue) -> Self {
        match value {
            ParamValue::Const(value) => value.into(),
            ParamValue::Int(value) => value.into(),
            ParamValue::String(value) => value.into(),
            ParamValue::Float(_) => {
                unimplemented!("Yosys JSON does not define serialization for floating point parameters")
            }
        }
    }
}

impl From<&ParamValue> for MetadataValue {
    fn from(value: &ParamValue) -> Self {
        value.clone().into()
    }
}

enum MetadataValueClass {
    Bits,
    BitsAndSpaces,
    Other,
}

impl MetadataValueClass {
    fn compute(value: &str) -> MetadataValueClass {
        let mut class = MetadataValueClass::Bits;
        for char in value.chars() {
            match (&class, char) {
                (MetadataValueClass::Bits, '0' | '1' | 'x' | 'z') => (),
                (MetadataValueClass::BitsAndSpaces, ' ') => (),
                (MetadataValueClass::Bits, ' ') => class = MetadataValueClass::BitsAndSpaces,
                _ => class = MetadataValueClass::Other,
            }
        }
        class
    }
}

impl TryFrom<JsonValue> for MetadataValue {
    type Error = SyntaxError;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        if let Some(value) = value.as_str() {
            Ok(match MetadataValueClass::compute(value) {
                MetadataValueClass::Bits => MetadataValue::Const(
                    value
                        .chars()
                        .rev()
                        .map(|char| match char {
                            'x' => Trit::Undef,
                            '0' => Trit::Zero,
                            '1' => Trit::One,
                            _ => unreachable!(),
                        })
                        .collect::<Vec<_>>()
                        .into(),
                ),
                MetadataValueClass::BitsAndSpaces => MetadataValue::String(value.strip_suffix(" ").unwrap().to_owned()),
                MetadataValueClass::Other => MetadataValue::String(value.to_owned()),
            })
        } else {
            Err(SyntaxError(value))
        }
    }
}

impl From<MetadataValue> for JsonValue {
    fn from(value: MetadataValue) -> JsonValue {
        match value {
            MetadataValue::String(value) => match MetadataValueClass::compute(&value) {
                MetadataValueClass::Bits => value.into(),
                MetadataValueClass::BitsAndSpaces => (value + " ").into(),
                MetadataValueClass::Other => value.into(),
            },
            MetadataValue::Const(value) => value
                .iter()
                .rev()
                .map(|trit| match trit {
                    Trit::Undef => "x",
                    Trit::Zero => "0",
                    Trit::One => "1",
                })
                .collect::<String>()
                .into(),
        }
    }
}

#[derive(Debug)]
pub struct Metadata(pub BTreeMap<String, MetadataValue>);

impl Metadata {
    pub fn new() -> Metadata {
        Metadata(BTreeMap::new())
    }

    pub fn iter(&self) -> btree_map::Iter<String, MetadataValue> {
        self.0.iter()
    }

    pub fn get(&self, key: &str) -> Option<&MetadataValue> {
        self.0.get(key)
    }

    pub fn add(&mut self, key: &str, value: impl ToOwned<Owned = MetadataValue>) {
        self.0.insert(key.to_owned(), value.to_owned());
    }

    pub fn extend(&mut self, values: impl IntoIterator<Item = (String, MetadataValue)>) {
        self.0.extend(values);
    }
}

impl TryFrom<JsonValue> for Metadata {
    type Error = SyntaxError;

    fn try_from(mut value: JsonValue) -> Result<Self, Self::Error> {
        if value.is_object() {
            let mut entries = BTreeMap::new();
            for (name, value) in value.entries_mut() {
                entries.insert(name.to_owned(), value.take().try_into()?);
            }
            Ok(Metadata(entries))
        } else {
            Err(SyntaxError(value))
        }
    }
}

impl From<Metadata> for JsonValue {
    fn from(value: Metadata) -> JsonValue {
        value.0.into()
    }
}

#[derive(Debug)]
pub struct Map<V>(pub BTreeMap<String, V>);

impl<V> Map<V> {
    pub fn new() -> Map<V> {
        Map(BTreeMap::new())
    }

    pub fn iter(&self) -> btree_map::Iter<String, V> {
        self.0.iter()
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.0.get(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn add(&mut self, key: &str, value: V) {
        self.0.insert(key.to_owned(), value);
    }
}

impl<V> From<BTreeMap<String, V>> for Map<V> {
    fn from(value: BTreeMap<String, V>) -> Self {
        Map(value)
    }
}

impl<V: TryFrom<JsonValue, Error = SyntaxError>> TryFrom<JsonValue> for Map<V> {
    type Error = SyntaxError;

    fn try_from(mut value: JsonValue) -> Result<Self, Self::Error> {
        if value.is_object() {
            let mut entries = BTreeMap::new();
            for (name, value) in value.entries_mut() {
                entries.insert(name.to_owned(), value.take().try_into()?);
            }
            Ok(Map(entries))
        } else {
            Err(SyntaxError(value))
        }
    }
}

impl<V: Into<JsonValue>> From<Map<V>> for JsonValue {
    fn from(value: Map<V>) -> JsonValue {
        value.0.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDirection {
    Input,
    Output,
    Inout,
}

impl TryFrom<JsonValue> for PortDirection {
    type Error = SyntaxError;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        match value.as_str() {
            Some("input") => Ok(PortDirection::Input),
            Some("output") => Ok(PortDirection::Output),
            Some("inout") => Ok(PortDirection::Inout),
            _ => Err(SyntaxError(value)),
        }
    }
}

impl From<PortDirection> for JsonValue {
    fn from(value: PortDirection) -> JsonValue {
        match value {
            PortDirection::Input => "input".into(),
            PortDirection::Output => "output".into(),
            PortDirection::Inout => "inout".into(),
        }
    }
}

#[derive(Debug)]
pub struct PortDetails {
    pub direction: PortDirection,
    pub bits: BitVector,
    pub offset: usize,
    pub upto: bool,
    pub signed: bool,
}

impl PortDetails {
    pub fn new(direction: PortDirection, bits: BitVector) -> PortDetails {
        PortDetails { direction, bits, offset: 0, upto: false, signed: false }
    }
}

impl TryFrom<JsonValue> for PortDetails {
    type Error = SyntaxError;

    fn try_from(mut value: JsonValue) -> Result<Self, Self::Error> {
        let direction = PortDirection::try_from(value["direction"].take())?;
        let bits = BitVector::try_from(value["bits"].take())?;
        let offset = value["offset"].as_usize().unwrap_or(0);
        let upto = value["upto"].as_usize().unwrap_or(0) != 0;
        let signed = value["signed"].as_usize().unwrap_or(0) != 0;
        Ok(PortDetails { direction, bits, offset, upto, signed })
    }
}

impl From<PortDetails> for JsonValue {
    fn from(value: PortDetails) -> JsonValue {
        let mut json = object! {
            direction: value.direction,
            bits: value.bits,
        };
        if value.offset != 0 {
            json["offset"] = value.offset.into();
        }
        if value.upto {
            json["upto"] = 1.into();
        }
        if value.signed {
            json["signed"] = 1.into();
        }
        json
    }
}

#[derive(Debug)]
pub struct CellDetails {
    pub hide_name: bool,
    pub type_: String,
    pub parameters: Metadata,
    pub attributes: Metadata,
    pub port_directions: Map<PortDirection>,
    pub connections: Map<BitVector>,
}

impl CellDetails {
    pub fn new(type_: &str) -> CellDetails {
        CellDetails {
            hide_name: false,
            type_: type_.into(),
            parameters: Metadata::new(),
            attributes: Metadata::new(),
            port_directions: Map::new(),
            connections: Map::new(),
        }
    }

    pub fn attr(mut self, name: &str, value: impl Into<MetadataValue>) -> CellDetails {
        self.attributes.add(name, value.into());
        self
    }

    pub fn attrs(mut self, attrs: impl IntoIterator<Item = (String, MetadataValue)>) -> CellDetails {
        self.attributes.extend(attrs);
        self
    }

    pub fn param(mut self, name: &str, value: impl Into<MetadataValue>) -> CellDetails {
        self.parameters.add(name, value.into());
        self
    }

    pub fn port(mut self, name: &str, port_direction: PortDirection, connection: impl Into<BitVector>) -> CellDetails {
        self.port_directions.add(name, port_direction);
        self.connections.add(name, connection.into());
        self
    }

    pub fn input(self, name: &str, connection: impl Into<BitVector>) -> CellDetails {
        self.port(name, PortDirection::Input, connection)
    }

    pub fn output(self, name: &str, connection: impl Into<BitVector>) -> CellDetails {
        self.port(name, PortDirection::Output, connection)
    }

    pub fn inout(self, name: &str, connection: impl Into<BitVector>) -> CellDetails {
        self.port(name, PortDirection::Inout, connection)
    }

    pub fn add_to(mut self, name: &str, module: &mut Module) {
        assert!(!module.cells.contains_key(name));
        self.hide_name = name.starts_with('$');
        module.cells.add(name, self)
    }
}

impl TryFrom<JsonValue> for CellDetails {
    type Error = SyntaxError;

    fn try_from(mut value: JsonValue) -> Result<Self, Self::Error> {
        let hide_name = value["hide_name"].as_usize().unwrap_or(0) != 0;
        let type_ = value["type"].as_str().map(|s| s.to_owned()).ok_or(SyntaxError(value["type"].take()))?;
        let parameters = Metadata::try_from(value["parameters"].take())?;
        let attributes = Metadata::try_from(value["attributes"].take())?;
        let port_directions = Map::<PortDirection>::try_from(value["port_directions"].take())?;
        let connections = Map::<BitVector>::try_from(value["connections"].take())?;
        Ok(CellDetails { hide_name, type_, parameters, attributes, port_directions, connections })
    }
}

impl From<CellDetails> for JsonValue {
    fn from(value: CellDetails) -> JsonValue {
        object! {
            hide_name: if value.hide_name { 1 } else { 0 },
            type: value.type_,
            parameters: value.parameters,
            attributes: value.attributes,
            port_directions: value.port_directions,
            connections: value.connections,
        }
    }
}

#[derive(Debug)]
pub struct MemoryDetails {
    pub hide_name: bool,
    pub attributes: Metadata,
    pub width: usize,
    pub start_offset: usize,
    pub size: usize,
}

impl TryFrom<JsonValue> for MemoryDetails {
    type Error = SyntaxError;

    fn try_from(mut value: JsonValue) -> Result<Self, Self::Error> {
        let hide_name = value["hide_name"].as_usize().unwrap_or(0) != 0;
        let attributes = Metadata::try_from(value["attributes"].take())?;
        let width = value["width"].as_usize().ok_or(SyntaxError(value["width"].take()))?;
        let start_offset = value["start_offset"].as_usize().ok_or(SyntaxError(value["start_offset"].take()))?;
        let size = value["size"].as_usize().ok_or(SyntaxError(value["size"].take()))?;
        Ok(MemoryDetails { hide_name, attributes, width, start_offset, size })
    }
}

impl From<MemoryDetails> for JsonValue {
    fn from(value: MemoryDetails) -> JsonValue {
        object! {
            hide_name: if value.hide_name { 1 } else { 0 },
            attributes: value.attributes,
            width: value.width,
            start_offset: value.start_offset,
            size: value.size,
        }
    }
}

#[derive(Debug)]
pub struct NetDetails {
    pub hide_name: bool,
    pub attributes: Metadata,
    pub bits: BitVector,
    pub offset: usize,
    pub upto: bool,
    pub signed: bool,
}

impl NetDetails {
    pub fn new(bits: impl Into<BitVector>) -> NetDetails {
        NetDetails {
            hide_name: false,
            attributes: Metadata::new(),
            bits: bits.into(),
            offset: 0,
            upto: false,
            signed: false,
        }
    }

    pub fn attr(mut self, name: &str, value: impl Into<MetadataValue>) -> NetDetails {
        self.attributes.add(name, value.into());
        self
    }

    pub fn attrs(mut self, attrs: impl IntoIterator<Item = (String, MetadataValue)>) -> NetDetails {
        self.attributes.extend(attrs);
        self
    }

    pub fn add_to(mut self, name: &str, module: &mut Module) {
        assert!(!module.netnames.contains_key(name));
        self.hide_name = name.starts_with('$');
        module.netnames.add(name, self)
    }
}

impl TryFrom<JsonValue> for NetDetails {
    type Error = SyntaxError;

    fn try_from(mut value: JsonValue) -> Result<Self, Self::Error> {
        let hide_name = value["hide_name"].as_usize().unwrap_or(0) != 0;
        let attributes = Metadata::try_from(value["attributes"].take())?;
        let bits = BitVector::try_from(value["bits"].take())?;
        let offset = value["offset"].as_usize().unwrap_or(0);
        let upto = value["upto"].as_usize().unwrap_or(0) != 0;
        let signed = value["signed"].as_usize().unwrap_or(0) != 0;
        Ok(NetDetails { hide_name, attributes, bits, offset, upto, signed })
    }
}

impl From<NetDetails> for JsonValue {
    fn from(value: NetDetails) -> JsonValue {
        let mut json = object! {
            hide_name: if value.hide_name { 1 } else { 0 },
            attributes: value.attributes,
            bits: value.bits,
        };
        if value.offset != 0 {
            json["offset"] = value.offset.into();
        }
        if value.upto {
            json["upto"] = 1.into();
        }
        if value.signed {
            json["signed"] = 1.into();
        }
        json
    }
}

#[derive(Debug)]
pub struct Module {
    pub attributes: Metadata,
    pub parameter_default_values: Metadata,
    pub ports: Map<PortDetails>,
    pub cells: Map<CellDetails>,
    pub memories: Map<MemoryDetails>,
    pub netnames: Map<NetDetails>,
}

impl Module {
    pub fn new() -> Module {
        Module {
            attributes: Metadata::new(),
            parameter_default_values: Metadata::new(),
            ports: Map::new(),
            cells: Map::new(),
            memories: Map::new(),
            netnames: Map::new(),
        }
    }
}

impl TryFrom<JsonValue> for Module {
    type Error = SyntaxError;

    fn try_from(mut value: JsonValue) -> Result<Self, Self::Error> {
        let attributes = Metadata::try_from(value["attributes"].take())?;
        let parameter_default_values = if value.has_key("parameter_default_values") {
            Metadata::try_from(value["parameter_default_values"].take())?
        } else {
            Metadata::new()
        };
        let ports = Map::<PortDetails>::try_from(value["ports"].take())?;
        let cells = Map::<CellDetails>::try_from(value["cells"].take())?;
        let memories = if value.has_key("memories") {
            Map::<MemoryDetails>::try_from(value["memories"].take())?
        } else {
            Map::new()
        };
        let netnames = Map::<NetDetails>::try_from(value["netnames"].take())?;
        Ok(Module { attributes, parameter_default_values, ports, cells, memories, netnames })
    }
}

impl From<Module> for JsonValue {
    fn from(value: Module) -> JsonValue {
        object! {
            attributes: value.attributes,
            parameter_default_values: value.parameter_default_values,
            ports: value.ports,
            cells: value.cells,
            memories: value.memories,
            netnames: value.netnames,
        }
    }
}

// One Yosys design corresponds to many prjunnamed designs.
#[derive(Debug)]
pub struct Design {
    pub creator: String,
    pub modules: Map<Module>,
    // we don't care about AIG models
}

impl TryFrom<JsonValue> for Design {
    type Error = SyntaxError;

    fn try_from(mut value: JsonValue) -> Result<Self, Self::Error> {
        let creator = value["creator"].as_str().map(|s| s.to_owned()).ok_or(SyntaxError(value["creator"].take()))?;
        let modules = Map::<Module>::try_from(value["modules"].take())?;
        Ok(Design { creator, modules })
    }
}

impl From<Design> for JsonValue {
    fn from(value: Design) -> JsonValue {
        object! {
            creator: value.creator,
            modules: value.modules,
        }
    }
}
