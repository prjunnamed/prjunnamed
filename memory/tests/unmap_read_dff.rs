use std::str::FromStr;

use prjunnamed_memory::MemoryExt;
use prjunnamed_netlist::{assert_isomorphic, Cell, Design};

fn unmap_read_dffs(design: &mut Design) {
    for cell_ref in design.iter_cells() {
        if let Cell::Memory(memory) = &*cell_ref.get() {
            let mut output = cell_ref.output();
            let mut memory = memory.clone();
            cell_ref.unalive();
            for port_index in 0..memory.read_ports.len() {
                memory.unmap_read_dff(design, port_index, &mut output);
            }
            design.replace_value(output, design.add_memory(memory));
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
        "%21:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"wclk\"\n",
        "%41:1 = input \"rclk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:4 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%40 \n",
        "    read addr=%30:4 width=#4 clk=%41 clr=%42,1111 rst=%43,0000 en=%21 rst>en init=1010 [undef]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:4\n",
    ))
    .unwrap();
    unmap_read_dffs(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"wclk\"\n",
        "%41:1 = input \"rclk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:4 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%60:4 = dff %50:4 clk=%41 clr=%42,1111 rst=%43,0000 en=%21 rst>en init=1010\n",
        "%70:0 = output \"rd\" %60:4\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_trans_simple() {
    let mut design = Design::from_str(concat!(
        "%0:4 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:4 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%40 \n",
        "    read addr=%30:4 width=#4 clk=%40 clr=%42,1111 rst=%43,0000 en=%21 rst>en init=1010 [trans]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:4\n",
    ))
    .unwrap();
    unmap_read_dffs(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:1 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:4 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%40\n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%60:1 = eq %30:4 %10:4\n",
        "%61:1 = mux %60 %20 0\n",
        "%70:4 = mux %61 %0:4 %50:4\n",
        "%100:4 = dff %70:4 clk=%40 clr=%42,1111 rst=%43,0000 en=%21 rst>en init=1010\n",
        "%110:0 = output \"rd\" %100:4\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_trans_double() {
    let mut design = Design::from_str(concat!(
        "%0:4 = input \"w0d\"\n",
        "%4:4 = input \"w1d\"\n",
        "%10:4 = input \"w0a\"\n",
        "%14:4 = input \"w1a\"\n",
        "%20:1 = input \"w0e\"\n",
        "%21:1 = input \"w1e\"\n",
        "%22:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:4 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%40 \n",
        "    write addr=%14:4 data=%4:4 mask=%21*4 clk=%40 \n",
        "    read addr=%30:4 width=#4 clk=%40 clr=%42,1111 rst=%43,0000 en=%22 rst>en init=1010 [trans trans]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:4\n",
    ))
    .unwrap();
    unmap_read_dffs(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"w0d\"\n",
        "%4:4 = input \"w1d\"\n",
        "%10:4 = input \"w0a\"\n",
        "%14:4 = input \"w1a\"\n",
        "%20:1 = input \"w0e\"\n",
        "%21:1 = input \"w1e\"\n",
        "%22:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:4 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%40 \n",
        "    write addr=%14:4 data=%4:4 mask=%21*4 clk=%40 \n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%60:1 = eq %30:4 %10:4\n",
        "%61:1 = mux %60 %20 0\n",
        "%70:4 = mux %61 %0:4 %50:4\n",
        "%80:1 = eq %30:4 %14:4\n",
        "%81:1 = mux %80 %21 0\n",
        "%90:4 = mux %81 %4:4 %70:4\n",
        "%100:4 = dff %90:4 clk=%40 clr=%42,1111 rst=%43,0000 en=%22 rst>en init=1010\n",
        "%110:0 = output \"rd\" %100:4\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_trans_wide_write() {
    let mut design = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:8 = input \"we\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:4 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:8 mask=%20:8 clk=%40 \n",
        "    read addr=%30:4 width=#4 clk=%40 [trans]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:4\n",
    ))
    .unwrap();
    unmap_read_dffs(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:8 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:8 = input \"we\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%50:4 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:8 mask=%20:8 clk=%40 \n",
        "    read addr=%30:4 width=#4\n",
        "}\n",
        "%60:8 = ushr %0:8 %30+0 #4\n",
        "%70:8 = ushr %20:8 %30+0 #4\n",
        "%80:1 = eq [ 0 %30+1:3 ] %10:4\n",
        "%81:4 = mux %80 %70:4 0000\n",
        "%90:1 = mux %81+0 %60+0 %50+0\n",
        "%91:1 = mux %81+1 %60+1 %50+1\n",
        "%92:1 = mux %81+2 %60+2 %50+2\n",
        "%93:1 = mux %81+3 %60+3 %50+3\n",
        "%100:4 = dff [ %93 %92 %91 %90 ] clk=%40\n",
        "%110:0 = output \"rd\" %100:4\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_trans_wide_read() {
    let mut design = Design::from_str(concat!(
        "%0:4 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:4 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:8 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20:4 clk=%40 \n",
        "    read addr=%30:4 width=#8 clk=%40 [trans]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:8\n",
    ))
    .unwrap();
    unmap_read_dffs(&mut design);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:4 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:8 = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20:4 clk=%40\n",
        "    read addr=%30:4 width=#8\n",
        "}\n",
        "%60:8 = shl [ 0000 %0:4 ] %10+0 #4\n",
        "%70:8 = shl [ 0000 %20:4 ] %10+0 #4\n",
        "%80:1 = eq %30:4 [ 0 %10+1:3 ]\n",
        "%81:8 = mux %80 %70:8 00000000\n",
        "%90:1 = mux %81 %60+0 %50+0\n",
        "%91:1 = mux %81+1 %60+1 %50+1\n",
        "%92:1 = mux %81+2 %60+2 %50+2\n",
        "%93:1 = mux %81+3 %60+3 %50+3\n",
        "%94:1 = mux %81+4 %60+4 %50+4\n",
        "%95:1 = mux %81+5 %60+5 %50+5\n",
        "%96:1 = mux %81+6 %60+6 %50+6\n",
        "%97:1 = mux %81+7 %60+7 %50+7\n",
        "%100:8 = dff [ %97 %96 %95 %94 %93 %92 %91 %90 ] clk=%40\n",
        "%110:0 = output \"rd\" %100:8\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
