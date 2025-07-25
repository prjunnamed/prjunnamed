use jzon::JsonValue;
use std::{cell::RefCell, collections::BTreeMap, io::Write};

use crate::yosys::{self, CellDetails, MetadataValue, NetDetails, PortDetails};
use prjunnamed_netlist::{
    Cell, Const, ControlNet, Design, IoNet, IoValue, MemoryPortRelation, MetaItem, MetaItemRef, Net, Trit, Value,
};

struct Counter(usize);

impl Counter {
    fn advance(&mut self) -> usize {
        let index = self.0;
        self.0 += 1;
        index
    }
}

struct NetlistIndexerState {
    map: BTreeMap<Net, usize>,
    io_map: BTreeMap<IoNet, usize>,
    next: Counter,
}

struct NetlistIndexer(RefCell<NetlistIndexerState>);

impl NetlistIndexer {
    fn new() -> NetlistIndexer {
        NetlistIndexer(RefCell::new(NetlistIndexerState {
            map: BTreeMap::new(),
            io_map: BTreeMap::new(),
            next: Counter(2), // "to avoid confusion", as write_json claims
        }))
    }

    fn net(&self, net: Net) -> yosys::Bit {
        match net.as_const() {
            Some(Trit::Undef) => yosys::Bit::Undef,
            Some(Trit::Zero) => yosys::Bit::Zero,
            Some(Trit::One) => yosys::Bit::One,
            None => {
                let state = &mut *self.0.borrow_mut();
                yosys::Bit::Net(*state.map.entry(net).or_insert_with(|| state.next.advance()))
            }
        }
    }

    fn synthetic_net(&self) -> yosys::Bit {
        yosys::Bit::Net(self.0.borrow_mut().next.advance())
    }

    fn value(&self, value: &Value) -> yosys::BitVector {
        yosys::BitVector(value.iter().map(|n| self.net(n)).collect::<Vec<_>>())
    }

    fn synthetic_value(&self, size: usize) -> yosys::BitVector {
        yosys::BitVector(std::iter::repeat_with(|| self.synthetic_net()).take(size).collect::<Vec<_>>())
    }

    fn io_net(&self, net: IoNet) -> yosys::Bit {
        let state = &mut *self.0.borrow_mut();
        yosys::Bit::Net(*state.io_map.entry(net).or_insert_with(|| state.next.advance()))
    }

    fn io_value(&self, value: &IoValue) -> yosys::BitVector {
        yosys::BitVector(value.iter().map(|n| self.io_net(n)).collect::<Vec<_>>())
    }
}

fn map_metadata(metadata: MetaItemRef) -> Vec<(String, yosys::MetadataValue)> {
    let mut ys_attrs: Vec<(String, MetadataValue)> = Vec::new();
    let mut ys_srcs = Vec::new();
    for item in metadata.iter() {
        match item.get() {
            MetaItem::Source { file, start, end } => {
                ys_srcs.push(format!(
                    "{}:{}.{}-{}.{}",
                    file.get(),
                    start.line + 1,
                    start.column + 1,
                    end.line + 1,
                    end.column + 1
                ));
            }
            MetaItem::Attr { name, value } => {
                ys_attrs.push((name.get().to_owned(), value.into()));
            }
            _ => (),
        }
    }
    if !ys_srcs.is_empty() {
        ys_attrs.push(("src".to_owned(), ys_srcs.join("|").into()));
    }
    ys_attrs
}

