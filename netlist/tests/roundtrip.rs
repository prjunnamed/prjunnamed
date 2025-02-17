use std::{collections::BTreeMap, sync::Arc};

use prjunnamed_netlist::{Const, Design, Target, TargetCell, TargetImportError, TargetPrototype, register_target};
use prjunnamed_netlist::parse;

#[track_caller]
fn onewaytrip(text: &str, expect: &str) {
    let design = match parse(None, text) {
        Ok(design) => design,
        Err(error) => panic!("{}", error),
    };
    assert_eq!(format!("{:#}", design), format!("{}", expect))
}

#[track_caller]
fn roundtrip(text: &str) {
    onewaytrip(text, text); // one way both ways
}

#[test]
fn test_empty() {
    parse(None, "").unwrap();
    parse(None, "\n").unwrap();
    parse(None, "\n  ").unwrap();
}

#[test]
fn test_comment() {
    parse(None, ";\n").unwrap();
    parse(None, "; foo\n").unwrap();
    parse(None, "  ; meow\n").unwrap();
    let design = parse(None, "; meow\n%0:1 = buf 0\n").unwrap();
    assert_eq!(design.iter_cells().count(), 1);
}

#[test]
fn test_metadata() {
    roundtrip("!1 = source \"top.py\" (#1 #2) (#3 #4)\n");
    roundtrip("!1 = scope \"top\"\n");
    roundtrip("!1 = source \"top.py\" (#1 #2) (#3 #4)\n!2 = scope \"top\" src=!1\n");
    roundtrip("!1 = scope \"top\"\n!2 = scope \"cpu\" in=!1\n");
    roundtrip("!1 = scope \"io\"\n!2 = scope #1 in=!1\n");
    roundtrip("!1 = scope \"top\"\n!2 = ident \"addr\" in=!1\n");
    roundtrip("!1 = scope \"top\"\n!2 = ident \"addr\" in=!1\n!3 = { !1 !2 }\n");
    roundtrip("!1 = attr \"foo\" 1101\n");
    roundtrip("!1 = attr \"foo\" #1\n");
    roundtrip("!1 = attr \"foo\" \"bar\"\n");
}

#[test]
fn test_const() {
    roundtrip("%0:1 = buf 0\n");
    roundtrip("%0:1 = buf 1\n");
    roundtrip("%0:1 = buf X\n");
    roundtrip("%0:2 = buf 10\n");
    roundtrip("%0:2 = buf 1X\n");
    roundtrip("%0:3 = buf 01X\n");
}

#[test]
fn test_concat() {
    roundtrip("%0:0 = buf []\n");
    onewaytrip("%0:1 = buf [ 0 ]\n", "%0:1 = buf 0\n");
    onewaytrip("%0:2 = buf [ 1 0 ]\n", "%0:2 = buf 10\n");
    onewaytrip("%0:1 = buf X\n%1:3 = buf [ 1 0 %0+0 ]\n", "%0:1 = buf X\n%1:3 = buf [ 10 %0 ]\n");
    onewaytrip("%0:4 = buf XXXX\n%4:4 = buf [ %0+3 %0+2 %0+3 %0+2 ]\n", "%0:4 = buf XXXX\n%4:4 = buf %0+2:2*2\n");
    onewaytrip("%0:3 = buf 000\n%3:6 = buf [ %0:3 %0+1:2 %0+0 ]\n", "%0:3 = buf 000\n%3:6 = buf %0:3*2\n");
}

#[test]
fn test_reference() {
    roundtrip("%0:1 = buf 0\n%1:1 = buf %0\n");
    roundtrip("%0:2 = buf 00\n%2:1 = buf %0+0\n");
    roundtrip("%0:2 = buf 00\n%2:1 = buf %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:2 = buf %0:2\n");
    roundtrip("%0:2 = buf 00\n%2:2 = buf [ %0+0 %0+1 ]\n");
    roundtrip("%0:4 = buf 0000\n%4:4 = buf %0+0:2*2\n");
    roundtrip("%0:3 = buf 000\n%3:6 = buf [ %0:3 %0+1 %0+1:2 ]\n");
    roundtrip("%0:2 = buf 00\n%2:8 = buf %0:2*4\n");
}

