use std::str::FromStr;

use prjunnamed_memory::MemoryExt;
use prjunnamed_netlist::{assert_isomorphic, Cell, Design};

fn extract_even_odd(design: &mut Design) {
    for cell_ref in design.iter_cells() {
        if let Cell::Memory(memory) = &*cell_ref.get() {
            let output = cell_ref.output();
            let even_ports = Vec::from_iter((0..memory.read_ports.len()).filter(|index| index % 2 == 0));
            let odd_ports = Vec::from_iter((0..memory.read_ports.len()).filter(|index| index % 2 == 1));
            let (mem0, output0) = memory.extract_read_ports(&even_ports, &output);
            let (mem1, output1) = memory.extract_read_ports(&odd_ports, &output);
            cell_ref.unalive();
            design.replace_value(output0, design.add_memory(mem0));
            design.replace_value(output1, design.add_memory(mem1));
        }
    }
    design.apply();
}

#[test]
fn test_simple() {
    let mut design = Design::from_str(concat!(
        "%0:4 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%21:1 = input \"r0e\"\n",
        "%22:1 = input \"r1e\"\n",
        "%23:1 = input \"r3e\"\n",
        "%30:4 = input \"r0a\"\n",
        "%40:4 = input \"r1a\"\n",
        "%50:4 = input \"r2a\"\n",
        "%60:4 = input \"r3a\"\n",
        "%70:4 = input \"r4a\"\n",
        "%80:1 = input \"wclk\"\n",
        "%81:1 = input \"r3clk\"\n",
        "%90:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%80\n",
        "    %90:4 = read addr=%30:4 clk=%80 en=%21 [rdfirst]\n",
        "    %91:16 = read addr=%40:4 clk=%80 en=%22 [trans]\n",
        "    %92:8 = read addr=%50:4\n",
        "    %93:4 = read addr=%60:4 clk=%81 en=%23 [undef]\n",
        "    %94:4 = read addr=%70:4\n",
        "}\n",
        "%130:0 = output \"r0d\" %90:4\n",
        "%140:0 = output \"r1d\" %91:16\n",
        "%150:0 = output \"r2d\" %92:8\n",
        "%160:0 = output \"r3d\" %93:4\n",
        "%170:0 = output \"r4d\" %94:4\n",
    ))
    .unwrap();
    extract_even_odd(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%21:1 = input \"r0e\"\n",
        "%22:1 = input \"r1e\"\n",
        "%23:1 = input \"r3e\"\n",
        "%30:4 = input \"r0a\"\n",
        "%40:4 = input \"r1a\"\n",
        "%50:4 = input \"r2a\"\n",
        "%60:4 = input \"r3a\"\n",
        "%70:4 = input \"r4a\"\n",
        "%80:1 = input \"wclk\"\n",
        "%81:1 = input \"r3clk\"\n",
        "%90:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%80\n",
        "    %90:4 = read addr=%30:4 clk=%80 en=%21 [rdfirst]\n",
        "    %92:8 = read addr=%50:4\n",
        "    %94:4 = read addr=%70:4\n",
        "}\n",
        "%91:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%80\n",
        "    %91:16 = read addr=%40:4 clk=%80 en=%22 [trans]\n",
        "    %93:4 = read addr=%60:4 clk=%81 en=%23 [undef]\n",
        "}\n",
        "%130:0 = output \"r0d\" %90:4\n",
        "%140:0 = output \"r1d\" %91:16\n",
        "%150:0 = output \"r2d\" %92:8\n",
        "%160:0 = output \"r3d\" %93:4\n",
        "%170:0 = output \"r4d\" %94:4\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
