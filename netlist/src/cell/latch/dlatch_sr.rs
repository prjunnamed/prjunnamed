use crate::{Const, ControlNet, Design, Net, Value};

/// A d-latch cell.
///
/// The output is determined by the following rules:
///
/// - at the beginning of time, the output is set to `init_value`
/// - whenever `enable` as active, the output is set to `data`
/// - whenever `enable` is not active, the output value is unchanged
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DLatchSr {
    pub data: Value,
    pub enable: ControlNet,

    pub set: Value,
    pub reset: Value,

    /// Must have the same width as `data`.
    pub init_value: Const,
}

impl DLatchSr {
    pub fn new(data: Value, enable: impl Into<ControlNet>, set: impl Into<Value>, reset: impl Into<Value>) -> Self {
        let size = data.len();
        DLatchSr { data, enable: enable.into(), set: set.into(), reset: reset.into(), init_value: Const::undef(size) }
    }

    pub fn with_data(self, data: impl Into<Value>) -> Self {
        Self { data: data.into(), ..self }
    }

    pub fn with_enable(self, enable: impl Into<ControlNet>) -> Self {
        Self { enable: enable.into(), ..self }
    }

    pub fn with_init(self, value: impl Into<Const>) -> Self {
        let value = value.into();
        Self { init_value: value, ..self }
    }

    pub fn with_set(self, set: impl Into<Value>) -> Self {
        Self { set: set.into(), ..self }
    }

    pub fn with_reset(self, reset: impl Into<Value>) -> Self {
        Self { reset: reset.into(), ..self }
    }

    pub fn output_len(&self) -> usize {
        self.data.len()
    }

    pub fn has_enable(&self) -> bool {
        !self.enable.is_always(true)
    }

    pub fn has_init_value(&self) -> bool {
        !self.init_value.is_undef()
    }

    pub fn has_set(&self) -> bool {
        !self.set.is_undef()
    }

    pub fn has_reset(&self) -> bool {
        !self.reset.is_undef()
    }

    pub fn slice(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> DLatchSr {
        DLatchSr {
            data: self.data.slice(range.clone()),
            enable: self.enable,
            set: self.set.slice(range.clone()),
            reset: self.reset.slice(range.clone()),
            init_value: self.init_value.slice(range.clone()),
        }
    }

    pub fn unmap_enable(&mut self, design: &Design, output: &Value) {
        self.data = design.add_mux(self.enable, &self.data, output);
        self.enable = ControlNet::ONE;
    }

    pub fn invert(&mut self, design: &Design, output: &Value) -> Value {
        self.data = design.add_not(&self.data);
        self.init_value = self.init_value.not();
        let new_output = design.add_void(self.data.len());
        design.replace_value(output, design.add_not(&new_output));
        new_output
    }

    pub fn visit(&self, mut f: impl FnMut(Net)) {
        self.data.visit(&mut f);
        self.enable.visit(&mut f);
    }

    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        self.data.visit_mut(&mut f);
        self.enable.visit_mut(&mut f);
    }
}
