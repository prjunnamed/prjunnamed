use std::{collections::BTreeMap, ops::Range};

use crate::{IoValue, Net, ParamValue, Value};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Instance {
    pub kind: String,
    pub params: BTreeMap<String, ParamValue>,
    pub inputs: BTreeMap<String, Value>,
    pub outputs: BTreeMap<String, Range<usize>>,
    pub ios: BTreeMap<String, IoValue>,
}

impl Instance {
    pub fn new(kind: impl Into<String>) -> Self {
        Instance {
            kind: kind.into(),
            params: Default::default(),
            inputs: Default::default(),
            outputs: Default::default(),
            ios: Default::default(),
        }
    }

    pub fn output_len(&self) -> usize {
        self.outputs.values().map(|range| range.len()).sum()
    }

    pub fn get_param_string(&self, name: &str) -> Option<&str> {
        let val = self.params.get(name)?;
        if let ParamValue::String(val) = val { Some(val) } else { None }
    }

    pub fn add_param(&mut self, name: impl Into<String>, value: impl Into<ParamValue>) {
        self.params.insert(name.into(), value.into());
    }

    pub fn rename_param(&mut self, name_from: impl AsRef<str>, name_to: impl Into<String>) {
        if let Some(param) = self.params.remove(name_from.as_ref()) {
            assert!(self.params.insert(name_to.into(), param).is_none());
        }
    }

    pub fn rename_input(&mut self, name_from: impl AsRef<str>, name_to: impl Into<String>) {
        if let Some(input) = self.inputs.remove(name_from.as_ref()) {
            assert!(self.inputs.insert(name_to.into(), input).is_none());
        }
    }

    pub fn rename_output(&mut self, name_from: impl AsRef<str>, name_to: impl Into<String>) {
        if let Some(output) = self.outputs.remove(name_from.as_ref()) {
            assert!(self.outputs.insert(name_to.into(), output).is_none());
        }
    }

    pub fn rename_io(&mut self, name_from: impl AsRef<str>, name_to: impl Into<String>) {
        if let Some(io) = self.ios.remove(name_from.as_ref()) {
            assert!(self.ios.insert(name_to.into(), io).is_none());
        }
    }

    pub fn visit(&self, mut f: impl FnMut(Net)) {
        for val in self.inputs.values() {
            val.visit(&mut f);
        }
    }

    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        for val in self.inputs.values_mut() {
            val.visit_mut(&mut f);
        }
    }
}
