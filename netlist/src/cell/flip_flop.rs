use crate::{value::ControlNets, Const, ControlNet, Design, Net, Value};

/// A flip-flop cell.
///
/// The output is determined by the following rules:
///
/// - at the beginning of time, the output is set to `init_value`
/// - whenever `clear` as active, the output is set to `clear_value`
/// - whenever `clear` is not active and `load` is active, the output is set to `load_data`
/// - whenever `clear` and `load` are not active, and an active edge happens on `clock`:
///   - if `reset_over_enable` is true:
///     - if `reset` is active, the output is set to `reset_value`
///     - if `enable` is false, output value is unchanged
///   - if `reset_over_enable` is false:
///     - if `enable` is false, output value is unchanged
///     - if `reset` is active, the output is set to `reset_value`
///   - otherwise, the output is set to `data`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlipFlop {
    pub data: Value,
    /// The clock.  The active edge is rising if it is a [`ControlNet::Pos`], and falling if it is
    /// a [`ControlNet::Neg`].
    pub clock: ControlNet,
    /// Asynchronous reset.
    pub clear: ControlNets,
    /// Asynchronous load.
    pub load: ControlNet,
    /// Synchronous reset.
    pub reset: ControlNet,
    /// Clock enable.
    pub enable: ControlNet,
    /// If true, `reset` has priority over `enable`.  Otherwise, `enable` has priority over `reset`.
    pub reset_over_enable: bool,

    /// Must have the same width as `data`.
    pub clear_value: Const,
    /// Must have the same width as `data` and `load_data`.
    pub load_data: Value,
    /// Must have the same width as `data`.
    pub reset_value: Const,
    /// Must have the same width as `data`.
    pub init_value: Const,
}

impl FlipFlop {
    pub fn new(data: Value, clock: impl Into<ControlNet>) -> Self {
        let size = data.len();
        FlipFlop {
            data,
            clock: clock.into(),
            clear: ControlNets::zero(size),
            load: ControlNet::ZERO,
            reset: ControlNet::ZERO,
            enable: ControlNet::ONE,
            reset_over_enable: false,
            clear_value: Const::undef(size),
            load_data: Value::undef(size),
            reset_value: Const::undef(size),
            init_value: Const::undef(size),
        }
    }

    pub fn with_data(self, data: impl Into<Value>) -> Self {
        Self { data: data.into(), ..self }
    }

    pub fn with_clock(self, clock: impl Into<ControlNet>) -> Self {
        Self { clock: clock.into(), ..self }
    }

    pub fn with_clear(self, clear: impl Into<ControlNets>) -> Self {
        Self { clear: clear.into(), ..self }
    }

    pub fn with_clear_value(self, clear: impl Into<ControlNets>, clear_value: impl Into<Const>) -> Self {
        Self { clear: clear.into(), clear_value: clear_value.into(), ..self }
    }

    pub fn with_reset(self, reset: impl Into<ControlNet>) -> Self {
        Self { reset: reset.into(), reset_over_enable: false, ..self }
    }

    pub fn with_reset_value(self, reset: impl Into<ControlNet>, reset_value: impl Into<Const>) -> Self {
        Self { reset: reset.into(), reset_over_enable: false, reset_value: reset_value.into(), ..self }
    }

    pub fn with_enable(self, enable: impl Into<ControlNet>) -> Self {
        Self { enable: enable.into(), reset_over_enable: true, ..self }
    }

    pub fn with_init(self, value: impl Into<Const>) -> Self {
        let value = value.into();
        Self { clear_value: value.clone(), reset_value: value.clone(), init_value: value, ..self }
    }

    pub fn with_load_data(self, load_data: impl Into<Value>) -> Self {
        Self { load_data: load_data.into(), ..self }
    }

    pub fn with_load(self, load: impl Into<ControlNet>) -> Self {
        Self { load: load.into(), ..self }
    }

    pub fn output_len(&self) -> usize {
        self.data.len()
    }

