use std::borrow::Cow;
use std::fmt::{Debug, Display};
use std::error::Error;
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::{CellRef, Const, Design, Instance, IoValue, ParamValue, TargetCell, Trit, Value};

pub trait Target: Debug {
    /// Get target name. The name of the target can be used to construct a new instance of it.
    fn name(&self) -> &str;

    /// Get target options. Target options define the exact variant of the target, and include
    /// the device family, device name, speed grade, temperature variant, and any other parameters
    /// required to fully define a specific device.
    fn options(&self) -> BTreeMap<String, String>;

    /// Get prototypes of target cells. Prototypes in conjunction with the target cell itself define
    /// the connectivity and properties of the primitive instance.
    fn prototype(&self, name: &str) -> Option<&TargetPrototype>;

    /// Validate target-specific constraints. Conformance with the prototype is always validated
    /// by `Design::validate`.
    fn validate(&self, design: &Design, cell: &TargetCell);

    /// Convert generic instances into target cells.
    fn import(&self, design: &mut Design) -> Result<(), TargetImportError>;

    /// Convert target cells into generic instances.
    fn export(&self, design: &mut Design);

    /// Run the complete synthesis flow.
    fn synthesize(&self, design: &mut Design) -> Result<(), ()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetParamKind {
    Bits(usize),
    Bool,
    IntEnum(Vec<i64>),
    StringEnum(Vec<String>),
}

impl TargetParamKind {
    pub fn is_valid(&self, value: &ParamValue) -> bool {
        match (self, value) {
            (TargetParamKind::Bits(width), ParamValue::Const(value)) => value.len() == *width,
            (TargetParamKind::Bool, ParamValue::Const(value)) => value.len() == 1 && !value.has_undef(),
            (TargetParamKind::IntEnum(items), ParamValue::Int(value)) => items.contains(value),
            (TargetParamKind::StringEnum(items), ParamValue::String(value)) => items.contains(value),
            _ => false,
        }
    }

