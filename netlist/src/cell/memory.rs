use crate::{Const, ControlNet, Design, Net, Value};

/// An all-in-one random-access memory cell.
///
/// A memory is made of `depth` rows, each of them `width` bits wide.  While a memory row
/// is considered to be the basic access unit, we support the notion of "wide ports",
/// which access a (naturally aligned) power-of-two number of memory rows at once.
///
/// While any number and combination of read and write ports is considered valid in
/// the netlist, the rules for what memories can actually be realized in hardware depend
/// on the target, and can be quite byzantine.
///
/// There are no priority rules among write ports.  If more than one port writes to the same
/// memory bit at the same time, the value written is undefined.
///
/// The output of the memory cell consists of the read data from all the read ports,
/// concatenated in order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Memory {
    /// The number of rows in the memory.
    ///
    /// For every port on the memory, `depth` must be evenly divisible
    /// by `port.data_len / memory.width`.  This ensures that, for wide ports, every access
    /// is either completely in-bounds, or completely out-of-bounds.
    pub depth: usize,
    /// The width of single memory row.
    pub width: usize,
    /// Initial value for the memory, with all the rows concatenated in order.
    /// Must have a length equal to `depth * width`.
    pub init_value: Const,
    pub write_ports: Vec<MemoryWritePort>,
    pub read_ports: Vec<MemoryReadPort>,
}

/// A synchronous memory write port.
///
/// Asynchronous memory write ports are not currently supported.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemoryWritePort {
    /// The write address, selecting which row(s) to write.  The address is always counted
    /// in units of the port's data width.  Thus, if the port width is equal to the memory
    /// width, the address is equal to the row index.  However, for wide ports,
    /// the address is implicitly shifted left by `log2(port.data.len() / memory.width)` bits
    /// to obtain the first row index.
    ///
    /// The address can have any width.  If the address is too short to address all
    /// memory rows, so be it — higher rows will be unreachable by this port.
    ///
    /// Writes to out-of-bounds addresses do not modify the memory.
    pub addr: Value,
    /// The write data.  The width must be a power-of-two multiple of the memory width.
    pub data: Value,
    /// The write mask.  Must have the same width as `data`.  On every active clock edge,
    /// a `1` enables writing to the memory for the given data bit, `0` prevents writing.
    pub mask: Value,
    pub clock: ControlNet,
}

/// A memory read port, either synchronous or asynchronous.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemoryReadPort {
    /// The read address, selecting which row(s) to read.  Follows the same rules as
    /// [`MemoryWritePort`] address.
    ///
    /// Reading an out-of-bounds address results in an undefined value.
    pub addr: Value,
    /// The width of the read data.  Must be a power-of-two multiple of the memory width.
    pub data_len: usize,
    /// A flip-flop-like structure describing the synchronous read port, or `None` for asynchronous
    /// read ports.  If this is `None`, the read port continuously reads the memory row(s) selected
    /// by the `addr` value and outputs that as the read data.
    pub flip_flop: Option<MemoryReadFlipFlop>,
}

/// A structure describing a synchronous read port's control signals and behavior.
///
/// The behavior of a synchronous read port is mostly the same as an asynchronous read port
/// feeding a flip-flop with the controls described in this structure.  However, synchronous
/// read ports have special behavior when a read coincides with a write to the same memory
/// row.  This behavior is selected by the `relations` field.
///
/// The fields other than `relations` have the same meaning as the corresponding fields in
/// the [`FlipFlop`] structure.  The width of the reset, clear, and init values must be equal to
/// the `data_len` of the port.
///
/// [`FlipFlop`]: crate::FlipFlop
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemoryReadFlipFlop {
    pub clock: ControlNet,
    pub clear: ControlNet, // async reset
    pub load: ControlNet,  // async load
    pub reset: ControlNet, // sync reset
    pub enable: ControlNet,
    pub reset_over_enable: bool,

    pub load_data: Value,

    pub clear_value: Const,
    pub reset_value: Const,
    pub init_value: Const,

    /// The behavior of this read port during a simultaneous write by a given write port.
    ///
    /// Each entry in this vector describes the behavior of this read port in relation to a write
    /// port with the same index in the `write_ports` vector of the containing memory.
    ///
    /// Only ports sharing the same `clock` net can have defined relations — this vector must contain
    /// `MemoryPortRelation::Undefined` for every write port where `write_port.clock != read_port.clock`.
    ///
    /// If a given memory bit is simultanously written by more than one write port while being
    /// read, the read data is undefined (as is the value written to memory), regardless of
    /// the relations between the ports.
    pub relations: Vec<MemoryPortRelation>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MemoryPortRelation {
    #[default]
    /// When the same memory bit is written by the given write port while being read,
    /// the read value is undefined.
    Undefined,
    /// When the same memory bit is written by the given write port while being read,
    /// the read value is the value of the memory bit before the write.
    ReadBeforeWrite,
    /// When the same memory bit is written by the given write port while being read,
    /// the read value is the newly written value.
    Transparent,
}

impl Memory {
    pub fn output_len(&self) -> usize {
        self.read_ports.iter().map(|port| port.data_len).sum()
    }

    pub fn visit(&self, mut f: impl FnMut(Net)) {
        for write_port in &self.write_ports {
            write_port.visit(&mut f);
        }
        for read_port in &self.read_ports {
            read_port.visit(&mut f);
        }
    }

    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        for write_port in &mut self.write_ports {
            write_port.visit_mut(&mut f);
        }
        for read_port in &mut self.read_ports {
            read_port.visit_mut(&mut f);
        }
    }

    pub fn read_port_output_slice(&self, port_index: usize) -> std::ops::Range<usize> {
        let mut start = 0;
        for port in &self.read_ports[..port_index] {
            start += port.data_len;
        }
        let port = &self.read_ports[port_index];
        start..start + port.data_len
    }
}

