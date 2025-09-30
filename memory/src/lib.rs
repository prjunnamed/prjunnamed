//! This library contains various common utilities for writing target-specific memory lowering passes.
//!
//! Target-specific memory lowering passes are organized roughly as follows:
//!
//! - for each memory in the design:
//!   - a list of candidate lowerings is assembled:
//!     - if a lowering mode (ff / lutram / blockram / hugeram) is explicitly selected, only the relevant
//!       target cells types are considered
//!     - for each available type of target cell:
//!       - hard requirements are checked, and the target cell is rejected if they are unmet, eg:
//!         - memories with more write ports than the target cell are rejected
//!         - memories with asynchronous read ports are rejected if not supported by target cell
//!       - a set of candidate "swizzle" transformations is assembled, for rearranging the memory geometry
//!         to match the target cell
//!         - the candidate swizzles are created according to target-specific rules, with some help from
//!           common functions for eg. legalizing write mask granularity
//!       - the best swizzle is selected, according to some metric (such as inserted soft mux depth or number
//!         of used target cells)
//!       - when a memory needs to be split/duplicated because of excessive read port count, each of the
//!         split/duplicated sub-memories will have its own swizzle
//!       - if a swizzle is egregiously bad (uses more resources than the entire FPGA has available), it is
//!         rejected
//!     - if applicable, a "fallback" lowering candidate is inserted into the list of candidates
//!       - no fallback lowering if multiple write port domains present
//!       - lowering to FFs for RAMs (very expensive)
//!       - lowering to LUTs for ROMs (somewhat expensive)
//!   - if no candidates are available, synthesis is aborted with a fatal error
//!   - one of the candidate lowerings is picked, according to some target-specific heuristic
//!     - initial heuristic will simply switch between target cells based on memory depth
//!     - in the future, we will want to take target resource counts into account (moving the depth threshold
//!       to avoid overfilling the FPGA)
//!   - if the "fallback" lowering candidate has been selected, the common lowering code is called to perform it
//!   - otherwise, target cell lowering is performed:
//!     - the memory is split into sub-memories (using common code), as required to satisfy max read port count,
//!       then each sub-memory is processed independently
//!     - various fixup transformations (implemented in common code, but driven by target code) are performed on
//!       the memory to match it with target cell capabilities:
//!       - sync read ports are converted to async if required
//!       - read port enable / clear / reset / initial value are unmapped and emulated if not supported
//!       - transparency is unmapped and emulated if not supported
//!       - read-first relations are emulated if not supported
//!     - the swizzle (computed earlier) is applied to the memory (using common code), aligning its geometry
//!       with the target cell
//!     - the memory is consumed and converted into actual target cells
//!     - any target-specific post-processing (such as merging output pipeline registers) is performed

use std::collections::{btree_map, BTreeMap};

use prjunnamed_netlist::{
    Const, ControlNet, Design, FlipFlop, Memory, MemoryPortRelation, MemoryReadFlipFlop, MemoryReadPort,
    MemoryWritePort, Net, Trit, Value,
};

/// Describes a "swizzle" transformation, used to align memory geometry to target requirements.
///
/// This structure is constructed by the target, with some help from common code.
///
/// The swizzle transformation involves the following steps:
///
/// 1. The data swizzle is applied to the write data, write mask, and read data signals.  This is used
///    to insert dummy data bits to ensure that target cell's write mask granularity requirements are met.
///
/// 2. The address bits selected by `soft_addr_bits_mask` are removed from all ports, and implemented using soft
///    logic.  This has the following effects:
///
///    - memory depth is divided by two (for each removed address bit)
///    - memory width is multiplied by two (for each removed address bit)
///    - for each write port:
///      - if the removed address bit was implicit (as part of a wide port), the port width doesn't change
///      - otherwise, the port width is multiplied by two, and a mux is inserted
///    - for each write port:
///      - if the removed address bit was implicit (as part of a wide port), the port width doesn't change
///      - otherwise, the port width is multiplied by two, and write mask is adjusted with address decoding logic
///
///    This part of the transformation is used to deal with memories that have excessively asymmetric port widths.
///
/// 3. The `wide_log2` of every write port is adjusted up to the factor requested in `write_wide_log2`, by doubling
///    port width and inserting write address decode logic as necessary.  It is an error if the requested factor
///    is smaller than the current one (after applying step #2).
///
///    This is used for funny cases like iCE40 memories, which require using 16-bit write ports to have per-bit
///    write masks.
///
/// 4. The `wide_log2` of every read port is adjusted up to the factor requested in `read_wide_log2`, by doubling
///    port width and inserting muxes as necessary.  It is an error if the requested factor is smaller than the
///    current one (after applying step #2).
///
///    This is used for obscure cases requiring a *minimum* port width asymmetry such as the Xilinx `RAM64X8SW` cell.
///
/// 5. The memory depth is adjusted to be at most `2 ** hard_addr_bits`.  Further, all read and write ports are
///    adjusted to have an address signal that is exactly `hard_addr_bits - port.wide_log2()` bits.
///
///    If the memory has larger depth than this limit, it is split up into `2 ** hard_addr_bits`-deep chunks,
///    the chunks are assembled "side-by-side" (converting depth to width), and appropriate read multiplexers
///    and write address decoders are inserted.
///
/// 6. The memory is cut along its width, into `data_width_unit`-sized chunks, which are returned as individual
///    [`Memory`] structures.  The last chunk is padded with dummy bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySwizzle {
    /// The data swizzle.  In the first step, the data width of the memory is adjusted to the length of this
    /// vector.  Each bit of the new memory width is described by an entry of this vector.  A `Some(idx)` means
    /// that the bit should correspond to the given bit slice in the original memory, while a `None` means
    /// a dummy bit inserted for legalization purposes.
    pub data_swizzle: Vec<Option<usize>>,
    pub soft_addr_bits_mask: u32,
    pub write_wide_log2: Vec<usize>,
    pub read_wide_log2: Vec<usize>,
    pub hard_addr_bits: usize,
    pub data_width_unit: usize,
}