#[test]
fn test_escaping() {
    roundtrip("&\"foo bar\":1\n");
    roundtrip("&\"foo\\22\":1\n");
    roundtrip("&\"foo\\7f\":1\n");
}

#[test]
fn test_ios() {
    roundtrip("&\"some\":1\n");
}

#[test]
fn test_cells() {
    roundtrip("%0:1 = buf 0\n%1:1 = buf %0\n");
    roundtrip("%0:2 = buf 00\n%2:1 = and %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = or %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = xor %0+0 %0+1\n");
    roundtrip("%0:3 = buf 000\n%3:1 = mux %0+0 %0+1 %0+2\n");
    roundtrip("%0:3 = buf 000\n%3:2 = adc %0+0 %0+1 %0+2\n");
    roundtrip("%0:2 = buf 00\n%2:1 = eq %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = ult %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = slt %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = shl %0+0 %0+1 #1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = ushr %0+0 %0+1 #2\n");
    roundtrip("%0:2 = buf 00\n%2:1 = sshr %0+0 %0+1 #3\n");
    roundtrip("%0:2 = buf 00\n%2:1 = xshr %0+0 %0+1 #4\n");
    roundtrip("%0:2 = buf 00\n%2:1 = mul %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = udiv %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = umod %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = sdiv_trunc %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = sdiv_floor %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = smod_trunc %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = smod_floor %0+0 %0+1\n");
    roundtrip("%0:2 = buf 00\n%2:3 = match %0:2 { (00 01) 10 11 }\n");
    roundtrip("%0:2 = buf 00\n%2:2 = match en=%0+1 %0+0 { 0 1 }\n");
    roundtrip("%0:16 = buf 0000000000000000\n%16:8 = match en=%0+1 %0:16 {\n  0000000000000001\n  0000000000000010\n  0000000000000100\n  0000000000001000\n  0000000000010000\n  0000000000100000\n  0000000001000000\n  0000000010000000\n}\n");
    roundtrip("%0:4 = buf 0000\n%4:2 = assign %0+0:2 %0+2:2\n");
    roundtrip("%0:4 = buf 0000\n%4:2 = assign en=%0+2 %0+0:2 %0+3 at=#1\n");
    roundtrip("%0:2 = buf 00\n%2:1 = dff %0+0 clk=%0+1\n");
    roundtrip("%0:0 = memory depth=#256 width=#16 {\n}\n");
    roundtrip("&\"purr\":1\n%0:2 = buf 00\n%2:1 = iobuf &\"purr\" o=%0+0 en=%0+1\n");
    roundtrip("&\"purr\":2\n%0:2 = buf 00\n%2:2 = iobuf &\"purr\":2 o=%0:2 en=%0+1\n");
    roundtrip("%0:_ = \"instance\" {\n}\n");
    roundtrip("%0:2 = input \"awa\"\n");
    roundtrip("%0:2 = buf 00\n%2:0 = output \"bite\" %0:2\n");
    roundtrip("%0:2 = buf 00\n%2:0 = name \"meow\" %0:2\n");
    roundtrip("%0:2 = buf 00\n%2:0 = debug \"hiss\" %0:2\n");
}

#[test]
fn test_cells_metadata() {
    roundtrip("!1 = source \"top.py\" (#1 #2) (#3 #4)\n%0:1 = buf 0 !1\n");
    roundtrip("!1 = source \"top.py\" (#1 #2) (#3 #4)\n%0:2 = buf 00\n%2:3 = match %0:2 !1 { (00 01) 10 11 }\n");
}

