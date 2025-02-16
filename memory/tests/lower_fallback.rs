use std::str::FromStr;

use prjunnamed_memory::MemoryExt;
use prjunnamed_netlist::{assert_isomorphic, Cell, Design};

fn lower_fallback(design: &mut Design) {
    for cell_ref in design.iter_cells() {
        let Cell::Memory(memory) = &*cell_ref.get() else { continue };
        let memory = memory.clone();
        let output = cell_ref.output();
        cell_ref.unalive();
        memory.lower_fallback(design, &output);
    }
    design.apply();
}

#[test]
fn test_simple() {
    let mut design = Design::from_str(concat!(
        "%0:8 = input \"w0d\"\n",
        "%10:4 = input \"w0a\"\n",
        "%20:1 = input \"w0e\"\n",
        "%30:4 = input \"w1d\"\n",
        "%40:4 = input \"w1a\"\n",
        "%50:1 = input \"w1e\"\n",
        "%60:4 = input \"r0a\"\n",
        "%70:4 = input \"r1a\"\n",
        "%80:1 = input \"r1e\"\n",
        "%90:1 = input \"wclk\"\n",
        "%91:1 = input \"rclk\"\n",
        "%100:12 = memory depth=#4 width=#4 {\n",
        "    init 0101\n",
        "    init 0000\n",
        "    init 1111\n",
        "    init X01X\n",
        "    write addr=%10:4 data=%0:8 mask=%20*8 clk=%90\n",
        "    write addr=%40:4 data=%30:4 mask=%50*4 clk=%90\n",
        "    read addr=%60:4 width=#8\n",
        "    read addr=%70:4 width=#4 clk=%91 en=%80 [undef undef]\n",
        "}\n",
        "%110:0 = output \"r0d\" %100+0:8\n",
        "%120:0 = output \"r1d\" %100+8:4\n",
    ))
    .unwrap();
    lower_fallback(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:8 = input \"w0d\"\n",
        "%10:4 = input \"w0a\"\n",
        "%20:1 = input \"w0e\"\n",
        "%30:4 = input \"w1d\"\n",
        "%40:4 = input \"w1a\"\n",
        "%50:1 = input \"w1e\"\n",
        "%60:4 = input \"r0a\"\n",
        "%70:4 = input \"r1a\"\n",
        "%80:1 = input \"r1e\"\n",
        "%90:1 = input \"wclk\"\n",
        "%91:1 = input \"rclk\"\n",
        "%100:1 = eq %10:4 0000\n",
        "%101:1 = eq %10:4 0001\n",
        "%110:1 = mux %100 %20 0\n",
        "%111:1 = mux %101 %20 0\n",
        "%120:1 = eq %40:4 0000\n",
        "%121:1 = eq %40:4 0001\n",
        "%122:1 = eq %40:4 0010\n",
        "%123:1 = eq %40:4 0011\n",
        "%130:1 = mux %120 %50 0\n",
        "%131:1 = mux %121 %50 0\n",
        "%132:1 = mux %122 %50 0\n",
        "%133:1 = mux %123 %50 0\n",
        "%140:4 = mux %110 %0+0:4 %160:4\n",
        "%141:4 = mux %110 %0+4:4 %161:4\n",
        "%142:4 = mux %111 %0+0:4 %162:4\n",
        "%143:4 = mux %111 %0+4:4 %163:4\n",
        "%150:4 = mux %130 %30+0:4 %140:4\n",
        "%151:4 = mux %131 %30+0:4 %141:4\n",
        "%152:4 = mux %132 %30+0:4 %142:4\n",
        "%153:4 = mux %133 %30+0:4 %143:4\n",
        "%160:4 = dff %150:4 clk=%90 init=0101\n",
        "%161:4 = dff %151:4 clk=%90 init=0000\n",
        "%162:4 = dff %152:4 clk=%90 init=1111\n",
        "%163:4 = dff %153:4 clk=%90 init=X01X\n",
        "%170:8 = mux %60+0 [%163:4 %162:4] [%161:4 %160:4]\n",
        "%180:4 = mux %70+0 %161:4 %160:4\n",
        "%181:4 = mux %70+0 %163:4 %162:4\n",
        "%182:4 = mux %70+1 %181:4 %180:4\n",
        "%190:4 = dff %182:4 clk=%91 en=%80\n",
        "%200:0 = output \"r0d\" %170:8\n",
        "%210:0 = output \"r1d\" %190:4\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}