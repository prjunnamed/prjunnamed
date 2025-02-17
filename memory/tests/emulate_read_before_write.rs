use std::str::FromStr;

use prjunnamed_memory::MemoryExt;
use prjunnamed_netlist::{assert_isomorphic, Cell, Design};

fn emulate_all_rdfirst(design: &mut Design) {
    for cell_ref in design.iter_cells() {
        if let Cell::Memory(memory) = &*cell_ref.get() {
            let mut memory = memory.clone();
            memory.emulate_read_before_write(design);
            cell_ref.replace(Cell::Memory(memory));
        }
    }
    design.apply();
}

#[test]
fn test_simple() {
    let mut design = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:2 = input \"we\"\n",
        "%22:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:_ = memory depth=#16 width=#8 {\n",
        "    init 01011100\n",
        "    write addr=%10:4 data=%0:8 mask=[%20+1*4 %20+0*4] clk=%40\n",
        "    %50:8 = read addr=%30:4 clk=%40 en=%22 [rdfirst]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:8\n",
    ))
    .unwrap();
    emulate_all_rdfirst(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:2 = input \"we\"\n",
        "%22:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:8 = dff %0:8 clk=%40\n",
        "%60:4 = dff %10:4 clk=%40\n",
        "%70:2 = dff %20:2 clk=%40 init=00\n",
        "%80:_ = memory depth=#16 width=#8 {\n",
        "    init 01011100\n",
        "    write addr=%60:4 data=%50:8 mask=[%70+1*4 %70+0*4] clk=!%40\n",
        "    %80:8 = read addr=%30:4 clk=%40 en=%22 [undef]\n",
        "}\n",
        "%90:0 = output \"rd\" %80:8\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_simple_no_init() {
    let mut design = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:2 = input \"we\"\n",
        "%22:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:_ = memory depth=#16 width=#8 {\n",
        "    write addr=%10:4 data=%0:8 mask=[%20+1*4 %20+0*4] clk=%40\n",
        "    %50:8 = read addr=%30:4 clk=%40 en=%22 [rdfirst]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:8\n",
    ))
    .unwrap();
    emulate_all_rdfirst(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:2 = input \"we\"\n",
        "%22:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:8 = dff %0:8 clk=%40\n",
        "%60:4 = dff %10:4 clk=%40\n",
        "%70:2 = dff %20:2 clk=%40\n",
        "%80:_ = memory depth=#16 width=#8 {\n",
        "    write addr=%60:4 data=%50:8 mask=[%70+1*4 %70+0*4] clk=!%40\n",
        "    %80:8 = read addr=%30:4 clk=%40 en=%22 [undef]\n",
        "}\n",
        "%90:0 = output \"rd\" %80:8\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