#[test]
fn test_dffs() {
    roundtrip("%0:5 = buf 00000\n%5:1 = dff %0+0 clk=%0+1\n");
    roundtrip("%0:5 = buf 00000\n%5:1 = dff %0+0 clk=%0+1 init=1\n");
    roundtrip("%0:5 = buf 00000\n%5:1 = dff %0+0 clk=%0+1 clr=%0+2\n");
    roundtrip("%0:5 = buf 00000\n%5:1 = dff %0+0 clk=%0+1 rst=%0+2\n");
    roundtrip("%0:5 = buf 00000\n%5:1 = dff %0+0 clk=%0+1 en=%0+2\n");
    roundtrip("%0:5 = buf 00000\n%5:1 = dff %0+0 clk=%0+1 rst=%0+2 en=%0+3 rst>en\n");
    roundtrip("%0:5 = buf 00000\n%5:1 = dff %0+0 clk=%0+1 rst=%0+2 en=!%0+3 en>rst\n");
    roundtrip("%0:5 = buf 00000\n%5:1 = dff %0+0 clk=%0+1 clr=%0+2 rst=%0+3 en=%0+4 en>rst init=1\n");
}

#[test]
fn test_memories() {
    roundtrip("%0:0 = memory depth=#3 width=#4 {\n  init 0001\n  init 0010\n  init 1000\n}\n");
    roundtrip(
        "%0:1 = buf 0\n%1:3 = buf 000\n%4:4 = buf 0000\n\
         %8:0 = memory depth=#8 width=#4 {\n  \
         write addr=%1:3 data=%4:4 clk=%0\n}\n",
    );
    roundtrip(
        "%0:1 = buf 0\n%1:3 = buf 000\n%4:4 = buf 0000\n%8:4 = buf 0001\n\
         %12:0 = memory depth=#8 width=#4 {\n  \
         write addr=%1:3 data=%4:4 mask=%8:4 clk=%0\n}\n",
    );
    roundtrip(
        "%0:1 = buf 0\n%1:3 = buf 000\n\
         %4:4 = memory depth=#8 width=#4 {\n  \
         read addr=%1:3 width=#4\n}\n",
    );
    roundtrip(
        "%0:5 = buf 00000\n%5:3 = buf 000\n\
         %8:4 = memory depth=#8 width=#4 {\n  \
         read addr=%5:3 width=#4 clk=%0+1 []\n}\n",
    );
    roundtrip(
        "%0:5 = buf 00000\n%5:3 = buf 000\n\
         %8:4 = memory depth=#8 width=#4 {\n  \
         read addr=%5:3 width=#4 clk=%0+1 init=1010 []\n}\n",
    );
    roundtrip(
        "%0:5 = buf 00000\n%5:3 = buf 000\n\
         %8:4 = memory depth=#8 width=#4 {\n  \
         read addr=%5:3 width=#4 clk=%0+1 clr=%0+2 []\n}\n",
    );
    roundtrip(
        "%0:5 = buf 00000\n%5:3 = buf 000\n\
         %8:4 = memory depth=#8 width=#4 {\n  \
         read addr=%5:3 width=#4 clk=%0+1 rst=%0+2 []\n}\n",
    );
    roundtrip(
        "%0:5 = buf 00000\n%5:3 = buf 000\n\
         %8:4 = memory depth=#8 width=#4 {\n  \
         read addr=%5:3 width=#4 clk=%0+1 en=%0+2 []\n}\n",
    );
    roundtrip(
        "%0:5 = buf 00000\n%5:3 = buf 000\n\
         %8:4 = memory depth=#8 width=#4 {\n  \
         read addr=%5:3 width=#4 clk=%0+1 rst=%0+2 en=%0+3 rst>en []\n}\n",
    );
    roundtrip(
        "%0:5 = buf 00000\n%5:3 = buf 000\n\
         %8:4 = memory depth=#8 width=#4 {\n  \
         read addr=%5:3 width=#4 clk=%0+1 rst=%0+2 en=!%0+3 en>rst []\n}\n",
    );
    roundtrip(
        "%0:5 = buf 00000\n%5:3 = buf 000\n\
         %8:4 = memory depth=#8 width=#4 {\n  \
         read addr=%5:3 width=#4 clk=%0+1 clr=%0+2 rst=%0+3 en=%0+4 en>rst init=1010 []\n}\n",
    );
    roundtrip(
        "%0:1 = buf 0\n%1:3 = buf 000\n%4:4 = buf 0000\n%8:4 = buf 0001\n\
         %12:4 = memory depth=#8 width=#4 {\n  \
         write addr=%1:3 data=%4:4 mask=%8:4 clk=%0\n  \
         read addr=%1:3 width=#4 clk=%0 [undef]\n}\n",
    );
    roundtrip(
        "%0:1 = buf 0\n%1:3 = buf 000\n%4:4 = buf 0000\n%8:4 = buf 0001\n\
         %12:4 = memory depth=#8 width=#4 {\n  \
         write addr=%1:3 data=%4:4 mask=%8:4 clk=%0\n  \
         write addr=%1:3 data=%4:4 mask=%8:4 clk=%0\n  \
         read addr=%1:3 width=#4 clk=%0 [trans rdfirst]\n}\n",
    );
}