/// An extension trait for `Memory` with assorted memory lowering utility functions.
pub trait MemoryExt {
    /// Returns a new memory, with the same dimensions and write ports as this one, but only the selected
    /// subset of read ports.
    ///
    /// `output` should be the output value of the memory.  This function will slice out the bits corresponding
    /// to the selected read ports, and return them.
    fn extract_read_ports(&self, ports: &[usize], output: &Value) -> (Memory, Value);

    /// Converts a synchronous read port to an asynchronous read port, by effectively extracting a flip-flop.
    /// If transparency relations are involved, soft transparency logic is inserted.
    ///
    /// `output` is the output value of the memory cell.  This function performs a `replace_value` on the bits
    /// corresponding to the converted port, and changes them in-place in the argument to newly-added void nets
    /// that are to be driven by the newly-converted port.  Thus, the original memory cell must be unalived, and
    /// `replace_value` must be called on the new value of `output`, possibly after further lowering.
    ///
    /// If the given read port is already asynchronous, the function will return without doing anything.
    fn unmap_read_dff(&mut self, design: &Design, port_index: usize, output: &mut Value);

    /// Unmaps the following features from a synchronous read port:
    ///
    /// - reset
    /// - clear
    /// - initial value
    /// - if `include_transparency` is true, all `MemoryPortRelation::Transparent` relations
    ///
    /// If none of the above are used on the given port, the function will return without doing anything.
    ///
    /// Note that, if any of the above features is unmapped, all of reset, clear, and initial value can be unmapped
    /// for free (it would actually be more complex to *not* unmap them).  Thus, we do not have granular controls
    /// for them.
    ///
    /// This function must not be called on asynchronous read ports.
    fn unmap_read_init_reset_transparency(
        &mut self,
        design: &Design,
        port_index: usize,
        include_transparency: bool,
        output: &mut Value,
    );

    // TODO: needs a `can_emulate_read_before_write()`

    /// Emulates all read-before-write relations in the memory.
    ///
    /// This function has strict preconditions:
    ///
    /// - all read ports must be synchronous and have the same clock
    /// - a single write port cannot have both read-before-write and transparent relations
    ///
    /// If there are no read-before-write relations in the memory, this function will return without doing anything.
    ///
    /// The read-before-write relations are emulated by delaying all write ports with read-before-write relations
    /// by half a cycle:  the clock polarity of the relevant write ports is flipped, and flip-flops (clocked by
    /// the original clock) are added on the data, mask, and address inputs.
    fn emulate_read_before_write(&mut self, design: &Design);

    /// Constructs a `data_swizzle` for the [`MemorySwizzle`] structure, ensuring that the resulting memory
    /// will satisfy mask granularity constraints for all write ports.  The argument must contain one number
    /// for each write port, specifying its mask granularity in bits.  For each write port, the result will
    /// satisfy the following constraints:
    ///
    /// - the result's length will be divisible by the specified granularity
    /// - every aligned group of `granularity` bits will consist only of bits that share the same `mask` net
    ///   in the write port; if the write port is wide, this rule applies individually to all of its subports
    fn make_data_swizzle(&self, write_port_granularity: &[usize]) -> Vec<Option<usize>>;

    /// Returns the depths of the memories that would be returned by `swizzle`, for cost estimation purposes.
    /// Each entry in the result vector corresponds to one memory.
    ///
    /// For the simple case of the target mapping every memory to a single target cell, the length
    /// of the returned vector is equal to the number of target cells used.  For more advanced mappings
    /// where the target can lower a memory to multiple target cells (e.g. where hardware cascade or chip
    /// selects are involved), the target can look at the exact memory depths required and perform its own
    /// computations.
    fn swizzle_depths(&self, swizzle: &MemorySwizzle) -> Vec<usize>;

    /// Returns the number of soft-decoded address bits for each write port and read port for a given `swizzle`.
    ///
    /// Assumes that the swizzle applies to a memory where `extract_read_ports` has been called first with
    /// the given `read_ports` argument.
    ///
    /// The first returned vector is the number of soft-decoded address bits for each write port, and the second
    /// is the number of soft-decoded address bits for each read port included in `read_ports`.
    fn swizzle_mux_bits(&self, read_ports: &[usize], swizzle: &MemorySwizzle) -> (Vec<usize>, Vec<usize>);

    /// Performs a [`MemorySwizzle`] transformation on a memory, and returns a list of new memories with
    /// the corresponding outputs that need to be driven.  The memories should be further lowered by the target.
    fn swizzle(self, design: &Design, output: &Value, swizzle: &MemorySwizzle) -> Vec<(Memory, Value)>;

    /// Returns true iff the memory is legal for fallback lowering.
    ///
    /// A memory is legal for fallback lowering iff all write ports share the same clock.
    fn can_lower_fallback(&self) -> bool;

    /// Performs fallback lowering of the memory.
    ///
    /// If the memory has write ports, a flip-flop is created for every bit of the memory, with associated
    /// write address decoding logic and read muxes.  It is thus *very* inefficient unless the memory depth
    /// is *very* small.
    ///
    /// If the memory has no write ports, it is lowered to just a tree of muxes for every read port, with
    /// constants at the leafs.  On FPGA targets, this will be lowered to a fairly simple tree of LUTs,
    /// which can be reasonably efficient for memories of small depth.
    fn lower_fallback(self, design: &Design, output: &Value);
}