    pub fn has_clock(&self) -> bool {
        !self.clock.is_const()
    }

    pub fn has_enable(&self) -> bool {
        !self.enable.is_always(true)
    }

    pub fn has_reset(&self) -> bool {
        !self.reset.is_always(false)
    }

    pub fn has_reset_value(&self) -> bool {
        !self.reset_value.is_undef()
    }

    pub fn has_clear(&self) -> bool {
        !self.clear.is_always(false)
    }

    pub fn has_clear_value(&self) -> bool {
        !self.clear_value.is_undef()
    }

    pub fn has_load(&self) -> bool {
        !self.load.is_always(false)
    }

    pub fn has_load_data(&self) -> bool {
        !self.load_data.is_undef()
    }

    pub fn has_init_value(&self) -> bool {
        !self.init_value.is_undef()
    }

    pub fn slice(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> FlipFlop {
        FlipFlop {
            data: self.data.slice(range.clone()),
            clock: self.clock,
            clear: self.clear.slice(range.clone()),
            load: self.load,
            reset: self.reset,
            enable: self.enable,
            reset_over_enable: self.reset_over_enable,
            clear_value: self.clear_value.slice(range.clone()),
            load_data: self.load_data.slice(range.clone()),
            reset_value: self.reset_value.slice(range.clone()),
            init_value: self.init_value.slice(range.clone()),
        }
    }

    pub fn remap_reset_over_enable(&mut self, design: &Design) {
        if self.reset_over_enable {
            return;
        }
        self.reset_over_enable = true;
        if self.reset.is_always(false) || self.enable.is_always(true) {
            return;
        }
        let reset = self.reset.into_pos(design);
        let enable = self.enable.into_pos(design);
        self.reset = ControlNet::Pos(design.add_and(reset, enable).unwrap_net());
    }

    pub fn remap_enable_over_reset(&mut self, design: &Design) {
        if !self.reset_over_enable {
            return;
        }
        self.reset_over_enable = false;
        if self.reset.is_always(false) || self.enable.is_always(true) {
            return;
        }
        let reset = self.reset.into_pos(design);
        let enable = self.enable.into_pos(design);
        self.enable = ControlNet::Pos(design.add_or(reset, enable).unwrap_net());
    }

    pub fn unmap_reset(&mut self, design: &Design) {
        self.remap_enable_over_reset(design);
        self.data = design.add_mux(self.reset, &self.reset_value, &self.data);
        self.reset = ControlNet::ZERO;
    }

    pub fn unmap_load(&mut self, design: &Design) {
        self.data = design.add_mux(self.load, &self.load_data, &self.data);
        self.load = ControlNet::ZERO;
    }

    pub fn unmap_enable(&mut self, design: &Design, output: &Value) {
        self.remap_reset_over_enable(design);
        self.data = design.add_mux(self.enable, &self.data, output);
        self.enable = ControlNet::ONE;
    }

    pub fn invert(&mut self, design: &Design, output: &Value) -> Value {
        self.data = design.add_not(&self.data);
        self.load_data = design.add_not(&self.load_data);
        self.clear_value = self.clear_value.not();
        self.reset_value = self.reset_value.not();
        self.init_value = self.init_value.not();
        let new_output = design.add_void(self.data.len());
        design.replace_value(output, design.add_not(&new_output));
        new_output
    }

    pub fn visit(&self, mut f: impl FnMut(Net)) {
        self.data.visit(&mut f);
        self.load_data.visit(&mut f);
        self.clock.visit(&mut f);
        self.enable.visit(&mut f);
        self.reset.visit(&mut f);
        self.load.visit(&mut f);
        self.clear.visit(&mut f);
    }

    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        self.data.visit_mut(&mut f);
        self.load_data.visit_mut(&mut f);
        self.clock.visit_mut(&mut f);
        self.enable.visit_mut(&mut f);
        self.reset.visit_mut(&mut f);
        self.load.visit_mut(&mut f);
        self.clear.visit_mut(&mut f);
    }
}
