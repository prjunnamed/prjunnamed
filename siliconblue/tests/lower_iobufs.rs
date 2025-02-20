use std::collections::BTreeMap;

use prjunnamed_netlist::assert_isomorphic;

use prjunnamed_siliconblue::SiliconBlueTarget;

// I hate rustfmt.
macro_rules! parse {
    ($source:expr) => {{
        let target = SiliconBlueTarget::new(BTreeMap::new());
        let design = prjunnamed_netlist::parse(Some(target.clone()), $source).unwrap();
        (target, design)
    }};
}

#[test]
fn test_lower_iobuf_input() {
    let (target, mut design) = parse! {r#"
        &"io":1 = io
        %0:1 = iobuf &"io" o=X en=0
        %1:0 = output "x" %0
    "#};
    target.lower_iobufs(&mut design);
    let (_, mut gold) = parse! {r#"
        &"io":1 = io
        %0:0 = output "x" %1+0
        %1:_ = target "SB_IO" {
            param "PIN_TYPE" = 000001
            param "PULLUP" = 0
            param "NEG_TRIGGER" = 0
            param "IS_GB" = 0
            param "IO_STANDARD" = "SB_LVCMOS"
            input "D_OUT_0" = X
            input "D_OUT_1" = X
            input "OUTPUT_ENABLE" = 0
            input "CLOCK_ENABLE" = 1
            input "INPUT_CLK" = X
            input "OUTPUT_CLK" = X
            input "LATCH_INPUT_VALUE" = X
            %1:1 = output "D_IN_0"
            %2:1 = output "D_IN_1"
            %3:1 = output "GLOBAL_BUFFER_OUTPUT"
            io "PACKAGE_PIN" = &"io"
            io "PACKAGE_PIN_B" = &_
        }
    "#};
    assert_isomorphic!(design, gold);
}

#[test]
fn test_lower_iobuf_output() {
    let (target, mut design) = parse! {r#"
        &"io":1 = io
        %0:1 = iobuf &"io" o=%1 en=1
        %1:1 = input "o"
        %2:0 = output "i" %0
    "#};
    target.lower_iobufs(&mut design);
    let (_, mut gold) = parse! {r#"
        &"io":1 = io
        %0:1 = input "o"
        %1:_ = target "SB_IO" {
            param "PIN_TYPE" = 011001
            param "PULLUP" = 0
            param "NEG_TRIGGER" = 0
            param "IS_GB" = 0
            param "IO_STANDARD" = "SB_LVCMOS"
            input "D_OUT_0" = %0
            input "D_OUT_1" = X
            input "OUTPUT_ENABLE" = 0
            input "CLOCK_ENABLE" = 1
            input "INPUT_CLK" = X
            input "OUTPUT_CLK" = X
            input "LATCH_INPUT_VALUE" = X
            %1:1 = output "D_IN_0"
            %2:1 = output "D_IN_1"
            %3:1 = output "GLOBAL_BUFFER_OUTPUT"
            io "PACKAGE_PIN" = &"io"
            io "PACKAGE_PIN_B" = &_
        }
        %4:0 = output "i" %1
    "#};
    assert_isomorphic!(design, gold);
}

#[test]
fn test_lower_iobuf_tristate() {
    let (target, mut design) = parse! {r#"
        &"io":1 = io
        %0:1 = iobuf &"io" o=%1 en=%2
        %1:1 = input "o"
        %2:1 = input "oe"
        %3:0 = output "i" %0
    "#};
    target.lower_iobufs(&mut design);
    let (_, mut gold) = parse! {r#"
        &"io":1 = io
        %0:1 = input "o"
        %1:1 = input "oe"
        %2:0 = output "i" %3
        %3:_ = target "SB_IO" {
            param "PIN_TYPE" = 101001
            param "PULLUP" = 0
            param "NEG_TRIGGER" = 0
            param "IS_GB" = 0
            param "IO_STANDARD" = "SB_LVCMOS"
            input "D_OUT_0" = %0
            input "D_OUT_1" = X
            input "OUTPUT_ENABLE" = %1
            input "CLOCK_ENABLE" = 1
            input "INPUT_CLK" = X
            input "OUTPUT_CLK" = X
            input "LATCH_INPUT_VALUE" = X
            %3:1 = output "D_IN_0"
            %4:1 = output "D_IN_1"
            %5:1 = output "GLOBAL_BUFFER_OUTPUT"
            io "PACKAGE_PIN" = &"io"
            io "PACKAGE_PIN_B" = &_
        }
    "#};
    assert_isomorphic!(design, gold);
}

#[test]
fn test_lower_iobuf_tristate_inv() {
    let (target, mut design) = parse! {r#"
        &"io":1 = io
        %0:1 = iobuf &"io" o=%1 en=!%2
        %1:1 = input "o"
        %2:1 = input "t"
        %3:0 = output "i" %0
    "#};
    target.lower_iobufs(&mut design);
    let (_, mut gold) = parse! {r#"
        &"io":1 = io
        %0:1 = input "o"
        %1:1 = input "t"
        %2:0 = output "i" %4
        %3:1 = not %1
        %4:_ = target "SB_IO" {
            param "PIN_TYPE" = 101001
            param "PULLUP" = 0
            param "NEG_TRIGGER" = 0
            param "IS_GB" = 0
            param "IO_STANDARD" = "SB_LVCMOS"
            input "D_OUT_0" = %0
            input "D_OUT_1" = X
            input "OUTPUT_ENABLE" = %3
            input "CLOCK_ENABLE" = 1
            input "INPUT_CLK" = X
            input "OUTPUT_CLK" = X
            input "LATCH_INPUT_VALUE" = X
            %4:1 = output "D_IN_0"
            %5:1 = output "D_IN_1"
            %6:1 = output "GLOBAL_BUFFER_OUTPUT"
            io "PACKAGE_PIN" = &"io"
            io "PACKAGE_PIN_B" = &_
        }
    "#};
    assert_isomorphic!(design, gold);
}

#[test]
fn test_lower_iobuf_tristate_wide() {
    let (target, mut design) = parse! {r#"
        &"io":4 = io
        %0:4 = input "o"
        %4:1 = input "oe"
        %5:4 = iobuf &"io":4 o=%0:4 en=%4
        %9:0 = output "i" %5:4
    "#};
    target.lower_iobufs(&mut design);
    let (_, mut gold) = parse! {r#"
        &"io":4 = io
        %0:4 = input "o"
        %4:1 = input "oe"
        %5:0 = output "i" [ %15 %12 %9 %6 ]
        %6:_ = target "SB_IO" {
            param "PIN_TYPE" = 101001
            param "PULLUP" = 0
            param "NEG_TRIGGER" = 0
            param "IS_GB" = 0
            param "IO_STANDARD" = "SB_LVCMOS"
            input "D_OUT_0" = %0+0
            input "D_OUT_1" = X
            input "OUTPUT_ENABLE" = %4
            input "CLOCK_ENABLE" = 1
            input "INPUT_CLK" = X
            input "OUTPUT_CLK" = X
            input "LATCH_INPUT_VALUE" = X
            %6:1 = output "D_IN_0"
            %7:1 = output "D_IN_1"
            %8:1 = output "GLOBAL_BUFFER_OUTPUT"
            io "PACKAGE_PIN" = &"io"+0
            io "PACKAGE_PIN_B" = &_
        }
        %9:_ = target "SB_IO" {
            param "PIN_TYPE" = 101001
            param "PULLUP" = 0
            param "NEG_TRIGGER" = 0
            param "IS_GB" = 0
            param "IO_STANDARD" = "SB_LVCMOS"
            input "D_OUT_0" = %0+1
            input "D_OUT_1" = X
            input "OUTPUT_ENABLE" = %4
            input "CLOCK_ENABLE" = 1
            input "INPUT_CLK" = X
            input "OUTPUT_CLK" = X
            input "LATCH_INPUT_VALUE" = X
            %9:1 = output "D_IN_0"
            %10:1 = output "D_IN_1"
            %11:1 = output "GLOBAL_BUFFER_OUTPUT"
            io "PACKAGE_PIN" = &"io"+1
            io "PACKAGE_PIN_B" = &_
        }
        %12:_ = target "SB_IO" {
            param "PIN_TYPE" = 101001
            param "PULLUP" = 0
            param "NEG_TRIGGER" = 0
            param "IS_GB" = 0
            param "IO_STANDARD" = "SB_LVCMOS"
            input "D_OUT_0" = %0+2
            input "D_OUT_1" = X
            input "OUTPUT_ENABLE" = %4
            input "CLOCK_ENABLE" = 1
            input "INPUT_CLK" = X
            input "OUTPUT_CLK" = X
            input "LATCH_INPUT_VALUE" = X
            %12:1 = output "D_IN_0"
            %13:1 = output "D_IN_1"
            %14:1 = output "GLOBAL_BUFFER_OUTPUT"
            io "PACKAGE_PIN" = &"io"+2
            io "PACKAGE_PIN_B" = &_
        }
        %15:_ = target "SB_IO" {
            param "PIN_TYPE" = 101001
            param "PULLUP" = 0
            param "NEG_TRIGGER" = 0
            param "IS_GB" = 0
            param "IO_STANDARD" = "SB_LVCMOS"
            input "D_OUT_0" = %0+3
            input "D_OUT_1" = X
            input "OUTPUT_ENABLE" = %4
            input "CLOCK_ENABLE" = 1
            input "INPUT_CLK" = X
            input "OUTPUT_CLK" = X
            input "LATCH_INPUT_VALUE" = X
            %15:1 = output "D_IN_0"
            %16:1 = output "D_IN_1"
            %17:1 = output "GLOBAL_BUFFER_OUTPUT"
            io "PACKAGE_PIN" = &"io"+3
            io "PACKAGE_PIN_B" = &_
        }
    "#};
    assert_isomorphic!(design, gold);
}
