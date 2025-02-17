// TODO
use std::str::FromStr;

use prjunnamed_memory::MemoryExt;
use prjunnamed_netlist::{assert_isomorphic, Cell, Design};

fn unmap_stuff(design: &mut Design, include_transparency: bool) {
    for cell_ref in design.iter_cells() {
        if let Cell::Memory(memory) = &*cell_ref.get() {
            let mut output = cell_ref.output();
            let mut memory = memory.clone();
            cell_ref.unalive();
            for port_index in 0..memory.read_ports.len() {
                memory.unmap_read_init_reset_transparency(design, port_index, include_transparency, &mut output);
            }
            design.replace_value(output, design.add_memory(memory));
        }
    }
    design.apply();
}

#[test]
fn test_noop() {
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
        "%50:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%40 \n",
        "    %50:4 = read addr=%30:4 clk=%41 en=%21 [undef]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:4\n",
    ))
    .unwrap();
    let mut gold = design.clone();
    unmap_stuff(&mut design, false);
    assert_isomorphic!(design, gold);
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
        "%50:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%40 \n",
        "    %50:4 = read addr=%30:4 clk=%41 clr=%42,1111 rst=%43,0000 en=%21 rst>en init=1010 [undef]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:4\n",
    ))
    .unwrap();
    unmap_stuff(&mut design, false);
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
        "%50:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20*4 clk=%40\n",
        "    %50:4 = read addr=%30:4 clk=%41 en=%21 [undef]\n",
        "}\n",
        "%60:1 = dff 0 clk=%41 clr=%42,1 rst=%43,1 en=%21 rst>en init=1\n",
        "%70:4 = dff XXXX clk=%41 clr=%42,1111 rst=%43,0000 en=%21 rst>en init=1010\n",
        "%80:4 = mux %60 %70:4 %50:4\n",
        "%90:0 = output \"rd\" %80:4\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}

#[test]
fn test_trans() {
    let mut design = Design::from_str(concat!(
        "%0:4 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:4 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20:4 clk=%40 \n",
        "    %50:4 = read addr=%30:4 clk=%40 clr=%42,1111 rst=%43,0000 en=%21 en>rst init=1010 [trans]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:4\n",
    ))
    .unwrap();
    unmap_stuff(&mut design, true);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"wd\"\n",
        "%10:4 = input \"wa\"\n",
        "%20:4 = input \"we\"\n",
        "%21:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20:4 clk=%40\n",
        "    %50:4 = read addr=%30:4 clk=%40 en=%21 [undef]\n",
        "}\n",
        "%60:1 = eq %30:4 %10:4\n",
        "%61:4 = mux %60 %20:4 0000\n",
        "%70:1 = mux %61+0 %0+0 X\n",
        "%71:1 = mux %61+1 %0+1 X\n",
        "%72:1 = mux %61+2 %0+2 X\n",
        "%73:1 = mux %61+3 %0+3 X\n",
        "%80:4 = or 0000 %20:4\n",
        "%90:4 = mux %60 %80:4 0000\n",
        "%100:4 = dff %90:4 clk=%40 clr=%42,1111 rst=%43,1111 en=%21 en>rst init=1111\n",
        "%110:4 = dff [ %73 %72 %71 %70 ] clk=%40 clr=%42,1111 rst=%43,0000 en=%21 en>rst init=1010\n",
        "%120:1 = mux %100+0 %110+0 %50+0\n",
        "%121:1 = mux %100+1 %110+1 %50+1\n",
        "%122:1 = mux %100+2 %110+2 %50+2\n",
        "%123:1 = mux %100+3 %110+3 %50+3\n",
        "%130:0 = output \"rd\" [ %123 %122 %121 %120 ]\n",
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
        "%20:4 = input \"w0e\"\n",
        "%24:4 = input \"w1e\"\n",
        "%28:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20:4 clk=%40 \n",
        "    write addr=%14:4 data=%4:4 mask=%24:4 clk=%40 \n",
        "    %50:4 = read addr=%30:4 clk=%40 clr=%42,1111 rst=%43,0000 en=%28 en>rst init=1010 [trans trans]\n",
        "}\n",
        "%60:0 = output \"rd\" %50:4\n",
    ))
    .unwrap();
    unmap_stuff(&mut design, true);
    let mut gold = Design::from_str(concat!(
        "%0:4 = input \"w0d\"\n",
        "%4:4 = input \"w1d\"\n",
        "%10:4 = input \"w0a\"\n",
        "%14:4 = input \"w1a\"\n",
        "%20:4 = input \"w0e\"\n",
        "%24:4 = input \"w1e\"\n",
        "%28:1 = input \"re\"\n",
        "%30:4 = input \"ra\"\n",
        "%40:1 = input \"clk\"\n",
        "%42:1 = input \"rclr\"\n",
        "%43:1 = input \"rrst\"\n",
        "%50:_ = memory depth=#16 width=#4 {\n",
        "    init 0101\n",
        "    write addr=%10:4 data=%0:4 mask=%20:4 clk=%40 \n",
        "    write addr=%14:4 data=%4:4 mask=%24:4 clk=%40 \n",
        "    %50:4 = read addr=%30:4 clk=%40 en=%28 [undef undef]\n",
        "}\n",
        "%60:1 = eq %30:4 %10:4\n",
        "%61:4 = mux %60 %20:4 0000\n",
        "%70:1 = mux %61+0 %0+0 X\n",
        "%71:1 = mux %61+1 %0+1 X\n",
        "%72:1 = mux %61+2 %0+2 X\n",
        "%73:1 = mux %61+3 %0+3 X\n",
        "%80:4 = or 0000 %20:4\n",
        "%90:4 = mux %60 %80:4 0000\n",
        "%100:1 = eq %30:4 %14:4\n",
        "%101:4 = mux %100 %24:4 0000\n",
        "%110:1 = mux %101+0 %4+0 %70\n",
        "%111:1 = mux %101+1 %4+1 %71\n",
        "%112:1 = mux %101+2 %4+2 %72\n",
        "%113:1 = mux %101+3 %4+3 %73\n",
        "%120:4 = or %90:4 %24:4\n",
        "%130:4 = mux %100 %120:4 %90:4\n",
        "%140:4 = dff %130:4 clk=%40 clr=%42,1111 rst=%43,1111 en=%28 en>rst init=1111\n",
        "%150:4 = dff [ %113 %112 %111 %110 ] clk=%40 clr=%42,1111 rst=%43,0000 en=%28 en>rst init=1010\n",
        "%160:1 = mux %140+0 %150+0 %50+0\n",
        "%161:1 = mux %140+1 %150+1 %50+1\n",
        "%162:1 = mux %140+2 %150+2 %50+2\n",
        "%163:1 = mux %140+3 %150+3 %50+3\n",
        "%170:0 = output \"rd\" [ %163 %162 %161 %160 ]\n",
    ))
    .unwrap();
    assert_isomorphic!(design, gold);
}
