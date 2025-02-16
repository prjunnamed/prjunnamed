use std::str::FromStr;

use prjunnamed_memory::{MemoryExt, MemorySwizzle};
use prjunnamed_netlist::{assert_isomorphic, Cell, Design};

fn lower_memories(design: &mut Design, swizzle: &MemorySwizzle, write_port_granularity: &[usize]) {
    for cell_ref in design.iter_cells() {
        if let Cell::Memory(memory) = &*cell_ref.get() {
            let output = cell_ref.output();
            cell_ref.unalive();
            let mut swizzle = swizzle.clone();
            swizzle.data_swizzle = memory.make_data_swizzle(write_port_granularity);
            let swizzle_depths = memory.swizzle_depths(&swizzle);
            let memories = memory.clone().swizzle(design, &output, &swizzle);
            assert_eq!(memories.len(), swizzle_depths.len());
            for ((new_mem, new_output), depth) in memories.into_iter().zip(swizzle_depths) {
                assert_eq!(new_mem.depth, depth);
                design.replace_value(new_output, design.add_memory(new_mem));
            }
        }
    }
    design.apply();
}

#[test]
fn test_simple() {
    let mut design = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:5 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%30:5 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:8 = memory depth=#24 width=#8 {\n",
        "    init 01011100\n",
        "    write addr=%10:5 data=%0:8 mask=%20*8 clk=%40\n",
        "    read addr=%30:5 width=#8\n",
        "}\n",
        "%60:0 = output \"rd\" %50:8\n",
    ))
    .unwrap();
    let swizzle = MemorySwizzle {
        data_swizzle: vec![],
        soft_addr_bits_mask: 0,
        write_wide_log2: vec![0],
        read_wide_log2: vec![0],
        hard_addr_bits: 4,
        data_width_unit: 4,
    };
    lower_memories(&mut design, &swizzle, &[1]);
    let mut gold = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:5 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%30:5 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:1 = eq %10+4 0\n",
        "%51:1 = eq %10+4 1\n",
        "%60:1 = mux %50 %20 0\n",
        "%61:1 = mux %51 %20 0\n",
        "%70:4 = memory depth=#16 width=#4 {\n",
        "    init 1100\n",
        "    write addr=%10:4 data=%0+0:4 mask=%60*4 clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%80:4 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0+4:4 mask=%60*4 clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%90:4 = memory depth=#8 width=#4 {\n",
        "    write addr=%10:4 data=%0+0:4 mask=%61*4 clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%100:4 = memory depth=#8 width=#4 {\n",
        "    write addr=%10:4 data=%0+4:4 mask=%61*4 clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%110:8 = mux %30+4 [%100:4 %90:4] [%80:4 %70:4]\n",
        "%120:0 = output \"rd\" %110:8\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_compact() {
    let mut design = Design::from_str(concat!(
        "%0:5 = input \"wd\"\n",
        "%10:6 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%30:6 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:5 = memory depth=#47 width=#5 {\n",
        "    init 01010\n",
        "    init 11100\n",
        "    init 10011\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init XXXXX\n",
        "    init 11111\n",
        "    init 00000\n",
        "    write addr=%10:6 data=%0:5 mask=%20*5 clk=%40\n",
        "    read addr=%30:6 width=#5\n",
        "}\n",
        "%60:0 = output \"rd\" %50:5\n",
    ))
    .unwrap();
    let swizzle = MemorySwizzle {
        data_swizzle: vec![],
        soft_addr_bits_mask: 0,
        write_wide_log2: vec![0],
        read_wide_log2: vec![0],
        hard_addr_bits: 4,
        data_width_unit: 4,
    };
    lower_memories(&mut design, &swizzle, &[1]);
    let mut gold = Design::from_str(concat!(
        "%0:5 = input \"wd\"\n",
        "%10:6 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%30:6 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:1 = eq [%10+5 %10+4] 00\n",
        "%51:1 = eq [%10+5 %10+4] 01\n",
        "%52:1 = eq [%10+5 %10+4] 10\n",
        "%60:1 = mux %50 %20 0\n",
        "%61:1 = mux %51 %20 0\n",
        "%62:1 = mux %52 %20 0\n",
        "%70:4 = memory depth=#16 width=#4 {\n",
        "    init 1010\n",
        "    init 1100\n",
        "    init 0011\n",
        "    write addr=%10:4 data=%0:4 mask=%60*4 clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%80:4 = memory depth=#16 width=#4 {\n",
        "    init 1110\n",
        "    init 0001\n",
        "    init XXX1\n",
        "    write addr=%10:4 data=[%0+2 %0+1 %0+0 %0+4] mask=[%61*3 %60] clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%90:4 = memory depth=#16 width=#4 {\n",
        "    init XX11\n",
        "    init XX00\n",
        "    write addr=%10:4 data=[%0+1 %0+0 %0+4 %0+3] mask=[%62*2 %61*2] clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%100:4 = memory depth=#15 width=#4 {\n",
        "    write addr=%10:4 data=[X %0+4 %0+3 %0+2] mask=[0 %62*3] clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%110:5 = mux %30+4 [%90+0:2 %80+1:3] [%80+0 %70:4]\n",
        "%120:5 = mux %30+5 [%100+0:3 %90+2:2] %110:5\n",
        "%130:0 = output \"rd\" %120:5\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_wide_read() {
    let mut design = Design::from_str(concat!(
        "%0:2 = input \"wd\"\n",
        "%10:5 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:3 = input \"ra\"\n",
        "%40:1 = input \"wclk\"\n",
        "%41:1 = input \"rclk\"\n",
        "%50:8 = memory depth=#64 width=#1 {\n",
        "    init 0\n",
        "    init 1\n",
        "    init 0\n",
        "    init 1\n",
        "    init 0\n",
        "    init 0\n",
        "    init 1\n",
        "    init 1\n",
        "    write addr=%10:5 data=%0:2 mask=%20*2 clk=%40\n",
        "    read addr=%30:3 width=#8 clk=%41 en=%21 init=11011000 [undef]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:8\n",
    ))
    .unwrap();
    let swizzle = MemorySwizzle {
        data_swizzle: vec![],
        soft_addr_bits_mask: 1,
        write_wide_log2: vec![0],
        read_wide_log2: vec![3],
        hard_addr_bits: 4,
        data_width_unit: 1,
    };
    lower_memories(&mut design, &swizzle, &[1]);
    let mut gold = Design::from_str(concat!(
        "%0:2 = input \"wd\"\n",
        "%10:5 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:3 = input \"ra\"\n",
        "%40:1 = input \"wclk\"\n",
        "%41:1 = input \"rclk\"\n",
        "%50:1 = eq %10+4 0\n",
        "%51:1 = eq %10+4 1\n",
        "%60:1 = mux %50 %20 0\n",
        "%61:1 = mux %51 %20 0\n",
        "%70:8 = memory depth=#16 width=#1 {\n",
        "    init 0\n",
        "    init 0\n",
        "    init 0\n",
        "    init 1\n",
        "    write addr=%10:4 data=%0 mask=%60 clk=%40\n",
        "    read addr=%30+1 width=#8 clk=%41 en=%21 init=11001100 [undef]\n",
        "}\n",
        "%80:8 = memory depth=#16 width=#1 {\n",
        "    init 1\n",
        "    init 1\n",
        "    init 0\n",
        "    init 1\n",
        "    write addr=%10:4 data=%0+1 mask=%60 clk=%40\n",
        "    read addr=%30+1 width=#8 clk=%41 en=%21 init=10101010 [undef]\n",
        "}\n",
        "%90:8 = memory depth=#16 width=#1 {\n",
        "    write addr=%10:4 data=%0 mask=%61 clk=%40\n",
        "    read addr=%30+1 width=#8 clk=%41 en=%21 init=11001100 [undef]\n",
        "}\n",
        "%100:8 = memory depth=#16 width=#1 {\n",
        "    write addr=%10:4 data=%0+1 mask=%61 clk=%40\n",
        "    read addr=%30+1 width=#8 clk=%41 en=%21 init=10101010 [undef]\n",
        "}\n",
        "%110:2 = dff [%30+2 %30+0] clk=%41 en=%21\n",
        "%120:8 = mux %110+0 [%80+7 %70+7 %80+6 %70+6 %80+5 %70+5 %80+4 %70+4] [%80+3 %70+3 %80+2 %70+2 %80+1 %70+1 %80+0 %70+0]\n",
        "%130:8 = mux %110+0 [%100+7 %90+7 %100+6 %90+6 %100+5 %90+5 %100+4 %90+4] [%100+3 %90+3 %100+2 %90+2 %100+1 %90+1 %100+0 %90+0]\n",
        "%140:8 = mux %110+1 %130:8 %120:8\n",
        "%150:0 = output \"rd\" %140:8\n",
    )).unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_wide_write() {
    let mut design = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:3 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:5 = input \"ra\"\n",
        "%40:1 = input \"wclk\"\n",
        "%41:1 = input \"rclk\"\n",
        "%50:2 = memory depth=#64 width=#1 {\n",
        "    init 0\n",
        "    init 1\n",
        "    init 0\n",
        "    init 1\n",
        "    init 0\n",
        "    init 0\n",
        "    init 1\n",
        "    init 1\n",
        "    write addr=%10:3 data=%0:8 mask=%20*8 clk=%40\n",
        "    read addr=%30:5 width=#2 clk=%41 en=%21 init=10 [undef]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:2\n",
    ))
    .unwrap();
    let swizzle = MemorySwizzle {
        data_swizzle: vec![],
        soft_addr_bits_mask: 1,
        write_wide_log2: vec![3],
        read_wide_log2: vec![0],
        hard_addr_bits: 4,
        data_width_unit: 1,
    };
    lower_memories(&mut design, &swizzle, &[1]);
    let mut gold = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:3 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:5 = input \"ra\"\n",
        "%40:1 = input \"wclk\"\n",
        "%41:1 = input \"rclk\"\n",
        "%50:1 = eq [%10+2 %10+0] 00\n",
        "%51:1 = eq [%10+2 %10+0] 01\n",
        "%52:1 = eq [%10+2 %10+0] 10\n",
        "%53:1 = eq [%10+2 %10+0] 11\n",
        "%60:1 = mux %50 %20 0\n",
        "%61:1 = mux %51 %20 0\n",
        "%62:1 = mux %52 %20 0\n",
        "%63:1 = mux %53 %20 0\n",
        "%70:1 = memory depth=#16 width=#1 {\n",
        "    init 0\n",
        "    init 0\n",
        "    init 0\n",
        "    init 1\n",
        "    write addr=%10+1 data=[%0+6 %0+4 %0+2 %0+0 %0+6 %0+4 %0+2 %0+0] mask=[%61*4 %60*4] clk=%40\n",
        "    read addr=%30:4 width=#1 clk=%41 en=%21 init=0 [undef]\n",
        "}\n",
        "%80:1 = memory depth=#16 width=#1 {\n",
        "    init 1\n",
        "    init 1\n",
        "    init 0\n",
        "    init 1\n",
        "    write addr=%10+1 data=[%0+7 %0+5 %0+3 %0+1 %0+7 %0+5 %0+3 %0+1] mask=[%61*4 %60*4] clk=%40\n",
        "    read addr=%30:4 width=#1 clk=%41 en=%21 init=1 [undef]\n",
        "}\n",
        "%90:1 = memory depth=#16 width=#1 {\n",
        "    write addr=%10+1 data=[%0+6 %0+4 %0+2 %0+0 %0+6 %0+4 %0+2 %0+0] mask=[%63*4 %62*4 ] clk=%40\n",
        "    read addr=%30:4 width=#1 clk=%41 en=%21 init=0 [undef]\n",
        "}\n",
        "%100:1 = memory depth=#16 width=#1 {\n",
        "    write addr=%10+1 data=[%0+7 %0+5 %0+3 %0+1 %0+7 %0+5 %0+3 %0+1] mask=[%63*4 %62*4 ] clk=%40\n",
        "    read addr=%30:4 width=#1 clk=%41 en=%21 init=1 [undef]\n",
        "}\n",
        "%110:1 = dff %30+4 clk=%41 en=%21\n",
        "%120:2 = mux %110 [%100 %90] [%80 %70]\n",
        "%130:0 = output \"rd\" %120:2\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_granularity() {
    let mut design = Design::from_str(concat!(
        "%0:5 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:3 = input \"we\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:5 = memory depth=#16 width=#5 {\n",
        "    init 01011\n",
        "    write addr=%10:4 data=%0:5 mask=[%20+2 %20+1*3 %20+0] clk=%40\n",
        "    read addr=%30:4 width=#5\n",
        "}\n",
        "%60:0 = output \"rd\" %50:5\n",
    ))
    .unwrap();
    let swizzle = MemorySwizzle {
        data_swizzle: vec![],
        soft_addr_bits_mask: 0,
        write_wide_log2: vec![0],
        read_wide_log2: vec![0],
        hard_addr_bits: 4,
        data_width_unit: 8,
    };
    lower_memories(&mut design, &swizzle, &[2]);
    let mut gold = Design::from_str(concat!(
        "%0:5 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:3 = input \"we\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:8 = memory depth=#16 width=#8 {\n",
        "    init X0X101X1\n",
        "    write addr=%10:4 data=[X %0+4 X %0+1:3 X %0+0] mask=[%20+2*2 %20+1*4 %20+0*2] clk=%40\n",
        "    read addr=%30:4 width=#8\n",
        "}\n",
        "%60:0 = output \"rd\" [%50+6 %50+2:3 %50+0]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