#[test]
fn test_memories_metadata() {
    roundtrip("!1 = source \"op.py\" (#1 #2) (#3 #4)\n%0:0 = memory depth=#1 width=#1 !1 {\n  init 1\n}\n");
}

#[test]
fn test_instances() {
    roundtrip("%0:1 = buf 0\n%1:_ = \"TBUF\" {\n  input \"EN\" = %0\n}\n");
    roundtrip("%2:_ = \"TBUF\" {\n  %2:2 = output \"I\"\n}\n");
    roundtrip("&\"pin\":1\n%0:_ = \"TBUF\" {\n  io \"PIN\" = &\"pin\"\n}\n");
    roundtrip("%0:_ = \"TBUF\" {\n  io \"PIN\" = &_\n}\n");
    roundtrip("%0:_ = \"TBUF\" {\n  io \"PIN\" = &_:4\n}\n");
    roundtrip(concat!(
        "%2:_ = \"DFF\" {\n",
        "  %2:1 = output \"Q\"\n",
        "  %3:1 = output \"QN\"\n}\n",
        "%4:0 = output \"q\" %2\n",
        "%5:0 = output \"qn\" %3\n"
    ));
}

#[test]
fn test_instances_metadata() {
    roundtrip("!1 = source \"op.py\" (#1 #2) (#3 #4)\n%0:1 = buf 0\n%1:_ = \"TBUF\" !1 {\n  input \"EN\" = %0\n}\n");
}

#[test]
fn test_instance_params() {
    roundtrip("%0:_ = \"CONFIG\" {\n  param \"A\" = 10X\n}\n");
    roundtrip("%0:_ = \"CONFIG\" {\n  param \"A\" = #15\n}\n");
    roundtrip("%0:_ = \"CONFIG\" {\n  param \"A\" = #-33\n}\n");
    roundtrip("%0:_ = \"CONFIG\" {\n  param \"A\" = \"x\"\n}\n");
    roundtrip("%0:_ = \"CONFIG\" {\n  param \"A\" = \"x\\7f\"\n}\n");
}

#[derive(Debug)]
struct TestTarget {
    options: BTreeMap<String, String>,
    prototypes: BTreeMap<String, TargetPrototype>,
}