impl MemoryWritePort {
    pub fn wide_log2(&self, memory: &Memory) -> usize {
        if memory.width == 0 {
            return 0;
        }
        (self.data.len() / memory.width).ilog2() as usize
    }

    pub fn visit(&self, mut f: impl FnMut(Net)) {
        self.addr.visit(&mut f);
        self.data.visit(&mut f);
        self.mask.visit(&mut f);
        self.clock.visit(&mut f);
    }

    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        self.addr.visit_mut(&mut f);
        self.data.visit_mut(&mut f);
        self.mask.visit_mut(&mut f);
        self.clock.visit_mut(&mut f);
    }
}

impl MemoryReadPort {
    pub fn new_asynchronous(addr: impl Into<Value>, data_len: usize) -> Self {
        Self { addr: addr.into(), data_len, flip_flop: None }
    }

    pub fn new_clocked(addr: impl Into<Value>, data_len: usize, clock: impl Into<ControlNet>) -> Self {
        Self {
            addr: addr.into(),
            data_len,
            flip_flop: Some(MemoryReadFlipFlop {
                clock: clock.into(),
                clear: ControlNet::ZERO,
                load: ControlNet::ZERO,
                reset: ControlNet::ZERO,
                enable: ControlNet::ONE,
                reset_over_enable: false,
                load_data: Value::undef(data_len),
                clear_value: Const::undef(data_len),
                reset_value: Const::undef(data_len),
                init_value: Const::undef(data_len),
                relations: vec![],
            }),
        }
    }

    pub fn wide_log2(&self, memory: &Memory) -> usize {
        if memory.width == 0 {
            return 0;
        }
        (self.data_len / memory.width).ilog2() as usize
    }

    pub fn visit(&self, mut f: impl FnMut(Net)) {
        self.addr.visit(&mut f);
        if let Some(ref flip_flop) = self.flip_flop {
            flip_flop.clock.visit(&mut f);
            flip_flop.clear.visit(&mut f);
            flip_flop.load.visit(&mut f);
            flip_flop.load_data.visit(&mut f);
            flip_flop.reset.visit(&mut f);
            flip_flop.enable.visit(&mut f);
        }
    }

    pub fn visit_mut(&mut self, mut f: impl FnMut(&mut Net)) {
        self.addr.visit_mut(&mut f);
        if let Some(ref mut flip_flop) = self.flip_flop {
            flip_flop.clock.visit_mut(&mut f);
            flip_flop.clear.visit_mut(&mut f);
            flip_flop.load.visit_mut(&mut f);
            flip_flop.load_data.visit_mut(&mut f);
            flip_flop.reset.visit_mut(&mut f);
            flip_flop.enable.visit_mut(&mut f);
        }
    }
}

impl MemoryReadFlipFlop {
    pub fn with_clock(self, clock: impl Into<ControlNet>) -> Self {
        Self { clock: clock.into(), ..self }
    }

    pub fn with_clear(self, clear: impl Into<ControlNet>) -> Self {
        Self { clear: clear.into(), ..self }
    }

    pub fn with_clear_value(self, clear: impl Into<ControlNet>, clear_value: impl Into<Const>) -> Self {
        Self { clear: clear.into(), clear_value: clear_value.into(), ..self }
    }

    pub fn with_load(self, load: impl Into<ControlNet>) -> Self {
        Self { load: load.into(), ..self }
    }

    pub fn with_load_data(self, load: impl Into<ControlNet>, load_data: impl Into<Value>) -> Self {
        Self { load: load.into(), load_data: load_data.into(), ..self }
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

    pub fn has_init_value(&self) -> bool {
        !self.init_value.is_undef()
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
}