// creates a transparency emulation mux, to be used by both sync-to-async conversion and transparency emulation.
// the data and mask given must be the same width as the read port.  the result is new data and mask:
// if the write port is writing to the same address as the read port is reading, the write data will be
// multiplexed onto the newly-returned data according to the write mask, and the new mask will be an OR of
// the input mask and the write mask.  otherwise, the returned data and mask are the same as the input.
// the mask can be None if the caller doesn't need to track it.
fn transparency_mux(
    memory: &Memory,
    design: &Design,
    read_port_index: usize,
    write_port_index: usize,
    data: Value,
    mask: Option<Value>,
) -> (Value, Option<Value>) {
    let read_port = &memory.read_ports[read_port_index];
    let write_port = &memory.write_ports[write_port_index];

    // adjust write data/mask to read port width
    let write_wide_log2 = write_port.wide_log2(memory);
    let read_wide_log2 = read_port.wide_log2(memory);
    let (write_addr, read_addr, write_data, write_mask) = match write_wide_log2.cmp(&read_wide_log2) {
        std::cmp::Ordering::Less => {
            // read wider than write: demux write data/mask using lower write addr bits
            let wide_log2_diff = read_wide_log2 - write_wide_log2;
            let select = write_port.addr.slice(..wide_log2_diff);
            let write_addr = write_port.addr.slice(wide_log2_diff..);
            let write_data =
                design.add_shl(write_port.data.zext(read_port.data_len), &select, write_port.data.len() as u32);
            let write_mask =
                design.add_shl(write_port.mask.zext(read_port.data_len), &select, write_port.mask.len() as u32);
            (write_addr, read_port.addr.clone(), write_data, write_mask)
        }
        std::cmp::Ordering::Equal => {
            (write_port.addr.clone(), read_port.addr.clone(), write_port.data.clone(), write_port.mask.clone())
        }
        std::cmp::Ordering::Greater => {
            // write wider than read: select write data/mask slices from wrport using lower read addr bits
            let wide_log2_diff = write_wide_log2 - read_wide_log2;
            let select = read_port.addr.slice(..wide_log2_diff);
            let read_addr = read_port.addr.slice(wide_log2_diff..);
            let write_data =
                design.add_ushr(&write_port.data, &select, read_port.data_len as u32).slice(..read_port.data_len);
            let write_mask =
                design.add_ushr(&write_port.mask, &select, read_port.data_len as u32).slice(..read_port.data_len);
            (write_port.addr.clone(), read_addr, write_data, write_mask)
        }
    };

    // compare the relevant address bits
    let max_addr_width = std::cmp::max(read_addr.len(), write_addr.len());
    let addr_eq = design.add_eq(read_addr.zext(max_addr_width), write_addr.zext(max_addr_width));

    // perform actual muxing
    let sel_write = design.add_dedup_mux(addr_eq, &write_mask, Value::zero(write_mask.len()));
    let new_data = design.add_bitwise_mux(sel_write, write_data, data);
    let new_mask = mask.map(|mask| design.add_dedup_mux(addr_eq, design.add_or(&mask, write_mask), mask));

    (new_data, new_mask)
}

impl MemoryExt for Memory {
    fn extract_read_ports(&self, ports: &[usize], output: &Value) -> (Memory, Value) {
        let mut new_memory = Memory {
            depth: self.depth,
            width: self.width,
            init_value: self.init_value.clone(),
            write_ports: self.write_ports.clone(),
            read_ports: vec![],
        };
        let mut new_output = Value::new();
        for &port_index in ports {
            new_memory.read_ports.push(self.read_ports[port_index].clone());
            let output_slice = self.read_port_output_slice(port_index);
            new_output.extend(output.slice(output_slice));
        }
        (new_memory, new_output)
    }

    fn unmap_read_dff(&mut self, design: &Design, port_index: usize, output: &mut Value) {
        let read_port = &mut self.read_ports[port_index];
        let Some(port_flip_flop) = read_port.flip_flop.take() else {
            return;
        };
        let new_port_output = design.add_void(read_port.data_len);
        let mut data = new_port_output.clone();
        for (write_port_index, relation) in port_flip_flop.relations.into_iter().enumerate() {
            if relation == MemoryPortRelation::Transparent {
                (data, _) = transparency_mux(self, design, port_index, write_port_index, data, None);
            }
        }
        let q = design.add_dff(FlipFlop {
            data,
            clock: port_flip_flop.clock,
            clear: port_flip_flop.clear.into(),
            load: port_flip_flop.load,
            reset: port_flip_flop.reset,
            enable: port_flip_flop.enable,
            reset_over_enable: port_flip_flop.reset_over_enable,
            clear_value: port_flip_flop.clear_value,
            load_data: port_flip_flop.load_data,
            reset_value: port_flip_flop.reset_value,
            init_value: port_flip_flop.init_value,
        });
        let output_slice = self.read_port_output_slice(port_index);
        design.replace_value(output.slice(output_slice.clone()), q);
        output[output_slice.clone()].copy_from_slice(&new_port_output[..]);
    }

    fn unmap_read_init_reset_transparency(
        &mut self,
        design: &Design,
        port_index: usize,
        include_transparency: bool,
        output: &mut Value,
    ) {
        let read_port = &self.read_ports[port_index];
        let port_flip_flop = read_port.flip_flop.as_ref().unwrap();

        let has_init = port_flip_flop.has_init_value();
        let has_reset = port_flip_flop.has_reset();
        let has_clear = port_flip_flop.has_clear();

        // create transparency data and mask values. data is the value that should override the read data,
        // and mask is the validity mask corresponding to it.
        // if not unmapping transparency, just set them to (X, 0).
        let mut data = Value::undef(read_port.data_len);
        let mut mask = Some(Value::zero(read_port.data_len));
        let mut has_transparency = false;
        if include_transparency {
            for (write_port_index, &relation) in port_flip_flop.relations.iter().enumerate() {
                if relation == MemoryPortRelation::Transparent {
                    (data, mask) = transparency_mux(self, design, port_index, write_port_index, data, mask);
                    has_transparency = true;
                }
            }
        }
        let mask = mask.unwrap();

        if !has_init && !has_reset && !has_clear && !has_transparency {
            return;
        }

        // now delay the above by one cycle, and also mix in the unmapped clear/reset/init into it.
        let read_port = &mut self.read_ports[port_index];
        let port_flip_flop = read_port.flip_flop.as_mut().unwrap();
        let data = design.add_dff(FlipFlop {
            data,
            clock: port_flip_flop.clock,
            clear: port_flip_flop.clear.into(),
            load: port_flip_flop.load,
            reset: port_flip_flop.reset,
            enable: port_flip_flop.enable,
            reset_over_enable: port_flip_flop.reset_over_enable,
            load_data: std::mem::replace(&mut port_flip_flop.load_data, Value::undef(read_port.data_len)),
            clear_value: std::mem::replace(&mut port_flip_flop.clear_value, Const::undef(read_port.data_len)),
            reset_value: std::mem::replace(&mut port_flip_flop.reset_value, Const::undef(read_port.data_len)),
            init_value: std::mem::replace(&mut port_flip_flop.init_value, Const::undef(read_port.data_len)),
        });
        let mask = design.add_dedup_dff(FlipFlop {
            data: mask,
            clock: port_flip_flop.clock,
            clear: port_flip_flop.clear.into(),
            load: port_flip_flop.load,
            reset: port_flip_flop.reset,
            enable: port_flip_flop.enable,
            reset_over_enable: port_flip_flop.reset_over_enable,
            load_data: Value::zero(read_port.data_len),
            clear_value: Const::ones(read_port.data_len),
            reset_value: Const::ones(read_port.data_len),
            init_value: if has_init { Const::ones(read_port.data_len) } else { Const::undef(read_port.data_len) },
        });
        if include_transparency {
            for relation in &mut port_flip_flop.relations {
                if *relation == MemoryPortRelation::Transparent {
                    *relation = MemoryPortRelation::Undefined;
                }
            }
        }
        port_flip_flop.clear = ControlNet::ZERO;
        port_flip_flop.reset = ControlNet::ZERO;

        // perform the actual muxing.
        let new_port_output = design.add_void(read_port.data_len);
        let mux = design.add_bitwise_mux(mask, data, &new_port_output);

        // and do the replacement.
        let output_slice = self.read_port_output_slice(port_index);
        design.replace_value(output.slice(output_slice.clone()), mux);
        output[output_slice.clone()].copy_from_slice(&new_port_output[..]);
    }