impl TestTarget {
    fn new(options: BTreeMap<String, String>) -> Arc<Self> {
        Arc::new(TestTarget {
            options,
            prototypes: BTreeMap::from([
                ("BUF".into(), TargetPrototype::new_pure().add_input("A", Const::undef(1)).add_output("Q", 1)),
                (
                    "QUAD_IOBUF".into(),
                    TargetPrototype::new_has_effects()
                        .add_param_bool("OE_INVERT", false)
                        .add_param_bits("PULLUP", Const::zero(4))
                        .add_input("O", Const::undef(4))
                        .add_input_invertible("OE", Const::zero(1), "OE_INVERT")
                        .add_output("I", 4)
                        .add_io("IO", 4),
                ),
                (
                    "ADD".into(),
                    TargetPrototype::new_pure()
                        .add_input("A", Const::undef(1))
                        .add_input("B", Const::undef(1))
                        .add_output("O", 1)
                        .add_output("CO", 1),
                ),
            ]),
        })
    }
}

impl Target for TestTarget {
    fn name(&self) -> &str {
        "test"
    }

    fn options(&self) -> BTreeMap<String, String> {
        self.options.clone()
    }

    fn prototype(&self, name: &str) -> Option<&TargetPrototype> {
        self.prototypes.get(name)
    }

    fn validate(&self, _design: &Design, _cell: &TargetCell) {}

    fn import(&self, _design: &mut Design) -> Result<(), TargetImportError> {
        Ok(())
    }

    fn export(&self, _design: &mut Design) {}

    fn synthesize(&self, _design: &mut Design) -> Result<(), ()> {
        Ok(())
    }
}

#[test]
fn test_target() {
    register_target("test", |options| Ok(TestTarget::new(options)));
    roundtrip("set target \"test\" \"device\"=\"example\"\n");
    onewaytrip(
        "set target \"test\"\n%0:4 = target \"QUAD_IOBUF\" {\n}\n",
        concat!(
            "set target \"test\"\n",
            "%0:4 = target \"QUAD_IOBUF\" {\n",
            "  param \"OE_INVERT\" = 0\n",
            "  param \"PULLUP\" = 0000\n",
            "  input \"O\" = XXXX\n",
            "  input \"OE\" = 0\n",
            "  io \"IO\" = &_:4\n",
            "}\n"
        ),
    );
    roundtrip(concat!(
        "set target \"test\"\n",
        "&\"pins\":3\n",
        "%0:4 = input \"O\"\n",
        "%4:4 = target \"QUAD_IOBUF\" {\n",
        "  param \"OE_INVERT\" = 0\n",
        "  param \"PULLUP\" = 1010\n",
        "  input \"O\" = %0:4\n",
        "  input \"OE\" = 1\n",
        "  io \"IO\" = [ &\"pins\"+0 &\"pins\"+1 &\"pins\"+2 &_ ]\n",
        "}\n"
    ));
    roundtrip(concat!(
        "set target \"test\"\n",
        "%0:1 = input \"A\"\n",
        "%1:1 = input \"B\"\n",
        "; drives \"co\"+0\n",
        "; drives \"o\"+0\n",
        "%4:_ = target \"ADD\" {\n",
        "  input \"A\" = %0\n",
        "  input \"B\" = %1\n",
        "  %4:1 = output \"O\"\n",
        "  %5:1 = output \"CO\"\n",
        "}\n",
        "%6:0 = output \"o\" %4\n",
        "%7:0 = output \"co\" %5\n",
    ));
    roundtrip(concat!(
        "set target \"test\"\n",
        "!1 = source \"op.py\" (#1 #2) (#3 #4)\n",
        "%0:1 = input \"A\"\n",
        "%1:1 = target \"BUF\" !1 {\n",
        "  input \"A\" = %0\n",
        "}\n"
    ));
    roundtrip(concat!(
        "set target \"test\"\n",
        "!1 = source \"op.py\" (#1 #2) (#3 #4)\n",
        "%0:1 = input \"A\"\n",
        "%1:1 = input \"B\"\n",
        "%4:_ = target \"ADD\" !1 {\n",
        "  input \"A\" = %0\n",
        "  input \"B\" = %1\n",
        "  %4:1 = output \"O\"\n",
        "  %5:1 = output \"CO\"\n",
        "}\n",
    ));
}
