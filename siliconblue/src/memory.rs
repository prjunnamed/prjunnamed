use prjunnamed_memory::{MemoryExt, MemorySwizzle};
use prjunnamed_netlist::{
    Cell, Const, ControlNet, Design, Memory, MemoryPortRelation, Net, Target, TargetCell, Trit, Value,
};

use crate::SiliconBlueTarget;

#[derive(Clone, Debug)]
struct BramLowering {
    read_port_swizzles: Vec<MemorySwizzle>,
}

const OPT_FOR_AREA: bool = false;

impl SiliconBlueTarget {
    fn find_bram_lowering(&self, memory: &Memory) -> Option<BramLowering> {
        // A memory can be lowered to SiliconBlue BRAM iff it satisfies two conditions:
        //
        // - at most one write port
        // - all read ports are synchronous
        if memory.write_ports.len() > 1 {
            return None;
        }
        if memory.read_ports.iter().any(|port| port.flip_flop.is_none()) {
            return None;
        }
        let write_port = memory.write_ports.first();
        let mut read_port_swizzles = vec![];
        for (read_port_index, read_port) in memory.read_ports.iter().enumerate() {
            let mut cand_swizzles = vec![];
            for base_mode in 0..3 {
                if base_mode != 0 && !self.is_ice40() {
                    continue;
                }
                let mut soft_addr_bits_mask = 0;
                let mut write_wide_log2 = vec![];
                let data_swizzle = if let Some(write_port) = write_port {
                    let wide_log2 = write_port.wide_log2(memory);
                    if wide_log2 > base_mode {
                        for bit in base_mode..wide_log2 {
                            soft_addr_bits_mask |= 1 << bit;
                        }
                    }
                    let actual_wide_log2 = wide_log2.min(base_mode);
                    write_wide_log2.push(actual_wide_log2);
                    memory.make_data_swizzle(&[if base_mode == actual_wide_log2 {
                        1
                    } else {
                        16 >> base_mode << actual_wide_log2
                    }])
                } else {
                    memory.make_data_swizzle(&[])
                };
                let read_wide_log2 = read_port.wide_log2(memory);
                if read_wide_log2 > base_mode {
                    for bit in base_mode..read_wide_log2 {
                        soft_addr_bits_mask |= 1 << bit;
                    }
                }
                let read_wide_log2 = vec![read_wide_log2.min(base_mode)];
                let mut swizzle = MemorySwizzle {
                    data_swizzle,
                    soft_addr_bits_mask,
                    write_wide_log2,
                    read_wide_log2,
                    hard_addr_bits: 8 + base_mode,
                    data_width_unit: 16 >> base_mode,
                };
                cand_swizzles.push(swizzle.clone());
                if write_port.is_some() && swizzle.write_wide_log2[0] != base_mode {
                    swizzle.write_wide_log2[0] = base_mode;
                    swizzle.data_swizzle = memory.make_data_swizzle(&[1]);
                    cand_swizzles.push(swizzle);
                }
            }
            let best_swizzle = if OPT_FOR_AREA {
                cand_swizzles
                    .into_iter()
                    .min_by_key(|swizzle| {
                        let num_brams = memory.swizzle_depths(swizzle).len();
                        let (write_mux_bits, read_mux_bits) = memory.swizzle_mux_bits(&[read_port_index], swizzle);
                        let write_mux_bits = write_mux_bits.first().copied().unwrap_or(0);
                        let read_mux_bits = read_mux_bits[0];
                        (num_brams, read_mux_bits, write_mux_bits)
                    })
                    .unwrap()
            } else {
                cand_swizzles
                    .into_iter()
                    .min_by_key(|swizzle| {
                        let num_brams = memory.swizzle_depths(swizzle).len();
                        let (write_mux_bits, read_mux_bits) = memory.swizzle_mux_bits(&[read_port_index], swizzle);
                        let write_mux_bits = write_mux_bits.first().copied().unwrap_or(0);
                        let read_mux_bits = read_mux_bits[0];
                        (read_mux_bits, num_brams, write_mux_bits)
                    })
                    .unwrap()
            };
            read_port_swizzles.push(best_swizzle);
        }
        Some(BramLowering { read_port_swizzles })
    }