    fn emulate_read_before_write(&mut self, design: &Design) {
        let mut rdfirst_write_ports = vec![false; self.write_ports.len()];
        let mut transparent_write_ports = vec![false; self.write_ports.len()];

        for port in &self.read_ports {
            let port_flip_flop = port.flip_flop.as_ref().unwrap();
            assert_eq!(
                port_flip_flop.clock,
                self.read_ports[0].flip_flop.as_ref().unwrap().clock,
                "all read ports must have the same clock"
            );
            for (write_port_index, &relation) in port_flip_flop.relations.iter().enumerate() {
                match relation {
                    MemoryPortRelation::Undefined => (),
                    MemoryPortRelation::ReadBeforeWrite => rdfirst_write_ports[write_port_index] = true,
                    MemoryPortRelation::Transparent => transparent_write_ports[write_port_index] = true,
                }
            }
        }

        let init_undef = self.init_value.is_undef();

        for (port_index, port) in self.write_ports.iter_mut().enumerate() {
            if !rdfirst_write_ports[port_index] {
                continue;
            }
            assert!(
                !transparent_write_ports[port_index],
                "a single write port cannot have both a read-before-write and a transparent relation to two read ports"
            );
            port.addr = design.add_dff(FlipFlop::new(std::mem::take(&mut port.addr), port.clock));
            port.data = design.add_dff(FlipFlop::new(std::mem::take(&mut port.data), port.clock));
            port.mask = design.add_dedup_dff(
                FlipFlop::new(std::mem::take(&mut port.mask), port.clock).with_init(if init_undef {
                    Const::undef(port.data.len())
                } else {
                    Const::zero(port.data.len())
                }),
            );
            port.clock = !port.clock;
            for read_port in &mut self.read_ports {
                let read_ff = read_port.flip_flop.as_mut().unwrap();
                read_ff.relations[port_index] = MemoryPortRelation::Undefined;
            }
        }
    }