    pub fn cast(&self, value: &ParamValue) -> Option<ParamValue> {
        match (self, value) {
            (TargetParamKind::Bits(width), ParamValue::Const(value)) if value.len() == *width => {
                Some(ParamValue::Const(value.clone()))
            }
            (TargetParamKind::Bits(width), ParamValue::Int(value)) if *value >= 0 && *value < 1 << *width => {
                Some(ParamValue::Const(Const::from_uint((*value).try_into().unwrap(), *width)))
            }
            (TargetParamKind::Bool, ParamValue::Int(value)) => match *value {
                1 => Some(ParamValue::Const(Const::ones(1))),
                0 => Some(ParamValue::Const(Const::zero(1))),
                _ => None,
            },
            (TargetParamKind::Bool, ParamValue::Const(value)) => match value.as_uint() {
                Some(1) => Some(ParamValue::Const(Const::ones(1))),
                Some(0) => Some(ParamValue::Const(Const::zero(1))),
                _ => None,
            },
            (TargetParamKind::IntEnum(items), ParamValue::Const(value)) => {
                let value = value.as_int()?;
                if items.contains(&value) {
                    Some(ParamValue::Int(value))
                } else {
                    None
                }
            }
            (TargetParamKind::IntEnum(items), ParamValue::Int(value)) if items.contains(value) => {
                Some(ParamValue::Int(*value))
            }
            (TargetParamKind::StringEnum(items), ParamValue::String(value)) if items.contains(value) => {
                Some(ParamValue::String(value.clone()))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetParam {
    pub name: String,
    pub kind: TargetParamKind,
    pub default: ParamValue,
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInput {
    pub name: String,
    pub default: Const,
    pub invert_param: Option<usize>,
    pub range: Range<usize>,
}

impl TargetInput {
    pub fn len(&self) -> usize {
        self.default.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetOutput {
    pub name: String,
    pub range: Range<usize>,
}

impl TargetOutput {
    pub fn len(&self) -> usize {
        self.range.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetIo {
    pub name: String,
    pub range: Range<usize>,
}

impl TargetIo {
    pub fn len(&self) -> usize {
        self.range.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetCellPurity {
    Pure,
    HasState,
    HasEffects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPrototype {
    pub purity: TargetCellPurity,
    pub params: Vec<TargetParam>,
    pub params_by_name: BTreeMap<String, usize>,
    pub inputs: Vec<TargetInput>,
    pub inputs_by_name: BTreeMap<String, usize>,
    pub input_len: usize,
    pub outputs: Vec<TargetOutput>,
    pub outputs_by_name: BTreeMap<String, usize>,
    pub output_len: usize,
    pub ios: Vec<TargetIo>,
    pub ios_by_name: BTreeMap<String, usize>,
    pub io_len: usize,
}

impl TargetPrototype {
    fn new(purity: TargetCellPurity) -> TargetPrototype {
        TargetPrototype {
            purity,
            params: vec![],
            params_by_name: Default::default(),
            inputs: vec![],
            inputs_by_name: Default::default(),
            input_len: 0,
            outputs: vec![],
            outputs_by_name: Default::default(),
            output_len: 0,
            ios: vec![],
            ios_by_name: Default::default(),
            io_len: 0,
        }
    }

    pub fn new_pure() -> TargetPrototype {
        Self::new(TargetCellPurity::Pure)
    }

    pub fn new_has_state() -> TargetPrototype {
        Self::new(TargetCellPurity::HasState)
    }

    pub fn new_has_effects() -> TargetPrototype {
        Self::new(TargetCellPurity::HasEffects)
    }

    fn add_param_raw(mut self, name: impl Into<String>, kind: TargetParamKind, default: ParamValue) -> Self {
        let name = name.into();
        let index = self.params.len();
        self.params_by_name.insert(name.clone(), index);
        self.params.push(TargetParam { name, kind, default, index });
        self
    }

    pub fn add_param_bits(self, name: impl Into<String>, default: impl Into<Const>) -> Self {
        let default = default.into();
        self.add_param_raw(name, TargetParamKind::Bits(default.len()), ParamValue::Const(default))
    }

    pub fn add_param_bool(self, name: impl Into<String>, default: bool) -> Self {
        self.add_param_raw(name, TargetParamKind::Bool, ParamValue::Const(Trit::from(default).into()))
    }

    pub fn add_param_int_enum(self, name: impl Into<String>, variants: &[i64]) -> Self {
        let variants = variants.to_vec();
        let default = variants[0];
        self.add_param_raw(name, TargetParamKind::IntEnum(variants), ParamValue::Int(default))
    }

    pub fn add_param_string_enum(self, name: impl Into<String>, variants: &[impl AsRef<str>]) -> Self {
        let variants = Vec::from_iter(variants.iter().map(|s| s.as_ref().to_owned()));
        let default = variants[0].clone();
        self.add_param_raw(name, TargetParamKind::StringEnum(variants), ParamValue::String(default))
    }

    fn add_input_raw(
        mut self,
        name: impl Into<String>,
        default: impl Into<Const>,
        invert_param: Option<usize>,
    ) -> Self {
        let default = default.into();
        let range = self.input_len..self.input_len + default.len();
        self.input_len += default.len();
        let name = name.into();
        self.inputs_by_name.insert(name.clone(), self.inputs.len());
        self.inputs.push(TargetInput { name, default, invert_param, range });
        self
    }

    pub fn add_input(self, name: impl Into<String>, default: impl Into<Const>) -> Self {
        self.add_input_raw(name, default, None)
    }

    pub fn add_input_invertible(
        self,
        name: impl Into<String>,
        default: impl Into<Const>,
        invert_param: impl AsRef<str>,
    ) -> Self {
        let invert_param = self.params_by_name[invert_param.as_ref()];
        let default = default.into();
        match &self.params[invert_param].kind {
            TargetParamKind::Bits(width) if *width == default.len() => (),
            TargetParamKind::Bool if default.len() == 1 => (),
            _ => panic!("invalid kind for inversion parameter {invert_param:?}"),
        }
        self.add_input_raw(name, default, Some(invert_param))
    }

    pub fn add_output(mut self, name: impl Into<String>, width: usize) -> Self {
        let range = self.output_len..self.output_len + width;
        self.output_len += width;
        let name = name.into();
        self.outputs_by_name.insert(name.clone(), self.outputs.len());
        self.outputs.push(TargetOutput { name, range });
        self
    }

    pub fn add_io(mut self, name: impl Into<String>, width: usize) -> Self {
        let range = self.io_len..self.io_len + width;
        self.io_len += width;
        let name = name.into();
        self.ios_by_name.insert(name.clone(), self.ios.len());
        self.ios.push(TargetIo { name, range });
        self
    }

    pub fn get_param(&self, name: &str) -> Option<&TargetParam> {
        self.params_by_name.get(name).map(|&index| &self.params[index])
    }

    pub fn get_input(&self, name: &str) -> Option<&TargetInput> {
        self.inputs_by_name.get(name).map(|&index| &self.inputs[index])
    }

    pub fn get_output(&self, name: &str) -> Option<&TargetOutput> {
        self.outputs_by_name.get(name).map(|&index| &self.outputs[index])
    }

    pub fn get_io(&self, name: &str) -> Option<&TargetIo> {
        self.ios_by_name.get(name).map(|&index| &self.ios[index])
    }

    pub fn apply_param(&self, target_cell: &mut TargetCell, name: impl AsRef<str>, value: impl Into<ParamValue>) {
        let name = name.as_ref();
        if let Some(TargetParam { index, .. }) = self.get_param(name) {
            target_cell.params[*index] = value.into();
        } else {
            panic!("parameter {:?} does not exist for target cell", name);
        }
    }

    pub fn apply_input<'a>(
        &self,
        target_cell: &mut TargetCell,
        name: impl AsRef<str>,
        value: impl Into<Cow<'a, Value>>,
    ) {
        let (name, value) = (name.as_ref(), value.into());
        if let Some(TargetInput { range, .. }) = self.get_input(name) {
            target_cell.inputs[range.clone()].copy_from_slice(&value[..]);
        } else {
            panic!("input {:?} does not exist for target cell", name);
        }
    }

    pub fn apply_io<'a>(
        &self,
        target_cell: &mut TargetCell,
        name: impl AsRef<str>,
        value: impl Into<Cow<'a, IoValue>>,
    ) {
        let (name, value) = (name.as_ref(), value.into());
        if let Some(TargetIo { range, .. }) = self.get_io(name) {
            target_cell.ios[range.clone()].copy_from_slice(&value[..]);
        } else {
            panic!("input {:?} does not exist for target cell", name);
        }
    }

    pub fn extract_param<'a>(&self, target_cell: &'a TargetCell, name: impl AsRef<str>) -> &'a ParamValue {
        let name = name.as_ref();
        if let Some(TargetParam { index, .. }) = self.get_param(name) {
            &target_cell.params[*index]
        } else {
            panic!("param {:?} does not exist for target cell", name);
        }
    }

    pub fn extract_param_bool(&self, target_cell: &TargetCell, name: impl AsRef<str>) -> bool {
        let name = name.as_ref();
        if let Some(TargetParam { index, kind, .. }) = self.get_param(name) {
            assert_eq!(*kind, TargetParamKind::Bool);
            let ParamValue::Const(ref value) = target_cell.params[*index] else { unreachable!() };
            value[0] == Trit::One
        } else {
            panic!("param {:?} does not exist for target cell", name);
        }
    }

    pub fn extract_input(&self, target_cell: &TargetCell, name: impl AsRef<str>) -> Value {
        let name = name.as_ref();
        if let Some(TargetInput { range, .. }) = self.get_input(name) {
            target_cell.inputs.slice(range.clone())
        } else {
            panic!("input {:?} does not exist for target cell", name);
        }
    }

    pub fn extract_output(&self, target_cell_output: &Value, name: impl AsRef<str>) -> Value {
        let name = name.as_ref();
        if let Some(TargetOutput { range, .. }) = self.get_output(name) {
            target_cell_output.slice(range.clone())
        } else {
            panic!("output {:?} does not exist for target cell", name);
        }
    }

    pub fn target_cell_to_instance(&self, cell: &TargetCell) -> Instance {
        let mut result = Instance::new(cell.kind.clone());
        for (index, param) in self.params.iter().enumerate() {
            result.params.insert(param.name.clone(), cell.params[index].clone());
        }
        for input in &self.inputs {
            result.inputs.insert(input.name.clone(), cell.inputs.slice(input.range.clone()));
        }
        for output in &self.outputs {
            result.outputs.insert(output.name.clone(), output.range.clone());
        }
        for io in &self.ios {
            result.ios.insert(io.name.clone(), cell.ios.slice(io.range.clone()));
        }
        result
    }

    pub fn instance_to_target_cell(
        &self,
        design: &Design,
        instance: &Instance,
        instance_output: Value,
    ) -> Result<(TargetCell, Value), TargetCellImportError> {
        let mut target_cell = TargetCell::new(instance.kind.clone(), self);
        for (name, value) in &instance.params {
            let param = self.get_param(name).ok_or_else(|| TargetCellImportError::UnknownParameter(name.clone()))?;
            let Some(value) = param.kind.cast(value) else {
                return Err(TargetCellImportError::ParameterValueInvalid(name.clone(), value.clone()));
            };
            target_cell.params[param.index] = value;
        }
        for (name, value) in &instance.inputs {
            let input = self.get_input(name).ok_or_else(|| TargetCellImportError::UnknownInput(name.clone()))?;
            if value.len() != input.len() {
                return Err(TargetCellImportError::InputSizeMismatch(name.clone()));
            }
            target_cell.inputs[input.range.clone()].copy_from_slice(&value[..]);
        }
        for (name, value) in &instance.ios {
            let io = self.get_io(name).ok_or_else(|| TargetCellImportError::UnknownIo(name.clone()))?;
            if value.len() != io.len() {
                return Err(TargetCellImportError::IoSizeMismatch(name.clone()));
            }
            target_cell.ios[io.range.clone()].copy_from_slice(&value[..]);
        }
        let mut target_output = design.add_void(self.output_len);
        for (name, range) in &instance.outputs {
            let output = self.get_output(name).ok_or_else(|| TargetCellImportError::UnknownOutput(name.clone()))?;
            if range.len() != output.len() {
                return Err(TargetCellImportError::OutputSizeMismatch(name.clone()));
            }
            target_output[output.range.clone()].copy_from_slice(&instance_output[range.clone()]);
        }
        Ok((target_cell, target_output))
    }
}

#[derive(Debug, Clone)]
pub enum TargetCellImportError {
    UnknownParameter(String),
    ParameterTypeMismatch(String),
    ParameterValueInvalid(String, ParamValue),
    UnknownInput(String),
    InputSizeMismatch(String),
    UnknownOutput(String),
    OutputSizeMismatch(String),
    UnknownIo(String),
    IoSizeMismatch(String),
}

impl Display for TargetCellImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetCellImportError::UnknownParameter(name) => write!(f, "unknown parameter {name:?}"),
            TargetCellImportError::ParameterTypeMismatch(name) => write!(f, "type mismatch for parameter {name:?}"),
            TargetCellImportError::ParameterValueInvalid(name, value) => {
                let value = match value {
                    ParamValue::Const(value) => format!("{value}"),
                    ParamValue::Int(value) => format!("{value}"),
                    ParamValue::Float(value) => format!("{value}"),
                    ParamValue::String(value) => format!("{value:?}"),
                };
                write!(f, "invalid value {value} for parameter {name:?}")
            }
            TargetCellImportError::UnknownInput(name) => write!(f, "unknown input {name:?}"),
            TargetCellImportError::InputSizeMismatch(name) => write!(f, "size mismatch for input {name:?}"),
            TargetCellImportError::UnknownOutput(name) => write!(f, "unknown output {name:?}"),
            TargetCellImportError::OutputSizeMismatch(name) => write!(f, "size mismatch for output {name:?}"),
            TargetCellImportError::UnknownIo(name) => write!(f, "unknown io {name:?}"),
            TargetCellImportError::IoSizeMismatch(name) => write!(f, "size mismatch for io {name:?}"),
        }
    }
}

impl Error for TargetCellImportError {}

#[derive(Debug, Clone)]
pub struct TargetImportError {
    cell_index: usize,
    pub cause: TargetCellImportError,
}

impl TargetImportError {
    pub fn new(cell_ref: CellRef, cause: TargetCellImportError) -> TargetImportError {
        TargetImportError { cell_index: cell_ref.debug_index(), cause }
    }

    pub fn unknown_parameter(cell_ref: CellRef, name: impl Into<String>) -> TargetImportError {
        Self::new(cell_ref, TargetCellImportError::UnknownParameter(name.into()))
    }

    pub fn parameter_type_mismatch(cell_ref: CellRef, name: impl Into<String>) -> TargetImportError {
        Self::new(cell_ref, TargetCellImportError::ParameterTypeMismatch(name.into()))
    }

    pub fn parameter_value_invalid(cell_ref: CellRef, name: impl Into<String>, value: ParamValue) -> TargetImportError {
        Self::new(cell_ref, TargetCellImportError::ParameterValueInvalid(name.into(), value))
    }

    pub fn unknown_input(cell_ref: CellRef, name: impl Into<String>) -> TargetImportError {
        Self::new(cell_ref, TargetCellImportError::UnknownInput(name.into()))
    }

    pub fn input_size_mismatch(cell_ref: CellRef, name: impl Into<String>) -> TargetImportError {
        Self::new(cell_ref, TargetCellImportError::InputSizeMismatch(name.into()))
    }

    pub fn unknown_output(cell_ref: CellRef, name: impl Into<String>) -> TargetImportError {
        Self::new(cell_ref, TargetCellImportError::UnknownOutput(name.into()))
    }

    pub fn output_size_mismatch(cell_ref: CellRef, name: impl Into<String>) -> TargetImportError {
        Self::new(cell_ref, TargetCellImportError::OutputSizeMismatch(name.into()))
    }

    pub fn unknown_io(cell_ref: CellRef, name: impl Into<String>) -> TargetImportError {
        Self::new(cell_ref, TargetCellImportError::UnknownIo(name.into()))
    }

    pub fn io_size_mismatch(cell_ref: CellRef, name: impl Into<String>) -> TargetImportError {
        Self::new(cell_ref, TargetCellImportError::IoSizeMismatch(name.into()))
    }
}

impl Display for TargetImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error importing cell %{}: {}", self.cell_index, self.cause)
    }
}

impl Error for TargetImportError {}

#[derive(Debug, Clone)]
pub struct UnknownTargetError(String);

impl Display for UnknownTargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown target {:?} (known targets:", self.0)?;
        for target in REGISTRY.lock().unwrap().keys() {
            write!(f, " {target}")?;
        }
        write!(f, ")")
    }
}

impl Error for UnknownTargetError {}

// I'm sorry! I'm sorry!! I'm trying to remove this but trait aliases aren't stable yet...
static REGISTRY: Mutex<
    BTreeMap<String, Box<dyn Fn(BTreeMap<String, String>) -> Result<Arc<dyn Target>, Box<dyn Error>> + Send>>,
> = Mutex::new(BTreeMap::new());

pub fn register_target(
    name: impl Into<String>,
    builder: impl Fn(BTreeMap<String, String>) -> Result<Arc<dyn Target>, Box<dyn Error>> + Send + 'static,
) {
    let mut registry = REGISTRY.lock().unwrap();
    assert!(registry.insert(name.into(), Box::new(builder)).is_none());
}

pub fn create_target(name: &str, options: BTreeMap<String, String>) -> Result<Arc<dyn Target>, Box<dyn Error>> {
    let registry = REGISTRY.lock().unwrap();
    match registry.get(name).map(|builder| builder(options)) {
        Some(target) => target,
        None => Err(UnknownTargetError(name.into()))?,
    }
}