    fn swizzle_bram_addr(&self, port_width: usize, addr: &Value) -> Value {
        assert!(addr.len() >= 8 && addr.len() <= 11);
        assert_eq!(port_width, 16 >> (addr.len() - 8));
        match addr.len() {
            8 => addr.zext(11),
            9 => addr.slice(1..).concat(addr[0]).zext(11),
            10 => addr.slice(2..).concat(addr[1]).concat(addr[0]).zext(11),
            11 => addr.slice(3..).concat(addr[2]).concat(addr[1]).concat(addr[0]),
            _ => unreachable!(),
        }
    }

    fn lower_single_bram(&self, design: &Design, memory: &Memory, output: &Value) {
        const SWIZZLE16: [usize; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
        let prototype = self.prototype("SB_RAM40_4K").unwrap();
        let mut target_cell = TargetCell::new("SB_RAM40_4K", prototype);

        assert!(memory.write_ports.len() <= 1);
        assert_eq!(memory.read_ports.len(), 1);
        assert!(memory.depth * memory.width <= 0x1000);

        if let Some(write_port) = memory.write_ports.first() {
            prototype.apply_param(&mut target_cell, "IS_WCLK_INVERTED", write_port.clock.is_negative());
            prototype.apply_input(&mut target_cell, "WCLK", write_port.clock.net());
            prototype.apply_input(&mut target_cell, "WE", Net::ONE);
            prototype.apply_input(
                &mut target_cell,
                "WADDR",
                self.swizzle_bram_addr(write_port.data.len(), &write_port.addr),
            );
            prototype.apply_param(&mut target_cell, "WRITE_MODE", (write_port.addr.len() - 8) as i64);
            if write_port.data.len() != 16 {
                prototype.apply_input(&mut target_cell, "WCLKE", write_port.mask[0]);
                for bit in &write_port.mask {
                    if bit != Net::UNDEF {
                        assert_eq!(bit, write_port.mask[0]);
                    }
                }
            } else {
                prototype.apply_input(&mut target_cell, "WCLKE", Net::ONE);
            }
            match write_port.data.len() {
                16 => {
                    prototype.apply_input(
                        &mut target_cell,
                        "WDATA",
                        Value::from_iter(SWIZZLE16.into_iter().map(|index| write_port.data[index])),
                    );
                    prototype.apply_input(
                        &mut target_cell,
                        "MASK",
                        design.add_not(Value::from_iter(SWIZZLE16.into_iter().map(|index| write_port.mask[index]))),
                    );
                }
                8 => {
                    prototype.apply_input(
                        &mut target_cell,
                        "WDATA",
                        Value::from_iter(SWIZZLE16.into_iter().map(|index| {
                            if index < 8 {
                                write_port.data[index]
                            } else {
                                Net::UNDEF
                            }
                        })),
                    );
                }
                4 => {
                    prototype.apply_input(
                        &mut target_cell,
                        "WDATA",
                        Value::from_iter(SWIZZLE16.into_iter().map(|index| {
                            if index >= 8 && index < 12 {
                                write_port.data[index - 8]
                            } else {
                                Net::UNDEF
                            }
                        })),
                    );
                }
                2 => {
                    prototype.apply_input(
                        &mut target_cell,
                        "WDATA",
                        Value::from_iter(SWIZZLE16.into_iter().map(|index| {
                            if index >= 12 && index < 14 {
                                write_port.data[index - 12]
                            } else {
                                Net::UNDEF
                            }
                        })),
                    );
                }
                _ => unreachable!(),
            }
        } else {
            prototype.apply_input(&mut target_cell, "WCLK", Net::ZERO);
        }

        let read_port = &memory.read_ports[0];
        let read_ff = read_port.flip_flop.as_ref().unwrap();
        prototype.apply_param(&mut target_cell, "IS_RCLK_INVERTED", read_ff.clock.is_negative());
        prototype.apply_input(&mut target_cell, "RCLK", read_ff.clock.net());
        assert!(read_ff.enable.is_positive());
        prototype.apply_input(&mut target_cell, "RCLKE", read_ff.enable.net());
        prototype.apply_input(&mut target_cell, "RE", Net::ONE);
        assert!(!read_ff.has_reset());
        assert!(!read_ff.has_clear());
        assert!(!read_ff.has_init_value());
        for &relation in &read_ff.relations {
            assert_eq!(relation, MemoryPortRelation::Undefined);
        }
        prototype.apply_input(&mut target_cell, "RADDR", self.swizzle_bram_addr(read_port.data_len, &read_port.addr));
        prototype.apply_param(&mut target_cell, "READ_MODE", (read_port.addr.len() - 8) as i64);

        let init_value = Const::from_iter((0..0x1000).map(|index| {
            let swz_index = (index & 0xff0) | SWIZZLE16[index & 0xf];
            if swz_index < memory.init_value.len() {
                memory.init_value[swz_index]
            } else {
                Trit::Undef
            }
        }));
        prototype.apply_param(&mut target_cell, "INIT", init_value);

        let target_cell_output = design.add_target(target_cell);
        let rdata = prototype.extract_output(&target_cell_output, "RDATA");
        match memory.read_ports[0].data_len {
            16 => {
                for (rdata_bit, index) in SWIZZLE16.into_iter().enumerate() {
                    design.replace_net(output[index], rdata[rdata_bit]);
                }
            }
            8 => {
                for (rdata_bit, index) in SWIZZLE16.into_iter().enumerate() {
                    if index < 8 {
                        design.replace_net(output[index], rdata[rdata_bit]);
                    }
                }
            }
            4 => {
                for (rdata_bit, index) in SWIZZLE16.into_iter().enumerate() {
                    if index >= 8 && index < 12 {
                        design.replace_net(output[index - 8], rdata[rdata_bit]);
                    }
                }
            }
            2 => {
                for (rdata_bit, index) in SWIZZLE16.into_iter().enumerate() {
                    if index >= 12 && index < 14 {
                        design.replace_net(output[index - 12], rdata[rdata_bit]);
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn lower_to_bram(&self, design: &Design, memory: &Memory, output: &Value, lowering: BramLowering) {
        for (read_port_index, swizzle) in lowering.read_port_swizzles.iter().enumerate() {
            let (mut port_memory, mut port_output) = memory.extract_read_ports(&[read_port_index], output);
            port_memory.unmap_read_init_reset_transparency(design, 0, true, &mut port_output);
            let read_ff = port_memory.read_ports[0].flip_flop.as_mut().unwrap();
            if let ControlNet::Neg(en) = read_ff.enable {
                read_ff.enable = ControlNet::Pos(design.add_not1(en));
            }
            port_memory.emulate_read_before_write(design);
            for (final_memory, final_output) in port_memory.swizzle(design, &port_output, swizzle) {
                self.lower_single_bram(design, &final_memory, &final_output);
            }
        }
    }

    pub fn lower_memories(&self, design: &mut Design) {
        for cell_ref in design.iter_cells() {
            let Cell::Memory(memory) = &*cell_ref.get() else { continue };
            let _guard = design.use_metadata_from(&[cell_ref]);
            let output = cell_ref.output();
            let bram_lowering = self.find_bram_lowering(memory);
            let fallback_ok = memory.can_lower_fallback();
            let fallback_reasonable =
                fallback_ok && if memory.write_ports.len() != 0 { memory.depth <= 1 } else { memory.depth <= 5 };
            cell_ref.unalive();
            if bram_lowering.is_some() && !fallback_reasonable {
                self.lower_to_bram(design, memory, &output, bram_lowering.unwrap());
            } else {
                memory.clone().lower_fallback(design, &output);
            }
        }
        design.compact();
    }
}