    fn make_data_swizzle(&self, write_port_granularity: &[usize]) -> Vec<Option<usize>> {
        assert_eq!(self.write_ports.len(), write_port_granularity.len());
        let mask_slices = Vec::from_iter((0..self.width).map(|bit_index| {
            Vec::from_iter(self.write_ports.iter().map(|port| {
                Value::from_iter(
                    (0..(port.data.len() / self.width))
                        .map(|subport_index| port.mask[subport_index * self.width + bit_index]),
                )
            }))
        }));
        let mut result = vec![];
        for bit_index in 0..self.width {
            'check_slices: loop {
                for (port_index, &port_granularity) in write_port_granularity.iter().enumerate() {
                    if result.len() % port_granularity != 0 {
                        if mask_slices[bit_index][port_index] != mask_slices[bit_index - 1][port_index] {
                            result.push(None);
                            continue 'check_slices;
                        }
                    }
                }
                break;
            }
            result.push(Some(bit_index));
        }
        'align: loop {
            for &port_granularity in write_port_granularity {
                if result.len() % port_granularity != 0 {
                    result.push(None);
                    continue 'align;
                }
            }
            break;
        }
        result
    }

    fn swizzle_depths(&self, swizzle: &MemorySwizzle) -> Vec<usize> {
        // compute how many low soft address bits there are
        let soft_addr_bits = swizzle.soft_addr_bits_mask.count_ones() as usize;
        // compute the dimensions of memory after swizzle steps 1-2 applied
        let base_width = swizzle.data_swizzle.len() << soft_addr_bits;
        let base_depth = self.depth >> soft_addr_bits;
        // compute how many chunks created in step 5 require the full `2 ** hard_addr_bits` depth
        let num_chunks_full = base_depth >> swizzle.hard_addr_bits;
        let depth_full = 1 << swizzle.hard_addr_bits;
        // compute the depth of the last, partial chunk created in step 5
        let depth_partial = base_depth & ((1 << swizzle.hard_addr_bits) - 1);
        // determine how much of the width of memory created in step 5 requires the full depth
        let width_full = base_width * num_chunks_full;
        // and determine its total width
        let width_total = if depth_partial != 0 { width_full + base_width } else { width_full };
        // now align up both of these numbers to `data_width_unit` chunks
        let num_mems_full = width_full.div_ceil(swizzle.data_width_unit);
        let num_mems_total = width_total.div_ceil(swizzle.data_width_unit);
        // and compute the final result
        let mut result = vec![depth_full; num_mems_full];
        while result.len() < num_mems_total {
            result.push(depth_partial);
        }
        result
    }

    fn swizzle_mux_bits(&self, read_ports: &[usize], swizzle: &MemorySwizzle) -> (Vec<usize>, Vec<usize>) {
        let soft_addr_bits = swizzle.soft_addr_bits_mask.count_ones() as usize;
        let chunks = self.depth.div_ceil(1 << (soft_addr_bits + swizzle.hard_addr_bits));
        let chunks_mux_bits = if chunks <= 1 { 0 } else { (chunks - 1).ilog2() as usize + 1 };
        let write_mux_bits = Vec::from_iter(self.write_ports.iter().zip(swizzle.write_wide_log2.iter()).map(
            |(port, &new_wide_log2)| {
                let old_wide_log2 = port.wide_log2(self);
                chunks_mux_bits + soft_addr_bits + new_wide_log2 - old_wide_log2
            },
        ));
        let read_mux_bits = Vec::from_iter(read_ports.iter().zip(swizzle.read_wide_log2.iter()).map(
            |(&port_index, &new_wide_log2)| {
                let port = &self.read_ports[port_index];
                let old_wide_log2 = port.wide_log2(self);
                chunks_mux_bits + soft_addr_bits + new_wide_log2 - old_wide_log2
            },
        ));
        (write_mux_bits, read_mux_bits)
    }

    fn swizzle(mut self, design: &Design, output: &Value, swizzle: &MemorySwizzle) -> Vec<(Memory, Value)> {
        // 0-width memories can have no outputs, and can just be unalived without remorse.
        if self.width == 0 {
            return vec![];
        }
        // 0-depth memories are annoying to deal with — get them out of the way.
        if self.depth == 0 {
            // while you cannot really read any row, the read ports can still have well-defined values
            // because of initial values and resets.  deal with them by converting them to async ones.
            let mut output = output.clone();
            for port_index in 0..self.read_ports.len() {
                self.unmap_read_dff(design, port_index, &mut output);
            }
            // now we have only asynchronous read ports, and can replace all their outputs with undef.
            design.replace_value(&output, Value::undef(output.len()));
            return vec![];
        }

        // first, replicate the calculations from `swizzle_size`
        let soft_addr_bits = swizzle.soft_addr_bits_mask.count_ones() as usize;
        let base_width = swizzle.data_swizzle.len() << soft_addr_bits;
        let base_depth = self.depth >> soft_addr_bits;
        let depth_full = 1 << swizzle.hard_addr_bits;
        let depth_partial = base_depth & ((1 << swizzle.hard_addr_bits) - 1);
        let num_chunks_full = base_depth >> swizzle.hard_addr_bits;
        let num_chunks_total = if depth_partial == 0 { num_chunks_full } else { num_chunks_full + 1 };
        let width_full = base_width * num_chunks_full;
        let width_total = base_width * num_chunks_total;
        let num_mems_full = width_full.div_ceil(swizzle.data_width_unit);
        let num_mems_total = width_total.div_ceil(swizzle.data_width_unit);
        let width_total_aligned = num_mems_total * swizzle.data_width_unit;

        // performs steps 2-5 of the transformation on a port's address; the first value returned is
        // the new address for the port (a subset of the original address bits, potentially padded with 0 at the top),
        // while the second value is the remaining address bits (known as mux addr), which should be used to form
        // a mux / write address decoder.
        // the third value describes the swizzle for the data bits: it is indexed first by the mux_addr value, second by
        // the original port's subport index. the value is (new subport index, chunk index within the new port).
        fn swizzle_addr(
            addr: &Value,
            swizzle: &MemorySwizzle,
            num_chunks_total: usize,
            orig_wide_log2: usize,
            new_wide_log2: usize,
        ) -> (Value, Value, Vec<Vec<(usize, usize)>>) {
            #[derive(Clone, Copy)]
            enum BitDisposition {
                NewSubportBit(usize),
                ChunkIndexBit(usize),
            }

            let mut num_chunks_port = num_chunks_total;
            let mut hw_addr = Value::new();
            let mut mux_addr = Value::new();
            let mut old_subport_bit_disposition = vec![];
            let mut mux_addr_bit_disposition = vec![];
            let mut new_subport_bit = 0;
            let mut new_chunk_bit = 0;
            for addr_bit_index in 0..orig_wide_log2 {
                if ((swizzle.soft_addr_bits_mask >> addr_bit_index) & 1) != 0 {
                    old_subport_bit_disposition.push(BitDisposition::ChunkIndexBit(new_chunk_bit));
                    new_chunk_bit += 1;
                } else {
                    old_subport_bit_disposition.push(BitDisposition::NewSubportBit(new_subport_bit));
                    new_subport_bit += 1;
                }
            }
            assert!(new_subport_bit <= new_wide_log2);
            for (addr_bit_index, addr_bit) in (orig_wide_log2..).zip(addr) {
                if ((swizzle.soft_addr_bits_mask >> addr_bit_index) & 1) != 0 {
                    mux_addr_bit_disposition.push(BitDisposition::ChunkIndexBit(new_chunk_bit));
                    new_chunk_bit += 1;
                    mux_addr.push(addr_bit);
                    num_chunks_port *= 2;
                } else if new_subport_bit != new_wide_log2 {
                    mux_addr_bit_disposition.push(BitDisposition::NewSubportBit(new_subport_bit));
                    new_subport_bit += 1;
                    mux_addr.push(addr_bit);
                    num_chunks_port *= 2;
                } else if hw_addr.len() != swizzle.hard_addr_bits - new_wide_log2 {
                    hw_addr.push(addr_bit);
                } else {
                    mux_addr_bit_disposition.push(BitDisposition::ChunkIndexBit(new_chunk_bit));
                    new_chunk_bit += 1;
                    mux_addr.push(addr_bit);
                }
            }
            assert!(hw_addr.len() <= swizzle.hard_addr_bits - new_wide_log2);
            while hw_addr.len() < swizzle.hard_addr_bits - new_wide_log2 {
                hw_addr.push(Net::ZERO);
            }
            let useful_mux_bits = if num_chunks_port < 2 { 0 } else { (num_chunks_port - 1).ilog2() as usize + 1 };
            while mux_addr.len() < useful_mux_bits {
                mux_addr_bit_disposition.push(BitDisposition::ChunkIndexBit(new_chunk_bit));
                new_chunk_bit += 1;
                mux_addr.push(Net::ZERO);
            }

            let mut data_swizzle = Vec::from_iter(std::iter::repeat_n(vec![], num_chunks_port));
            for port_chunk in 0..num_chunks_port {
                for orig_subport in 0..(1 << orig_wide_log2) {
                    let mut new_subport = 0;
                    let mut new_chunk = 0;
                    for (bit, disposition) in old_subport_bit_disposition.iter().copied().enumerate() {
                        let bit_val = (orig_subport >> bit) & 1;
                        match disposition {
                            BitDisposition::NewSubportBit(new_bit) => new_subport |= bit_val << new_bit,
                            BitDisposition::ChunkIndexBit(new_bit) => new_chunk |= bit_val << new_bit,
                        }
                    }
                    for (bit, disposition) in mux_addr_bit_disposition.iter().copied().enumerate() {
                        if bit >= useful_mux_bits {
                            continue;
                        }
                        let bit_val = (port_chunk >> bit) & 1;
                        match disposition {
                            BitDisposition::NewSubportBit(new_bit) => new_subport |= bit_val << new_bit,
                            BitDisposition::ChunkIndexBit(new_bit) => new_chunk |= bit_val << new_bit,
                        }
                    }

                    data_swizzle[port_chunk].push((new_subport, new_chunk));
                }
            }

            (hw_addr, mux_addr, data_swizzle)
        }

        // perform steps 1-5 of the transformation on the write ports; this is done in a somewhat weird way:
        // each write port is split into `1 << wide_log2` chunks, which are later reassembled
        // when emitting the final memories.  this is done to make the slicing easier.
        let mut write_ports = vec![];
        for (port_index, orig_port) in self.write_ports.iter().enumerate() {
            let orig_wide_log2 = orig_port.wide_log2(&self);
            let new_wide_log2 = swizzle.write_wide_log2[port_index];
            let (new_addr, mux_addr, data_swizzle) =
                swizzle_addr(&orig_port.addr, swizzle, num_chunks_total, orig_wide_log2, new_wide_log2);
            let mut subports = vec![];
            for _ in 0..(1 << new_wide_log2) {
                subports.push(MemoryWritePort {
                    addr: new_addr.clone(),
                    data: Value::undef(width_total_aligned),
                    mask: Value::zero(width_total_aligned),
                    clock: orig_port.clock,
                });
            }
            for (mux_index, mux_index_data_swizzle) in data_swizzle.into_iter().enumerate() {
                let mux_match = design.add_eq(&mux_addr, Const::from_uint(mux_index as u128, mux_addr.len()));
                let mut new_data = Value::new();
                let mut new_mask = Value::new();
                // when creating the write mask, whenever a dummy bit needs to be inserted, make it a duplicate
                // of the previous bit instead of an X.  this ensures that any mask granularity constraints
                // are met (such constraints are generally the reason why dummy bits are inserted into the swizzle
                // anyway).
                for orig_subport_index in 0..(1 << orig_wide_log2) {
                    for &source in &swizzle.data_swizzle {
                        match source {
                            Some(index) => {
                                new_data.push(orig_port.data[orig_subport_index * self.width + index]);
                                new_mask.push(orig_port.mask[orig_subport_index * self.width + index]);
                            }
                            None => {
                                new_data.push(Net::UNDEF);
                                if new_mask.is_empty() {
                                    new_mask.push(Net::UNDEF);
                                } else {
                                    new_mask.push(new_mask.msb());
                                }
                            }
                        }
                    }
                }
                if !mux_addr.is_empty() {
                    new_mask = design.add_dedup_mux(
                        mux_match,
                        new_mask,
                        Value::zero(swizzle.data_swizzle.len() << orig_wide_log2),
                    );
                }
                for (orig_subport_index, (new_subport_index, new_chunk_index)) in
                    mux_index_data_swizzle.into_iter().enumerate()
                {
                    let new_chunk_slice = new_chunk_index * swizzle.data_swizzle.len()
                        ..(new_chunk_index + 1) * swizzle.data_swizzle.len();
                    let orig_subport_slice = orig_subport_index * swizzle.data_swizzle.len()
                        ..(orig_subport_index + 1) * swizzle.data_swizzle.len();
                    subports[new_subport_index].data[new_chunk_slice.clone()]
                        .copy_from_slice(&new_data[orig_subport_slice.clone()]);
                    subports[new_subport_index].mask[new_chunk_slice].copy_from_slice(&new_mask[orig_subport_slice]);
                }
            }
            write_ports.push(subports);
        }

        // likewise, perform steps 1-5 of the transformation on the read ports, also in a chunky manner.
        let mut read_ports = vec![];
        for (port_index, orig_port) in self.read_ports.iter().enumerate() {
            let orig_wide_log2 = orig_port.wide_log2(&self);
            let new_wide_log2 = swizzle.read_wide_log2[port_index];
            let (new_addr, mut mux_addr, data_swizzle) =
                swizzle_addr(&orig_port.addr, swizzle, num_chunks_total, orig_wide_log2, new_wide_log2);
            let mut subports = vec![];
            for _ in 0..(1 << new_wide_log2) {
                subports.push((
                    MemoryReadPort {
                        addr: new_addr.clone(),
                        data_len: width_total_aligned,
                        flip_flop: orig_port.flip_flop.as_ref().map(|flip_flop| MemoryReadFlipFlop {
                            clock: flip_flop.clock,
                            clear: flip_flop.clear,
                            load: flip_flop.load,
                            reset: flip_flop.reset,
                            enable: flip_flop.enable,
                            reset_over_enable: flip_flop.reset_over_enable,
                            load_data: Value::undef(width_total_aligned),
                            clear_value: Const::undef(width_total_aligned),
                            reset_value: Const::undef(width_total_aligned),
                            init_value: Const::undef(width_total_aligned),
                            relations: flip_flop.relations.clone(),
                        }),
                    },
                    design.add_void(width_total_aligned),
                ));
            }
            if let Some(ref flip_flop) = orig_port.flip_flop {
                mux_addr = design.add_dff(FlipFlop::new(mux_addr, flip_flop.clock).with_enable(flip_flop.enable));
            }
            let mut output_chunks = vec![];
            for mux_index_data_swizzle in data_swizzle {
                let mut output_chunk = Value::undef(orig_port.data_len);
                for (orig_subport_index, (new_subport_index, new_chunk_index)) in
                    mux_index_data_swizzle.into_iter().enumerate()
                {
                    let new_chunk_slice = new_chunk_index * swizzle.data_swizzle.len()
                        ..(new_chunk_index + 1) * swizzle.data_swizzle.len();
                    if let Some(ref flip_flop) = orig_port.flip_flop {
                        let swizzle_const = |value: &Const| {
                            Const::from_iter(swizzle.data_swizzle.iter().map(|&source| match source {
                                Some(index) => value[orig_subport_index * self.width + index],
                                None => Trit::Undef,
                            }))
                        };
                        let new_flip_flop = subports[new_subport_index].0.flip_flop.as_mut().unwrap();
                        new_flip_flop.clear_value[new_chunk_slice.clone()]
                            .copy_from_slice(&swizzle_const(&flip_flop.clear_value)[..]);
                        new_flip_flop.reset_value[new_chunk_slice.clone()]
                            .copy_from_slice(&swizzle_const(&flip_flop.reset_value)[..]);
                        new_flip_flop.init_value[new_chunk_slice.clone()]
                            .copy_from_slice(&swizzle_const(&flip_flop.init_value)[..]);
                    }
                    let swizzled_output_chunk = subports[new_subport_index].1.slice(new_chunk_slice);
                    for (&index, swizzled_output_bit) in swizzle.data_swizzle.iter().zip(swizzled_output_chunk) {
                        if let Some(index) = index {
                            output_chunk[orig_subport_index * self.width + index] = swizzled_output_bit;
                        }
                    }
                }
                output_chunks.push(output_chunk);
            }
            // create the mux tree bottom-up, halving the size of the output_chunks array each time
            for addr_bit in &mux_addr {
                output_chunks = Vec::from_iter(output_chunks.chunks(2).map(|pair| {
                    if pair.len() == 2 {
                        design.add_mux(addr_bit, &pair[1], &pair[0])
                    } else {
                        // we may be reading out-of-bounds. could also use a mux-with-undef here, but there's little point.
                        pair[0].clone()
                    }
                }))
            }
            let [port_output] = output_chunks.try_into().unwrap();
            design.replace_value(output.slice(self.read_port_output_slice(port_index)), port_output);
            read_ports.push(subports);
        }

        // prepare the memories, applying step 6 (not including initial values)
        let mut result = vec![];
        for index in 0..num_mems_total {
            let data_slice = index * swizzle.data_width_unit..(index + 1) * swizzle.data_width_unit;
            let depth = if index < num_mems_full { depth_full } else { depth_partial };
            let mut memory = Memory {
                depth,
                width: swizzle.data_width_unit,
                init_value: Const::undef(depth * swizzle.data_width_unit),
                write_ports: vec![],
                read_ports: vec![],
            };
            for write_subports in &write_ports {
                let mut port = MemoryWritePort {
                    addr: write_subports[0].addr.clone(),
                    data: Value::new(),
                    mask: Value::new(),
                    clock: write_subports[0].clock,
                };
                for subport in write_subports {
                    port.data.extend(subport.data.slice(data_slice.clone()));
                    port.mask.extend(subport.mask.slice(data_slice.clone()));
                }
                memory.write_ports.push(port);
            }
            let mut new_output = Value::new();
            for read_subports in &read_ports {
                let mut port = MemoryReadPort {
                    addr: read_subports[0].0.addr.clone(),
                    data_len: swizzle.data_width_unit * read_subports.len(),
                    flip_flop: read_subports[0].0.flip_flop.as_ref().map(|flip_flop| MemoryReadFlipFlop {
                        clock: flip_flop.clock,
                        clear: flip_flop.clear,
                        load: flip_flop.load,
                        reset: flip_flop.reset,
                        enable: flip_flop.enable,
                        reset_over_enable: flip_flop.reset_over_enable,
                        // HERE
                        load_data: Value::undef(swizzle.data_width_unit * read_subports.len()),
                        clear_value: Const::new(),
                        reset_value: Const::new(),
                        init_value: Const::new(),
                        relations: flip_flop.relations.clone(),
                    }),
                };
                for (subport, subport_output) in read_subports {
                    if let Some(ref subport_flip_flop) = subport.flip_flop {
                        let port_flip_flop = port.flip_flop.as_mut().unwrap();
                        port_flip_flop.clear_value.extend(subport_flip_flop.clear_value.slice(data_slice.clone()));
                        port_flip_flop.reset_value.extend(subport_flip_flop.reset_value.slice(data_slice.clone()));
                        port_flip_flop.init_value.extend(subport_flip_flop.init_value.slice(data_slice.clone()));
                    }
                    new_output.extend(subport_output.slice(data_slice.clone()));
                }
                memory.read_ports.push(port);
            }
            result.push((memory, new_output));
        }

        // fill initial values.
        for row in 0..self.depth {
            let orig_value = self.init_value.slice(row * self.width..(row + 1) * self.width);
            // apply step 1
            let swizzled_value = Const::from_iter(swizzle.data_swizzle.iter().map(|&index| match index {
                None => Trit::Undef,
                Some(index) => orig_value[index],
            }));
            // apply steps 2 and 5 to the row index
            let mut new_row = 0;
            let mut new_row_bit = 0;
            let mut chunk_index = 0;
            let mut chunk_index_bit = 0;
            assert!(row < (1 << 32));
            for bit in 0..32 {
                let bit_value = (row >> bit) & 1;
                if ((swizzle.soft_addr_bits_mask >> bit) & 1) != 0 || new_row_bit == swizzle.hard_addr_bits {
                    chunk_index |= bit_value << chunk_index_bit;
                    chunk_index_bit += 1;
                } else {
                    new_row |= bit_value << new_row_bit;
                    new_row_bit += 1;
                }
            }
            assert_eq!(new_row_bit, swizzle.hard_addr_bits);
            // apply step 6
            for (index, bit) in swizzled_value.iter().enumerate() {
                let full_index = chunk_index * swizzle.data_swizzle.len() + index;
                let mem_index = full_index / swizzle.data_width_unit;
                let mem_bit_index = full_index % swizzle.data_width_unit;
                result[mem_index].0.init_value[new_row * swizzle.data_width_unit + mem_bit_index] = bit;
            }
        }

        result
    }

    fn can_lower_fallback(&self) -> bool {
        for port in &self.write_ports {
            if port.clock != self.write_ports[0].clock {
                return false;
            }
        }
        true
    }

    fn lower_fallback(mut self, design: &Design, output: &Value) {
        let mut write_clock = None;
        for write_port in &self.write_ports {
            if write_clock.is_none() {
                write_clock = Some(write_port.clock);
            } else if write_clock != Some(write_port.clock) {
                panic!("trying to lower memory with multiple write clocks");
            }
        }
        let mut output = output.clone();
        // convert all read ports to async first.
        for port_index in 0..self.read_ports.len() {
            self.unmap_read_dff(design, port_index, &mut output);
        }
        let max_wide_log2 = self
            .write_ports
            .iter()
            .map(|port| port.wide_log2(&self))
            .max()
            .unwrap_or(0)
            .max(self.read_ports.iter().map(|port| port.wide_log2(&self)).max().unwrap_or(0));
        // create a swizzle structure.  this will effectively split the memory into one sub-memory per row.
        let swizzle = MemorySwizzle {
            data_swizzle: Vec::from_iter((0..self.width).map(Some)),
            soft_addr_bits_mask: (1 << max_wide_log2) - 1,
            write_wide_log2: vec![0; self.write_ports.len()],
            read_wide_log2: vec![0; self.read_ports.len()],
            hard_addr_bits: 0,
            data_width_unit: self.width,
        };
        for (memory, mem_output) in self.swizzle(design, &output, &swizzle) {
            assert_eq!(memory.depth, 1);
            let data = if memory.write_ports.is_empty() {
                Value::from(memory.init_value)
            } else {
                let q = design.add_void(memory.width);
                let mut data = q.clone();
                for port in memory.write_ports {
                    assert_eq!(port.data.len(), data.len());
                    // multiplex our data onto the chain
                    data = design.add_bitwise_mux(port.mask, port.data, data);
                }
                design.replace_value(
                    &q,
                    design.add_dff(FlipFlop::new(data, write_clock.unwrap()).with_init(memory.init_value)),
                );
                q
            };
            // all read ports are asynchronous, and there is only a single row in the memory.
            // just wire it straight to all read data outputs.
            for (port_index, port) in memory.read_ports.into_iter().enumerate() {
                design
                    .replace_value(mem_output.slice(port_index * memory.width..(port_index + 1) * memory.width), &data);
                assert_eq!(port.data_len, data.len());
            }
        }
    }
}

