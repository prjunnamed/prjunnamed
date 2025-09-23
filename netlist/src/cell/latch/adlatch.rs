use crate::{Const, ControlNet, Design, Net, Value};

/// A d-latch cell.
///
/// The output is determined by the following rules:
///
/// - at the beginning of time, the output is set to `init_value`
/// - whenever `enable` as active, the output is set to `data`
/// - whenever `enable` is not active, the output value is unchanged
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ADLatch {
    pub data: Value,
    pub enable: ControlNet,

    pub arst: ControlNet,

    /// Must have the same width as `data`.
    pub init_value: Const,
    pub arst_value: Const,
}

impl ADLatch {
    pub fn new(data: Value, enable: impl Into<ControlNet>, arst: impl Into<ControlNet>) -> Self {
        let size = data.len();
        ADLatch {
            data,
            enable: enable.into(),
            arst: arst.into(),
            init_value: Const::undef(size),
            arst_value: Const::undef(size),
        }
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

    pub fn with_arst(self, arst: impl Into<ControlNet>) -> Self {
        Self { arst: arst.into(), ..self }
    }

    pub fn with_reset_value(self, value: impl Into<Const>) -> Self {
        let value = value.into();
        Self { arst_value: value, ..self }
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

    pub fn has_reset_value(&self) -> bool {
        !self.arst_value.is_undef()
    }

    pub fn slice(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> ADLatch {
        ADLatch {
            data: self.data.slice(range.clone()),
            enable: self.enable,
            init_value: self.init_value.slice(range.clone()),
            arst: self.arst,
            arst_value: self.arst_value.slice(range.clone()),
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