fn export_module(mut design: Design) -> yosys::Module {
    let indexer = NetlistIndexer::new();
    let mut ys_module = yosys::Module::new();

    // Yosys IR cannot express DFFs with both asynchronous and synchronous reset.
    for cell_ref in design.iter_cells() {
        if let Cell::Dff(flip_flop) = &*cell_ref.get() {
            if flip_flop.has_clear() && flip_flop.has_reset() {
                let mut flip_flop = flip_flop.clone();
                flip_flop.unmap_reset(&design);
                cell_ref.replace(Cell::Dff(flip_flop));
            }
        }
    }
    design.apply();

    for (name, io_value) in design.iter_ios() {
        ys_module.ports.add(name, PortDetails::new(yosys::PortDirection::Inout, indexer.io_value(&io_value)))
    }

    for cell_ref in design.iter_cells() {
        let cell_index = cell_ref.debug_index();
        let output = cell_ref.output();

        let ys_cell_name = format!("${cell_index}");

        let ys_cell_unary = |module: &mut yosys::Module, ty: &str, a: &Value| {
            CellDetails::new(ty)
                .param("A_SIGNED", 0)
                .param("A_WIDTH", a.len())
                .param("Y_WIDTH", output.len())
                .input("A", indexer.value(a))
                .output("Y", indexer.value(&output))
                .attrs(map_metadata(cell_ref.metadata()))
                .add_to(&format!("${cell_index}"), module)
        };

        let ys_cell_binary = |module: &mut yosys::Module, ty: &str, a: &Value, b: &Value, signed: bool| {
            CellDetails::new(ty)
                .param("A_SIGNED", if signed { 1 } else { 0 })
                .param("A_WIDTH", a.len())
                .param("B_SIGNED", if signed { 1 } else { 0 })
                .param("B_WIDTH", b.len())
                .param("Y_WIDTH", output.len())
                .input("A", indexer.value(a))
                .input("B", indexer.value(b))
                .output("Y", indexer.value(&output))
                .attrs(map_metadata(cell_ref.metadata()))
                .add_to(&format!("${cell_index}"), module)
        };

        let ys_shift_count = |module: &mut yosys::Module, a: &Value, stride: u32| -> yosys::BitVector {
            if stride == 1 {
                indexer.value(a)
            } else if stride == 0 {
                indexer.value(&Value::zero(1))
            } else {
                let stride_bits = stride.ilog2() + 1;
                let stride = Const::from_uint(stride.into(), stride_bits as usize);
                let result = indexer.synthetic_value(a.len() + stride.len());
                CellDetails::new("$mul")
                    .param("A_SIGNED", 0)
                    .param("A_WIDTH", a.len())
                    .param("B_SIGNED", 0)
                    .param("B_WIDTH", stride.len())
                    .param("Y_WIDTH", result.len())
                    .input("A", indexer.value(a))
                    .input("B", indexer.value(&Value::from(stride)))
                    .output("Y", result.clone())
                    .attrs(map_metadata(cell_ref.metadata()))
                    .add_to(&format!("${cell_index}$stride"), module);
                result
            }
        };

        let ys_cell_shift = |module: &mut yosys::Module, ty: &str, a: &Value, b: &Value, stride: u32, signed: bool| {
            let b = ys_shift_count(module, b, stride);
            CellDetails::new(ty)
                .param("A_SIGNED", if signed { 1 } else { 0 })
                .param("A_WIDTH", a.len())
                .param("B_SIGNED", 0)
                .param("B_WIDTH", b.len())
                .param("Y_WIDTH", output.len())
                .input("A", indexer.value(a))
                .input("B", b)
                .output("Y", indexer.value(&output))
                .attrs(map_metadata(cell_ref.metadata()))
                .add_to(&format!("${cell_index}"), module)
        };

        let ys_control_net_pos = |module: &mut yosys::Module, not_name: &str, cnet: ControlNet| -> yosys::Bit {
            match cnet {
                ControlNet::Pos(net) => indexer.net(net),
                ControlNet::Neg(net) => {
                    let result = indexer.synthetic_net();
                    CellDetails::new("$not")
                        .param("A_SIGNED", 0)
                        .param("A_WIDTH", 1)
                        .param("Y_WIDTH", output.len())
                        .input("A", indexer.value(&net.into()))
                        .output("Y", result)
                        .attrs(map_metadata(cell_ref.metadata()))
                        .add_to(not_name, module);
                    result
                }
            }
        };

        match &*cell_ref.get() {
            Cell::Buf(arg) => ys_cell_unary(&mut ys_module, "$pos", arg),
            Cell::Not(arg) => ys_cell_unary(&mut ys_module, "$not", arg),
            Cell::And(arg1, arg2) => ys_cell_binary(&mut ys_module, "$and", arg1, arg2, false),
            Cell::Or(arg1, arg2) => ys_cell_binary(&mut ys_module, "$or", arg1, arg2, false),
            Cell::Xor(arg1, arg2) => ys_cell_binary(&mut ys_module, "$xor", arg1, arg2, false),
            Cell::Mux(arg1, arg2, arg3) => CellDetails::new("$mux")
                .param("WIDTH", output.len())
                .input("A", indexer.value(arg3))
                .input("B", indexer.value(arg2))
                .input("S", indexer.net(*arg1))
                .output("Y", indexer.value(&output))
                .attrs(map_metadata(cell_ref.metadata()))
                .add_to(&format!("${cell_index}"), &mut ys_module),
            Cell::Adc(arg1, arg2, arg3) => {
                // The $alu cell isn't supported by `write_verilog`, so we have to pattern-match here.
                match arg3.as_const() {
                    Some(Trit::Zero) => {
                        // no carry-in
                        CellDetails::new("$add")
                            .param("A_SIGNED", 0)
                            .param("A_WIDTH", arg1.len())
                            .param("B_SIGNED", 0)
                            .param("B_WIDTH", arg2.len())
                            .param("Y_WIDTH", output.len())
                            .input("A", indexer.value(arg1))
                            .input("B", indexer.value(arg2))
                            .output("Y", indexer.value(&output))
                            .attrs(map_metadata(cell_ref.metadata()))
                            .add_to(&format!("${cell_index}"), &mut ys_module);
                    }
                    _ => {
                        // generic
                        let ys_a = Value::from(arg3).concat(arg1);
                        let ys_b = Value::from(Net::ONE).concat(arg2);
                        let ys_y = indexer.synthetic_value(1).concat(&indexer.value(&output));
                        CellDetails::new("$add")
                            .param("A_SIGNED", 0)
                            .param("A_WIDTH", 1 + arg1.len())
                            .param("B_SIGNED", 0)
                            .param("B_WIDTH", 1 + arg2.len())
                            .param("Y_WIDTH", 1 + output.len())
                            .input("A", indexer.value(&ys_a))
                            .input("B", indexer.value(&ys_b))
                            .output("Y", ys_y)
                            .attrs(map_metadata(cell_ref.metadata()))
                            .add_to(&format!("${cell_index}"), &mut ys_module);
                    }
                }
            }
            Cell::Aig(arg1, arg2) => {
                let arg1 = ys_control_net_pos(&mut ys_module, &format!("${cell_index}$not1"), *arg1);
                let arg2 = ys_control_net_pos(&mut ys_module, &format!("${cell_index}$not2"), *arg2);
                CellDetails::new("$and")
                    .param("A_SIGNED", 0)
                    .param("A_WIDTH", 1)
                    .param("B_SIGNED", 0)
                    .param("B_WIDTH", 1)
                    .param("Y_WIDTH", 1)
                    .input("A", arg1)
                    .input("B", arg2)
                    .output("Y", indexer.value(&output))
                    .attrs(map_metadata(cell_ref.metadata()))
                    .add_to(&format!("${cell_index}"), &mut ys_module)
            }

            Cell::Eq(arg1, arg2) => ys_cell_binary(&mut ys_module, "$eq", arg1, arg2, false),
            Cell::ULt(arg1, arg2) => ys_cell_binary(&mut ys_module, "$lt", arg1, arg2, false),
            Cell::SLt(arg1, arg2) => ys_cell_binary(&mut ys_module, "$lt", arg1, arg2, true),

            Cell::Shl(arg1, arg2, stride) => ys_cell_shift(&mut ys_module, "$shl", arg1, arg2, *stride, false),
            Cell::UShr(arg1, arg2, stride) => ys_cell_shift(&mut ys_module, "$shr", arg1, arg2, *stride, false),
            Cell::SShr(arg1, arg2, stride) => ys_cell_shift(&mut ys_module, "$sshr", arg1, arg2, *stride, true),
            Cell::XShr(arg1, arg2, stride) => ys_cell_shift(&mut ys_module, "$shiftx", arg1, arg2, *stride, false),

            Cell::Mul(arg1, arg2) => ys_cell_binary(&mut ys_module, "$mul", arg1, arg2, false),
            Cell::UDiv(arg1, arg2) => ys_cell_binary(&mut ys_module, "$div", arg1, arg2, false),
            Cell::UMod(arg1, arg2) => ys_cell_binary(&mut ys_module, "$mod", arg1, arg2, false),
            Cell::SDivTrunc(arg1, arg2) => ys_cell_binary(&mut ys_module, "$div", arg1, arg2, true),
            Cell::SDivFloor(arg1, arg2) => ys_cell_binary(&mut ys_module, "$divfloor", arg1, arg2, true),
            Cell::SModTrunc(arg1, arg2) => ys_cell_binary(&mut ys_module, "$mod", arg1, arg2, true),
            Cell::SModFloor(arg1, arg2) => ys_cell_binary(&mut ys_module, "$modfloor", arg1, arg2, true),

            Cell::Match { .. } => unimplemented!("match cells must be lowered first for Yosys JSON export"),
            Cell::Assign { .. } => unimplemented!("assign cells must be lowered first for Yosys JSON export"),

            Cell::Dff(flip_flop) => {
                let ys_cell_type = match (
                    flip_flop.has_clear(),
                    flip_flop.has_reset(),
                    flip_flop.has_enable(),
                    flip_flop.reset_over_enable,
                ) {
                    (true, true, _, _) => unreachable!(),
                    (true, false, false, _) => "$adff",
                    (true, false, true, _) => "$adffe",
                    (false, true, false, _) => "$sdff",
                    (false, true, true, false) => "$sdffce",
                    (false, true, true, true) => "$sdffe",
                    (false, false, false, _) => "$dff",
                    (false, false, true, _) => "$dffe",
                };
                let mut ys_cell = CellDetails::new(ys_cell_type);
                if flip_flop.has_clear() {
                    ys_cell = ys_cell
                        .param("ARST_POLARITY", flip_flop.clear.is_positive())
                        .param("ARST_VALUE", flip_flop.clear_value.clone())
                        .input("ARST", indexer.net(flip_flop.clear.net()));
                }
                if flip_flop.has_reset() {
                    ys_cell = ys_cell
                        .param("SRST_POLARITY", flip_flop.reset.is_positive())
                        .param("SRST_VALUE", flip_flop.reset_value.clone())
                        .input("SRST", indexer.net(flip_flop.reset.net()));
                }
                if flip_flop.has_enable() {
                    ys_cell = ys_cell
                        .param("EN_POLARITY", flip_flop.enable.is_positive())
                        .input("EN", indexer.net(flip_flop.enable.net()));
                }
                ys_cell
                    .param("CLK_POLARITY", flip_flop.clock.is_positive())
                    .input("CLK", indexer.net(flip_flop.clock.net()))
                    .param("WIDTH", output.len())
                    .input("D", indexer.value(&flip_flop.data))
                    .output("Q", indexer.value(&output))
                    .attrs(map_metadata(cell_ref.metadata()))
                    .add_to(&ys_cell_name, &mut ys_module);
                NetDetails::new(indexer.value(&output))
                    .attr("init", flip_flop.init_value.clone())
                    .add_to(&format!("{ys_cell_name}$ff"), &mut ys_module);
                continue; // skip default $out wire (init-less) creation
            }

            Cell::Memory(memory) => {
                let abits = memory
                    .write_ports
                    .iter()
                    .map(|port| port.addr.len() + port.wide_log2(memory))
                    .max()
                    .unwrap_or(0)
                    .max(
                        memory
                            .read_ports
                            .iter()
                            .map(|port| port.addr.len() + port.wide_log2(memory))
                            .max()
                            .unwrap_or(0),
                    );

                let mut wr_ports = 0;
                let mut wr_clk_polarity = Const::new();
                let mut wr_wide_continuation = Const::new();
                let mut wr_addr = Value::new();
                let mut wr_data = Value::new();
                let mut wr_clk = Value::new();
                let mut wr_en = Value::new();
                let mut write_port_indices = vec![];
                for (port_index, port) in memory.write_ports.iter().enumerate() {
                    let wide_log2 = port.wide_log2(memory);
                    for index in 0..(1 << wide_log2) {
                        let addr = Value::from(Const::from_uint(index, wide_log2)).concat(&port.addr).zext(abits);
                        wr_clk.extend([port.clock.net()]);
                        wr_addr.extend(&addr);
                        wr_clk_polarity.push(port.clock.is_positive());
                        wr_wide_continuation.push(index != 0);
                        write_port_indices.push(port_index);
                    }
                    wr_data.extend(&port.data);
                    wr_en.extend(&port.mask);
                    wr_ports += 1 << wide_log2;
                }
                if wr_ports == 0 {
                    wr_clk_polarity.push(false);
                    wr_wide_continuation.push(false);
                }

                let mut rd_ports = 0;
                let mut rd_clk_enable = Const::new();
                let mut rd_clk_polarity = Const::new();
                let mut rd_transparency_mask = Const::new();
                let mut rd_collision_x_mask = Const::new();
                let mut rd_wide_continuation = Const::new();
                let mut rd_ce_over_srst = Const::new();
                let mut rd_arst_value = Const::new();
                let mut rd_srst_value = Const::new();
                let mut rd_init_value = Const::new();
                let mut rd_clk = Value::new();
                let mut rd_en = yosys::BitVector(vec![]);
                let mut rd_arst = yosys::BitVector(vec![]);
                let mut rd_srst = yosys::BitVector(vec![]);
                let mut rd_addr = Value::new();
                for (port_index, port) in memory.read_ports.iter().enumerate() {
                    let wide_log2 = port.wide_log2(memory);
                    for index in 0..(1 << wide_log2) {
                        let addr = Value::from(Const::from_uint(index, wide_log2)).concat(&port.addr).zext(abits);
                        rd_addr.extend(&addr);
                        if let Some(ref flip_flop) = port.flip_flop {
                            rd_clk.extend([flip_flop.clock.net()]);
                            rd_clk_enable.push(true);
                            rd_clk_polarity.push(flip_flop.clock.is_positive());
                            rd_ce_over_srst.push(!flip_flop.reset_over_enable);
                            for &write_port_index in &write_port_indices {
                                let (trans, col_x) = if flip_flop.clock == memory.write_ports[write_port_index].clock {
                                    match flip_flop.relations[write_port_index] {
                                        MemoryPortRelation::Undefined => (false, true),
                                        MemoryPortRelation::ReadBeforeWrite => (false, false),
                                        MemoryPortRelation::Transparent => (true, false),
                                    }
                                } else {
                                    (false, false)
                                };
                                rd_transparency_mask.push(trans);
                                rd_collision_x_mask.push(col_x);
                            }
                        } else {
                            rd_clk.extend([Net::UNDEF]);
                            rd_clk_enable.push(false);
                            rd_clk_polarity.push(Trit::Undef);
                            rd_ce_over_srst.push(Trit::Undef);
                            rd_transparency_mask.extend(Const::undef(wr_ports));
                            rd_collision_x_mask.extend(Const::undef(wr_ports));
                        }
                        rd_wide_continuation.push(index != 0);
                    }
                    if let Some(ref flip_flop) = port.flip_flop {
                        rd_arst_value.extend(&flip_flop.clear_value);
                        rd_srst_value.extend(&flip_flop.reset_value);
                        rd_init_value.extend(&flip_flop.init_value);
                        let enable = ys_control_net_pos(
                            &mut ys_module,
                            &format!("${cell_index}$rd{port_index}$en$not"),
                            flip_flop.enable,
                        );
                        let clear = ys_control_net_pos(
                            &mut ys_module,
                            &format!("${cell_index}$rd{port_index}$arst$not"),
                            flip_flop.clear,
                        );
                        let reset = ys_control_net_pos(
                            &mut ys_module,
                            &format!("${cell_index}$rd{port_index}$srst$not"),
                            flip_flop.reset,
                        );
                        rd_en = rd_en.concat(&yosys::BitVector(vec![enable; 1 << wide_log2]));
                        rd_srst = rd_srst.concat(&yosys::BitVector(vec![reset; 1 << wide_log2]));
                        rd_arst = rd_arst.concat(&yosys::BitVector(vec![clear; 1 << wide_log2]));
                    } else {
                        rd_arst_value.extend(Const::undef(port.data_len));
                        rd_srst_value.extend(Const::undef(port.data_len));
                        rd_init_value.extend(Const::undef(port.data_len));
                        rd_en = rd_en.concat(&yosys::BitVector(vec![yosys::Bit::One; 1 << wide_log2]));
                        rd_srst = rd_srst.concat(&yosys::BitVector(vec![yosys::Bit::Zero; 1 << wide_log2]));
                        rd_arst = rd_arst.concat(&yosys::BitVector(vec![yosys::Bit::Zero; 1 << wide_log2]));
                    }
                    rd_ports += 1 << wide_log2;
                }
                if rd_ports == 0 {
                    rd_clk_enable.push(false);
                    rd_clk_polarity.push(false);
                    rd_wide_continuation.push(false);
                    rd_ce_over_srst.push(false);
                    rd_arst_value.push(false);
                    rd_srst_value.push(false);
                    rd_init_value.push(false);
                }
                if rd_ports == 0 || wr_ports == 0 {
                    rd_transparency_mask.push(false);
                    rd_collision_x_mask.push(false);
                }

                CellDetails::new("$mem_v2")
                    .param("MEMID", format!("${cell_index}$mem"))
                    .param("OFFSET", 0)
                    .param("SIZE", memory.depth)
                    .param("WIDTH", memory.width)
                    .param("ABITS", abits)
                    .param("INIT", memory.init_value.clone())
                    .param("WR_PORTS", wr_ports)
                    .param("WR_CLK_ENABLE", Const::ones(wr_ports.max(1)))
                    .param("WR_CLK_POLARITY", wr_clk_polarity)
                    .param("WR_PRIORITY_MASK", Const::zero(wr_ports.max(1)))
                    .param("WR_WIDE_CONTINUATION", wr_wide_continuation)
                    .input("WR_ADDR", indexer.value(&wr_addr))
                    .input("WR_DATA", indexer.value(&wr_data))
                    .input("WR_CLK", indexer.value(&wr_clk))
                    .input("WR_EN", indexer.value(&wr_en))
                    .param("RD_PORTS", rd_ports)
                    .param("RD_CLK_ENABLE", rd_clk_enable)
                    .param("RD_CLK_POLARITY", rd_clk_polarity)
                    .param("RD_TRANSPARENCY_MASK", rd_transparency_mask)
                    .param("RD_COLLISION_X_MASK", rd_collision_x_mask)
                    .param("RD_WIDE_CONTINUATION", rd_wide_continuation)
                    .param("RD_CE_OVER_SRST", rd_ce_over_srst)
                    .param("RD_ARST_VALUE", rd_arst_value)
                    .param("RD_SRST_VALUE", rd_srst_value)
                    .param("RD_INIT_VALUE", rd_init_value)
                    .input("RD_CLK", indexer.value(&rd_clk))
                    .input("RD_EN", rd_en)
                    .input("RD_ARST", rd_arst)
                    .input("RD_SRST", rd_srst)
                    .input("RD_ADDR", indexer.value(&rd_addr))
                    .output("RD_DATA", indexer.value(&output))
                    .add_to(&format!("${cell_index}"), &mut ys_module);
            }

            Cell::IoBuf(io_buffer) => {
                let ys_enable = ys_control_net_pos(&mut ys_module, &format!("${cell_index}$en$not"), io_buffer.enable);
                let ys_attrs = map_metadata(cell_ref.metadata());
                CellDetails::new("$tribuf")
                    .param("WIDTH", output.len())
                    .input("A", indexer.value(&io_buffer.output))
                    .input("EN", ys_enable)
                    .output("Y", indexer.io_value(&io_buffer.io))
                    .attrs(ys_attrs.clone())
                    .add_to(&format!("${cell_index}"), &mut ys_module);
                CellDetails::new("$pos")
                    .param("A_SIGNED", 0)
                    .param("A_WIDTH", io_buffer.io.len())
                    .param("Y_WIDTH", output.len())
                    .input("A", indexer.io_value(&io_buffer.io))
                    .output("Y", indexer.value(&output))
                    .attrs(ys_attrs)
                    .add_to(&format!("${cell_index}$pos"), &mut ys_module);
            }

            Cell::Other(instance) => {
                let mut ys_cell = CellDetails::new(&instance.kind);
                for (name, value) in instance.params.iter() {
                    ys_cell = ys_cell.param(name, value);
                }
                for (name, value) in instance.inputs.iter() {
                    ys_cell = ys_cell.input(name, indexer.value(value));
                }
                for (name, value_range) in instance.outputs.iter() {
                    ys_cell = ys_cell.output(name, indexer.value(&Value::from(&output[value_range.clone()])));
                }
                for (name, io_value) in instance.ios.iter() {
                    ys_cell = ys_cell.inout(name, indexer.io_value(io_value));
                }
                ys_cell.attrs(map_metadata(cell_ref.metadata())).add_to(&ys_cell_name, &mut ys_module);
            }
            Cell::Target(_target_cell) => {
                unimplemented!("target cells must be converted to instances first for Yosys JSON export")
            }

            Cell::Input(port_name, _size) => {
                ys_module.ports.add(port_name, PortDetails::new(yosys::PortDirection::Input, indexer.value(&output)))
            }
            Cell::Output(port_name, value) => {
                ys_module.ports.add(port_name, PortDetails::new(yosys::PortDirection::Output, indexer.value(value)))
            }
            Cell::Name(name, value) | Cell::Debug(name, value) => ys_module.netnames.add(
                &name.replace(" ", "."),
                NetDetails::new(indexer.value(value)).attrs(map_metadata(cell_ref.metadata())).attr("hdlname", name),
            ),
        };

        if output.len() > 0 {
            // Duplicating the cell metadata on the cell output net is the only way to get source locations
            // to show up in nextpnr's timing reports.
            NetDetails::new(indexer.value(&output))
                .attrs(map_metadata(cell_ref.metadata()))
                .add_to(&format!("{ys_cell_name}$out"), &mut ys_module);
        }
    }

    ys_module
}

// Exporting a design to Yosys JSON can require modifying it if it has target cells that must be mapped to instances.
pub fn export(writer: &mut impl Write, designs: BTreeMap<String, Design>) -> std::io::Result<()> {
    let mut ys_modules = BTreeMap::new();
    for (name, design) in designs {
        ys_modules.insert(name, export_module(design));
    }
    let ys_design = yosys::Design { creator: "prjunnamed".into(), modules: ys_modules.into() };

    let json = JsonValue::from(ys_design);
    json.write_pretty(writer, /*spaces=*/ 4)
}