trait DesignExt {
    fn add_dedup_dff(&self, flip_flop: FlipFlop) -> Value;
    fn add_dedup_mux(&self, arg1: impl Into<ControlNet>, arg2: impl Into<Value>, arg3: impl Into<Value>) -> Value;
    fn add_bitwise_mux(&self, arg1: impl Into<Value>, arg2: impl Into<Value>, arg3: impl Into<Value>) -> Value;
}

impl DesignExt for Design {
    fn add_dedup_dff(&self, mut flip_flop: FlipFlop) -> Value {
        let mut cache = BTreeMap::new();
        let mut result_swizzle = vec![];
        let mut dedup_data = Value::new();
        let mut dedup_reset_value = Const::new();
        let mut dedup_clear_value = Const::new();
        let mut dedup_init_value = Const::new();

        for (((data_bit, reset_bit), clear_bit), init_bit) in
            flip_flop.data.iter().zip(flip_flop.reset_value).zip(flip_flop.clear_value).zip(flip_flop.init_value)
        {
            match cache.entry((data_bit, reset_bit, clear_bit, init_bit)) {
                btree_map::Entry::Vacant(entry) => {
                    let index = dedup_data.len();
                    dedup_data.push(data_bit);
                    dedup_reset_value.push(reset_bit);
                    dedup_clear_value.push(clear_bit);
                    dedup_init_value.push(init_bit);
                    result_swizzle.push(index);
                    entry.insert(index);
                }
                btree_map::Entry::Occupied(entry) => {
                    result_swizzle.push(*entry.get());
                }
            }
        }
        flip_flop.data = dedup_data;
        flip_flop.reset_value = dedup_reset_value;
        flip_flop.clear_value = dedup_clear_value;
        flip_flop.init_value = dedup_init_value;
        let dedup_result = self.add_dff(flip_flop);
        Value::from_iter(result_swizzle.iter().map(|&index| dedup_result[index]))
    }

    fn add_dedup_mux(&self, arg1: impl Into<ControlNet>, arg2: impl Into<Value>, arg3: impl Into<Value>) -> Value {
        let arg1 = arg1.into();
        let arg2 = arg2.into();
        let arg3 = arg3.into();
        let mut cache = BTreeMap::new();
        let mut result_swizzle = vec![];
        let mut dedup_arg2 = Value::new();
        let mut dedup_arg3 = Value::new();
        for (arg2_bit, arg3_bit) in arg2.iter().zip(arg3) {
            match cache.entry((arg2_bit, arg3_bit)) {
                btree_map::Entry::Vacant(entry) => {
                    let index = dedup_arg2.len();
                    dedup_arg2.push(arg2_bit);
                    dedup_arg3.push(arg3_bit);
                    result_swizzle.push(index);
                    entry.insert(index);
                }
                btree_map::Entry::Occupied(entry) => {
                    result_swizzle.push(*entry.get());
                }
            }
        }
        let dedup_result = self.add_mux(arg1, dedup_arg2, dedup_arg3);
        Value::from_iter(result_swizzle.iter().map(|&index| dedup_result[index]))
    }

    fn add_bitwise_mux(&self, arg1: impl Into<Value>, arg2: impl Into<Value>, arg3: impl Into<Value>) -> Value {
        let arg1 = arg1.into();
        let arg2 = arg2.into();
        let arg3 = arg3.into();
        let mut result = Value::new();
        let mut index = 0;
        assert_eq!(arg1.len(), arg2.len());
        assert_eq!(arg1.len(), arg3.len());
        while index < arg1.len() {
            let mut end_index = index;
            while end_index < arg1.len() && arg1[end_index] == arg1[index] {
                end_index += 1;
            }
            let arg2_chunk = arg2.slice(index..end_index);
            let arg3_chunk = arg3.slice(index..end_index);
            result.extend(self.add_dedup_mux(arg1[index], arg2_chunk, arg3_chunk));
            index = end_index;
        }
        result
    }
}
