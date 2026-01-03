//! This library covers the Lattice iCE65 and iCE40 FPGA families (acquired with SiliconBlue).

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use prjunnamed_generic::{chain_rebalance, tree_rebalance};
use prjunnamed_netlist::{
    Cell, Const, ControlNet, Design, Instance, MetaItemRef, Net, ParamValue, Target, TargetCell, TargetImportError, TargetPrototype, Trit, Value
};

use prjunnamed_lut::Lut;

mod memory;

pub fn register() {
    prjunnamed_netlist::register_target("siliconblue", |options| Ok(SiliconBlueTarget::new(options)));
}

#[derive(Debug)]
pub struct SiliconBlueTarget {
    prototypes: BTreeMap<String, TargetPrototype>,
}

pub const SB_LUT4: &str = "SB_LUT4";
pub const SB_LUT4_CARRY: &str = "SB_LUT4_CARRY";
pub const SB_CARRY: &str = "SB_CARRY";
pub const SB_DFF: &str = "SB_DFF";
pub const SB_IO: &str = "SB_IO";
pub const SB_GB: &str = "SB_SB";
pub const SB_RAM40_4K: &str = "SB_RAM40_4K";
pub const SB_PLL40: &str = "SB_PLL40";
pub const SB_MAC16: &str = "SB_MAC16";
pub const SB_SPRAM256KA: &str = "SB_SPRAM256KA";

impl SiliconBlueTarget {
    pub fn new(options: BTreeMap<String, String>) -> Arc<Self> {
        if !options.is_empty() {
            panic!("SiliconBlue target doesn't take options");
        }
        let mut prototypes = BTreeMap::new();
        prototypes.insert(
            SB_LUT4.into(),
            TargetPrototype::new_pure()
                .add_param_bits("LUT_INIT", Const::undef(16))
                .add_input("I", Const::undef(4)) // synthetic; vendor uses separate I0-I3 ports
                .add_output("O", 1),
        );
        prototypes.insert(
            SB_CARRY.into(),
            TargetPrototype::new_pure()
                .add_input("I0", Const::undef(1))
                .add_input("I1", Const::undef(1))
                .add_input("CI", Const::undef(1))
                .add_output("CO", 1),
        );
        prototypes.insert(
            // synthetic; corresponds to SB_LUT4 + SB_CARRY
            SB_LUT4_CARRY.into(),
            TargetPrototype::new_pure()
                .add_param_bits("LUT_INIT", Const::undef(16))
                .add_param_bool("IS_I3_CI", false)
                .add_input("I", Const::undef(4))
                .add_input("CI", Const::undef(1))
                .add_output("O", 1)
                .add_output("CO", 1),
        );
        prototypes.insert(
            // actually corresponds to SB_DFF* vendor cells
            SB_DFF.into(),
            TargetPrototype::new_has_state()
                .add_param_bits("RESET_VALUE", Const::undef(1)) // synthetic
                .add_param_bool("IS_RESET_ASYNC", false) // synthetic
                .add_param_bool("IS_C_INVERTED", false) // synthetic
                .add_input("D", Const::undef(1))
                .add_input_invertible("C", Const::undef(1), "IS_C_INVERTED")
                .add_input("E", Const::ones(1))
                .add_input("R", Const::zero(1))
                .add_output("Q", 1),
        );
        prototypes.insert(
            SB_GB.into(),
            TargetPrototype::new_pure()
                .add_input("I", Const::undef(1)) // synthetic; USER_SIGNAL_TO_GLOBAL_BUFFER
                .add_output("O", 1), // synthetic; GLOBAL_BUFFER_OUTPUT
        );
        prototypes.insert(
            // actually corresponds to several vendor cells:
            // - SB_IO
            // - SB_GB_IO (when IS_GB is set)
            // - SB_IO_DS (when IO_STANDARD is differential)
            // - SB_IO_OD (when IO_STANDARD is OPEN_DRAIN)
            // TODO: SB_IO_I3C
            SB_IO.into(),
            TargetPrototype::new_has_effects()
                .add_param_bits("PIN_TYPE", Const::zero(6))
                .add_param_bool("PULLUP", false)
                .add_param_bits("NEG_TRIGGER", Const::zero(1))
                .add_param_bool("IS_GB", false) // synthetic
                .add_param_string_enum("IO_STANDARD", &[
                    // the situation with IO standards is a bit of a mess.
                    //
                    // all banks of iCE40, all banks of iCE65L01, and banks 0-2 (+ SPI) of other iCE65
                    // can only do two IO standards:
                    //
                    // - SB_LVCMOS: LVCMOS of whatever VCC the device is given; no configurable parameters whatsoever
                    // - SB_LVDS_INPUT (only iCE40, and only on some banks): input-only, using differential comparator
                    //
                    // iCE65L04, iCE65P04, and iCE65L08 bank 3 actually has more interesting IO standards:
                    //
                    // - SB_LVCMOS{12,18,25,33}_*: LVCMOS with several selectable drive strengths
                    // - SB_MDDR*: another name for SB_LVCMOS18_*
                    // - SB_SSTL2_* and SB_SSTL18_*: VREF-based input buffer
                    // - SB_SUBLVDS_INPUT: input-only, differential comparator, 1.8V
                    // - SB_LVDS_INPUT: input-only, differential comparator, 2.5V
                    //
                    // icecube allows the fancy IO standards on all banks of all iCE65 devices, but downcasts
                    // them to SB_LVCMOS or an error when used on bank other than bank 3, or on iCE65L01.
                    // on iCE40, it accepts only SB_LVCMOS and SB_LVDS_INPUT.
                    //
                    // we also add a fake OPEN_DRAIN standard to mark the special open-drain IO (SB_IO_OD) in our
                    // unified model.
                    "SB_LVCMOS",
                    "SB_LVCMOS15_2",
                    "SB_LVCMOS15_4",
                    "SB_LVCMOS18_2",
                    "SB_LVCMOS18_4",
                    "SB_LVCMOS18_8",
                    "SB_LVCMOS18_10",
                    "SB_LVCMOS25_4",
                    "SB_LVCMOS25_8",
                    "SB_LVCMOS25_12",
                    "SB_LVCMOS25_16",
                    "SB_LVCMOS33_8",
                    "SB_SSTL2_CLASS_1",
                    "SB_SSTL2_CLASS_2",
                    "SB_SSTL18_FULL",
                    "SB_SSTL18_HALF",
                    "SB_MDDR2",
                    "SB_MDDR4",
                    "SB_MDDR8",
                    "SB_MDDR10",
                    "SB_SUBLVDS_INPUT",
                    "SB_LVDS_INPUT",
                    "OPEN_DRAIN", // synthetic; used to mark SB_IO_OD
                ])
                // synthetic for SB_IO_OD cell (it has all of these without underscores for some unhinged reason),
                // except for PACKAGE_PIN_B and GLOBAL_BUFFER_OUTPUT which it lacks
                .add_input("D_OUT_0", Const::undef(1))
                .add_input("D_OUT_1", Const::undef(1))
                .add_input("OUTPUT_ENABLE", Const::zero(1))
                .add_input("CLOCK_ENABLE", Const::ones(1))
                .add_input("INPUT_CLK", Const::undef(1))
                .add_input("OUTPUT_CLK", Const::undef(1))
                .add_input("LATCH_INPUT_VALUE", Const::undef(1))
                .add_output("D_IN_0", 1)
                .add_output("D_IN_1", 1)
                .add_output("GLOBAL_BUFFER_OUTPUT", 1)
                .add_io("PACKAGE_PIN", 1)
                .add_io("PACKAGE_PIN_B", 1),
        );
        // TODO: iCE65 RAM (or just represent it as this cell with READ_MODE = WRITE_MODE = 0?)
        prototypes.insert(
            SB_RAM40_4K.into(),
            TargetPrototype::new_has_state()
                .add_param_int_enum("READ_MODE", &[0, 1, 2, 3])
                .add_param_int_enum("WRITE_MODE", &[0, 1, 2, 3])
                .add_param_bits("INIT", Const::undef(4096)) // synthetic; vendor has INIT_0..INIT_F
                .add_param_bool("IS_RCLK_INVERTED", false) // synthetic
                .add_param_bool("IS_WCLK_INVERTED", false) // synthetic
                .add_input_invertible("RCLK", Const::undef(1), "IS_RCLK_INVERTED") // synthetic on *NR cells
                .add_input("RCLKE", Const::undef(1))
                .add_input("RE", Const::undef(1))
                .add_input_invertible("WCLK", Const::undef(1), "IS_WCLK_INVERTED") // synthetic on *NW cells
                .add_input("WCLKE", Const::undef(1))
                .add_input("WE", Const::undef(1))
                .add_input("RADDR", Const::undef(11))
                .add_input("WADDR", Const::undef(11))
                .add_input("WDATA", Const::undef(16))
                .add_input("MASK", Const::undef(16))
                .add_output("RDATA", 16),
        );
        // TODO: iCE65 PLL
        prototypes.insert(
            // actually corresponds to several vendor cells (selected by MODE parameter)
            SB_PLL40.into(),
            TargetPrototype::new_has_state()
                .add_param_string_enum("MODE", &[
                    "SB_PLL40_CORE",
                    "SB_PLL40_PAD",
                    "SB_PLL40_2_PAD",
                    "SB_PLL40_2F_CORE",
                    "SB_PLL40_2F_PAD",
                ]) // synthetic
                .add_param_string_enum("FEEDBACK_PATH", &["SIMPLE", "DELAY", "PHASE_AND_DELAY", "EXTERNAL"])
                .add_param_string_enum("DELAY_ADJUSTMENT_MODE_FEEDBACK", &["FIXED", "DYNAMIC"])
                .add_param_string_enum("DELAY_ADJUSTMENT_MODE_RELATIVE", &["FIXED", "DYNAMIC"])
                .add_param_bits("SHIFTREG_DIV_MODE", Const::zero(2))
                .add_param_bits("FDA_FEEDBACK", Const::zero(4))
                .add_param_bits("FDA_RELATIVE", Const::zero(4))
                .add_param_string_enum("PLLOUT_SELECT_PORTA", &[
                    "GENCLK",
                    "GENCLK_HALF",
                    "SHIFTREG_0deg",
                    "SHIFTREG_90deg",
                ]) // synthetic for SB_PLL40_CORE, SB_PLL40_PAD
                .add_param_string_enum("PLLOUT_SELECT_PORTB", &[
                    "GENCLK",
                    "GENCLK_HALF",
                    "SHIFTREG_0deg",
                    "SHIFTREG_90deg",
                ])
                .add_param_bits("DIVR", Const::zero(4))
                .add_param_bits("DIVF", Const::zero(7))
                .add_param_bits("DIVQ", Const::zero(3))
                .add_param_bits("FILTER_RANGE", Const::zero(3))
                .add_param_bits("ENABLE_ICEGATE_PORTA", Const::zero(1)) // synthetic for SB_PLL40_CORE, SB_PLL40_PAD
                .add_param_bits("ENABLE_ICEGATE_PORTB", Const::zero(1))
                .add_param_bits("TEST_MODE", Const::zero(1))
                .add_input("REFERENCECLK", Const::undef(1)) // SB_PLL40_*CORE only
                .add_input("EXTFEEDBACK", Const::undef(1))
                .add_input("DYNAMICDELAY", Const::undef(8))
                .add_input("BYPASS", Const::undef(1))
                .add_input("RESETB", Const::undef(1))
                .add_input("LATCHINPUTVALUE", Const::undef(1))
                .add_input("SDI", Const::undef(1))
                .add_input("SCLK", Const::undef(1))
                .add_output("PLLOUTCOREA", 1) // synthetic for SB_PLL40_CORE, SB_PLL40_PAD
                .add_output("PLLOUTGLOBALA", 1) // synthetic for SB_PLL40_CORE, SB_PLL40_PAD
                .add_output("PLLOUTCOREB", 1)
                .add_output("PLLOUTGLOBALB", 1)
                .add_output("LOCK", 1)
                .add_output("SDO", 1)
                .add_io("PACKAGEPIN", 1), // SB_PLL40_*PAD only
        );
        prototypes.insert(
            "SB_WARMBOOT".into(),
            TargetPrototype::new_has_effects()
                .add_input("S0", Const::undef(1))
                .add_input("S1", Const::undef(1))
                .add_input("BOOT", Const::zero(1)),
        );
        prototypes.insert(
            SB_MAC16.into(),
            TargetPrototype::new_has_state()
                .add_param_bits("NEG_TRIGGER", Const::zero(1))
                .add_param_bits("A_REG", Const::zero(1))
                .add_param_bits("B_REG", Const::zero(1))
                .add_param_bits("C_REG", Const::zero(1))
                .add_param_bits("D_REG", Const::zero(1))
                .add_param_bits("TOP_8x8_MULT_REG", Const::zero(1))
                .add_param_bits("BOT_8x8_MULT_REG", Const::zero(1))
                .add_param_bits("PIPELINE_16x16_MULT_REG1", Const::zero(1))
                .add_param_bits("PIPELINE_16x16_MULT_REG2", Const::zero(1))
                .add_param_bits("TOPOUTPUT_SELECT", Const::zero(2))
                .add_param_bits("BOTOUTPUT_SELECT", Const::zero(2))
                .add_param_bits("TOPADDSUB_LOWERINPUT", Const::zero(2))
                .add_param_bits("BOTADDSUB_LOWERINPUT", Const::zero(2))
                .add_param_bits("TOPADDSUB_UPPERINPUT", Const::zero(1))
                .add_param_bits("BOTADDSUB_UPPERINPUT", Const::zero(1))
                .add_param_bits("TOPADDSUB_CARRYSELECT", Const::zero(2))
                .add_param_bits("BOTADDSUB_CARRYSELECT", Const::zero(2))
                .add_param_bits("MODE_8x8", Const::zero(1))
                .add_param_bits("A_SIGNED", Const::zero(1))
                .add_param_bits("B_SIGNED", Const::zero(1))
                .add_input("A", Const::undef(16))
                .add_input("B", Const::undef(16))
                .add_input("C", Const::undef(16))
                .add_input("D", Const::undef(16))
                .add_input_invertible("CLK", Const::undef(1), "NEG_TRIGGER")
                .add_input("CE", Const::undef(1))
                .add_input("IRSTTOP", Const::undef(1))
                .add_input("IRSTBOT", Const::undef(1))
                .add_input("ORSTTOP", Const::undef(1))
                .add_input("ORSTBOT", Const::undef(1))
                .add_input("AHOLD", Const::undef(1))
                .add_input("BHOLD", Const::undef(1))
                .add_input("CHOLD", Const::undef(1))
                .add_input("DHOLD", Const::undef(1))
                .add_input("OHOLDTOP", Const::undef(1))
                .add_input("OHOLDBOT", Const::undef(1))
                .add_input("OLOADTOP", Const::undef(1))
                .add_input("OLOADBOT", Const::undef(1))
                .add_input("ADDSUBTOP", Const::undef(1))
                .add_input("ADDSUBBOT", Const::undef(1))
                .add_input("CI", Const::undef(1))
                .add_input("ACCUMCI", Const::undef(1))
                .add_input("SIGNEXTIN", Const::undef(1))
                .add_output("O", 32)
                .add_output("CO", 1)
                .add_output("ACCUMCO", 1)
                .add_output("SIGNEXTOUT", 1),
        );
        prototypes.insert(
            SB_SPRAM256KA.into(),
            TargetPrototype::new_has_state()
                .add_input("ADDRESS", Const::undef(14))
                .add_input("DATAIN", Const::undef(16))
                .add_input("MASKWREN", Const::undef(4))
                .add_input("WREN", Const::undef(1))
                .add_input("CHIPSELECT", Const::undef(1))
                .add_input("CLOCK", Const::undef(1))
                .add_input("STANDBY", Const::undef(1))
                .add_input("SLEEP", Const::undef(1))
                .add_input("POWEROFF", Const::undef(1))
                .add_output("DATAOUT", 16),
        );
        // TODO: SB_I2C, SB_SPI, SB_FILTER_50NS, SB_I2C_FIFO
        // TODO: SB_LSOSC, SB_HSOSC, SB_LFOSC, SB_HFOSC
        // TODO: SB_IR_DRV, SB_RGB_DRV, SB_LED_DRV_CUR, SB_LEDD_IP
        // TODO: SB_BARCODE_DRV, SB_IR400_DRV, SB_IR500_DRV, SB_RGBA_DRV
        // TODO: SB_LEDDA_IP, SB_IR_IP

        Arc::new(SiliconBlueTarget { prototypes })
    }

    pub fn is_ice40(&self) -> bool {
        // TODO: actually parse options and determine if we're ice40 or ice65
        true
    }
}

impl Target for SiliconBlueTarget {
    fn name(&self) -> &str {
        "siliconblue"
    }

    fn options(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn prototype(&self, name: &str) -> Option<&TargetPrototype> {
        self.prototypes.get(name)
    }

    fn validate(&self, _design: &Design, _cell: &TargetCell) {
        // TODO:
        // - SB_IO:
        //   - validate PACKAGE_PIN_B floating if IO not differential
        //   - validate PULLUP off if IO differential or open-drain
        // - SB_PLL40: validate ports / parameters unused according to mode
        // - SB_MAC16: validate parameters
    }

    fn import(&self, design: &mut Design) -> Result<(), TargetImportError> {
        for cell_ref in design.iter_cells() {
            let Cell::Other(instance) = &*cell_ref.get() else { continue };
            let _guard = design.use_metadata_from(&[cell_ref]);
            let orig_kind = &instance.kind[..];
            let mut instance = instance.clone();
            match orig_kind {
                "GND" | "VCC" => {
                    let prototype = TargetPrototype::new_pure().add_output("Y", 1);
                    let (_target_cell, value) = prototype
                        .instance_to_target_cell(design, &instance, cell_ref.output())
                        .map_err(|cause| TargetImportError::new(cell_ref, cause))?;
                    let const_value = match orig_kind {
                        "GND" => Value::zero(1),
                        "VCC" => Value::ones(1),
                        _ => unreachable!(),
                    };
                    design.replace_value(value, const_value);
                    cell_ref.unalive();
                    continue;
                }
                "SB_LUT4" => {
                    let mut input = Value::new();
                    for index in 0..4 {
                        let name = format!("I{index}");
                        if let Some(input_bit) = instance.inputs.remove(&name) {
                            if input_bit.len() != 1 {
                                return Err(TargetImportError::input_size_mismatch(cell_ref, name));
                            }
                            input.extend(input_bit);
                        } else {
                            input.extend([Net::UNDEF]);
                        }
                    }
                    if !input.is_undef() {
                        if instance.inputs.insert("I".into(), input).is_some() {
                            return Err(TargetImportError::unknown_input(cell_ref, "I"));
                        }
                    }
                }
                "SB_DFF" | "SB_DFFR" | "SB_DFFS" | "SB_DFFSR" | "SB_DFFSS" | "SB_DFFE" | "SB_DFFER" | "SB_DFFES"
                | "SB_DFFESR" | "SB_DFFESS" | "SB_DFFN" | "SB_DFFNR" | "SB_DFFNS" | "SB_DFFNSR" | "SB_DFFNSS"
                | "SB_DFFNE" | "SB_DFFNER" | "SB_DFFNES" | "SB_DFFNESR" | "SB_DFFNESS" => {
                    instance.kind = SB_DFF.into();
                    let mut kind = orig_kind.strip_prefix("SB_DFF").unwrap();
                    if let Some(rest) = kind.strip_prefix('N') {
                        instance.add_param("IS_C_INVERTED", true);
                        kind = rest;
                    }
                    if let Some(rest) = kind.strip_prefix('E') {
                        kind = rest;
                    }
                    match kind {
                        "" => (),
                        "R" => {
                            instance.add_param("RESET_VALUE", Const::zero(1));
                            instance.add_param("IS_RESET_ASYNC", true);
                        }
                        "S" => {
                            instance.rename_input("S", "R");
                            instance.add_param("RESET_VALUE", Const::ones(1));
                            instance.add_param("IS_RESET_ASYNC", true);
                        }
                        "SR" => {
                            instance.add_param("RESET_VALUE", Const::zero(1));
                            instance.add_param("IS_RESET_ASYNC", false);
                        }
                        "SS" => {
                            instance.rename_input("S", "R");
                            instance.add_param("RESET_VALUE", Const::ones(1));
                            instance.add_param("IS_RESET_ASYNC", false);
                        }
                        _ => unreachable!(),
                    }
                }
                "SB_GB" => {
                    instance.rename_input("USER_SIGNAL_TO_GLOBAL_BUFFER", "I");
                    instance.rename_output("GLOBAL_BUFFER_OUTPUT", "O");
                }
                "SB_GB_IO" => {
                    instance.kind = SB_IO.into();
                    instance.add_param("IS_GB", true);
                }
                "SB_IO_DS" => {
                    instance.kind = SB_IO.into();
                }
                "SB_IO_OD" => {
                    instance.kind = SB_IO.into();
                    instance.add_param("IO_STANDARD", "OPEN_DRAIN");
                    instance.rename_input("DOUT0", "D_OUT_0");
                    instance.rename_input("DOUT1", "D_OUT_1");
                    instance.rename_input("OUTPUTENABLE", "OUTPUT_ENABLE");
                    instance.rename_input("INPUTCLK", "INPUT_CLK");
                    instance.rename_input("OUTPUTCLK", "OUTPUT_CLK");
                    instance.rename_input("CLOCKENABLE", "CLOCK_ENABLE");
                    instance.rename_input("LATCHINPUTVALUE", "LATCH_INPUT_VALUE");
                    instance.rename_output("DIN0", "D_IN_0");
                    instance.rename_output("DIN1", "D_IN_1");
                    instance.rename_io("PACKAGEPIN", "PACKAGE_PIN");
                }
                // TODO: lower SB_RAM256x16, SB_RAM512x8, SB_RAM1024x4, SB_RAM2048x2 here.
                //
                // you may think that it is an easy task that you can do when you have spare 30 minutes.
                //
                // you are horribly wrong.
                //
                // these cells fear neither God nor Satan.  you get to figure out the weird mapping of *ADDR bits,
                // and the weird mapping of RDATA bits, and the weird mapping of WDATA bits, and also swizzle
                // the entire INIT parameter.  you will end up cursing whatever gods you believe in for not striking
                // you down with a merciful lightning bolt or perhaps burning out your eyes before you ever gazed
                // upon the SB_RAM40 simulation model.
                //
                // you will also have to figure out how to implement output bit swizzling (and resizing) with our
                // raw_import model which unfortunately is not really fit for the task.
                "SB_RAM40_4K" | "SB_RAM40_4KNR" | "SB_RAM40_4KNW" | "SB_RAM40_4KNRNW" => {
                    let mut kind = orig_kind.strip_prefix("SB_RAM40_4K").unwrap();
                    if let Some(rest) = kind.strip_prefix("NR") {
                        instance.add_param("IS_RCLK_INVERTED", true);
                        instance.rename_input("RCLKN", "RCLK");
                        kind = rest;
                    }
                    if let Some(rest) = kind.strip_prefix("NW") {
                        instance.add_param("IS_WCLK_INVERTED", true);
                        instance.rename_input("WCLKN", "WCLK");
                        kind = rest;
                    }
                    assert!(kind.is_empty());
                    instance.kind = SB_RAM40_4K.into();
                    let mut init = Const::new();
                    for index in 0..16 {
                        let param_name = format!("INIT_{index:01X}");
                        if let Some(value) = instance.params.remove(&param_name) {
                            let ParamValue::Const(value) = value else {
                                return Err(TargetImportError::parameter_type_mismatch(cell_ref, param_name));
                            };
                            if value.len() != 256 {
                                return Err(TargetImportError::parameter_value_invalid(
                                    cell_ref,
                                    param_name,
                                    ParamValue::Const(value),
                                ));
                            }
                            init.extend(value);
                        } else {
                            init.extend(Const::undef(256));
                        }
                    }
                    instance.add_param("INIT", init);
                }
                "SB_PLL40_CORE" | "SB_PLL40_PAD" | "SB_PLL40_2_PAD" | "SB_PLL40_2F_CORE" | "SB_PLL40_2F_PAD" => {
                    instance.kind = SB_PLL40.into();
                    instance.add_param("MODE", orig_kind);
                    if matches!(orig_kind, "SB_PLL40_CORE" | "SB_PLL40_PAD") {
                        instance.rename_param("PLLOUT_SELECT", "PLLOUT_SELECT_PORTA");
                        instance.rename_param("ENABLE_ICEGATE", "ENABLE_ICEGATE_PORTA");
                        instance.rename_output("PLLOUTCORE", "PLLOUTCOREA");
                        instance.rename_output("PLLOUTGLOBAL", "PLLOUTGLOBALA");
                    }
                }
                _ => {}
            }
            if let Some(prototype) = self.prototypes.get(&instance.kind) {
                if let Some(value) = instance.params.get_mut("IO_STANDARD") {
                    if let ParamValue::String(value) = value {
                        if value.starts_with("SB_LVCMOS") && value != "SB_LVCMOS" {
                            eprintln!("fix your shit please");
                            *value = "SB_LVCMOS".into();
                        }
                    }
                }

                cell_ref.unalive();
                let (target_cell, value) = prototype
                    .instance_to_target_cell(design, &instance, cell_ref.output())
                    .map_err(|cause| TargetImportError::new(cell_ref, cause))?;
                design.replace_value(value, design.add_target(target_cell));
            }
        }
        design.compact();
        Ok(())
    }

    fn export(&self, design: &mut Design) {
        fn take_param_unwrap_const(instance: &mut Instance, name: &str) -> Const {
            match instance.params.remove(name) {
                Some(ParamValue::Const(value)) => value,
                _ => unreachable!(),
            }
        }

        fn take_param_unwrap_string(instance: &mut Instance, name: &str) -> String {
            match instance.params.remove(name) {
                Some(ParamValue::String(value)) => value,
                _ => unreachable!(),
            }
        }

        for cell_ref in design.iter_cells() {
            let Cell::Target(target_cell) = &*cell_ref.get() else { continue };
            let _guard = design.use_metadata_from(&[cell_ref]);
            let prototype = design.target_prototype(target_cell);
            let mut instance = prototype.target_cell_to_instance(target_cell);
            let mut removed_outputs = BTreeSet::new();
            match target_cell.kind.as_str() {
                "SB_LUT4" => {
                    let input = instance.inputs.remove("I").unwrap();
                    for index in 0..4 {
                        instance.inputs.insert(format!("I{index}"), Value::from(input[index]));
                    }
                }
                "SB_DFF" => {
                    if take_param_unwrap_const(&mut instance, "IS_C_INVERTED").is_ones() {
                        instance.kind.push('N');
                    }
                    let enable = instance.inputs.get("E").unwrap();
                    if enable.is_ones() {
                        instance.inputs.remove("E");
                    } else {
                        instance.kind.push('E');
                    }
                    let reset_value = take_param_unwrap_const(&mut instance, "RESET_VALUE");
                    if reset_value.is_undef() {
                        instance.inputs.remove("R");
                    } else {
                        if take_param_unwrap_const(&mut instance, "IS_RESET_ASYNC").is_zero() {
                            instance.kind.push('S');
                        }
                        if reset_value.is_ones() {
                            instance.kind.push('S');
                            instance.rename_input("R", "S");
                        } else {
                            instance.kind.push('R');
                        }
                    }
                }
                "SB_LUT4_CARRY" => {
                    let is_i3_ci = prototype.extract_param_bool(target_cell, "IS_I3_CI");
                    let i = prototype.extract_input(target_cell, "I");
                    let ci = prototype.extract_input(target_cell, "CI");
                    let target_output = cell_ref.output();
                    let mut inst_lut4 = Instance::new("SB_LUT4");
                    inst_lut4.add_param("LUT_INIT", prototype.extract_param(target_cell, "LUT_INIT").clone());
                    inst_lut4.inputs.insert("I0".into(), Value::from(i[0]));
                    inst_lut4.inputs.insert("I1".into(), Value::from(i[1]));
                    inst_lut4.inputs.insert("I2".into(), Value::from(i[2]));
                    inst_lut4.inputs.insert("I3".into(), Value::from(if is_i3_ci { ci.unwrap_net() } else { i[3] }));
                    inst_lut4.outputs.insert("O".into(), 0..1);
                    let o = design.add_other(inst_lut4);
                    design.replace_value(prototype.extract_output(&target_output, "O"), o);
                    let mut inst_carry = Instance::new(SB_CARRY);
                    inst_carry.inputs.insert("I0".into(), Value::from(i[1]));
                    inst_carry.inputs.insert("I1".into(), Value::from(i[2]));
                    inst_carry.inputs.insert("CI".into(), ci);
                    inst_carry.outputs.insert("CO".into(), 0..1);
                    let co = design.add_other(inst_carry);
                    design.replace_value(prototype.extract_output(&target_output, "CO"), co);
                    cell_ref.unalive();
                    continue;
                }
                "SB_GB" => {
                    instance.rename_input("I", "USER_SIGNAL_TO_GLOBAL_BUFFER");
                    instance.rename_output("O", "GLOBAL_BUFFER_OUTPUT");
                }
                "SB_IO" => {
                    match instance.get_param_string("IO_STANDARD").unwrap() {
                        "SB_LVCMOS" | "SB_SSTL2_CLASS_1" | "SB_SSTL2_CLASS_2" | "SB_SSTL18_FULL" | "SB_SSTL18_HALF"
                        | "SB_MDDR10" | "SB_MDDR8" | "SB_MDDR4" | "SB_MDDR2" => {
                            instance.ios.remove("PACKAGE_PIN_B");
                        }
                        "SB_LVDS_INPUT" | "SB_LVDS_OUTPUT" | "SB_LVDS_IO" => {
                            instance.params.remove("PULLUP");
                            instance.kind = "SB_IO_DS".into();
                        }
                        "OPEN_DRAIN" => {
                            instance.params.remove("PULLUP");
                            instance.params.remove("IO_STANDARD");
                            instance.ios.remove("PACKAGE_PIN_B");
                            instance.rename_input("D_OUT_0", "DOUT0");
                            instance.rename_input("D_OUT_1", "DOUT1");
                            instance.rename_input("OUTPUT_ENABLE", "OUTPUTENABLE");
                            instance.rename_input("INPUT_CLK", "INPUTCLK");
                            instance.rename_input("OUTPUT_CLK", "OUTPUTCLK");
                            instance.rename_input("CLOCK_ENABLE", "CLOCKENABLE");
                            instance.rename_input("LATCH_INPUT_VALUE", "LATCHINPUTVALUE");
                            instance.rename_output("D_IN_0", "DIN0");
                            instance.rename_output("D_IN_1", "DIN1");
                            instance.rename_io("PACKAGE_PIN", "PACKAGEPIN");
                            instance.kind = "SB_IO_OD".into();
                        }
                        _ => unreachable!(),
                    }
                    if take_param_unwrap_const(&mut instance, "IS_GB").is_ones() {
                        assert_eq!(instance.kind, "SB_IO");
                        instance.kind = "SB_GB_IO".into();
                    } else {
                        removed_outputs.insert("GLOBAL_BUFFER_OUTPUT");
                    }
                }
                "SB_RAM40_4K" => {
                    for (param, suffix, name_from, name_to) in
                        [("IS_RCLK_INVERTED", "NR", "RCLK", "RCLKN"), ("IS_WCLK_INVERTED", "NW", "WCLK", "WCLKN")]
                    {
                        if take_param_unwrap_const(&mut instance, param).is_ones() {
                            instance.kind.push_str(suffix);
                            instance.rename_input(name_from, name_to);
                        }
                    }
                    let init_value = take_param_unwrap_const(&mut instance, "INIT");
                    for index in 0..16 {
                        instance.add_param(
                            format!("INIT_{index:01X}"),
                            init_value.slice(index * 0x100..(index + 1) * 0x100),
                        );
                    }
                }
                "SB_PLL40" => {
                    let mode = take_param_unwrap_string(&mut instance, "MODE");
                    if matches!(mode.as_str(), "SB_PLL40_CORE" | "SB_PLL40_PAD") {
                        instance.rename_param("PLLOUT_SELECT_PORTA", "PLLOUT_SELECT");
                        instance.rename_param("ENABLE_ICEGATE_PORTA", "ENABLE_ICEGATE");
                        instance.rename_output("PLLOUTCOREA", "PLLOUTCORE");
                        instance.rename_output("PLLOUTGLOBALA", "PLLOUTGLOBAL");
                        instance.params.remove("PLLOUT_SELECT_PORTB");
                        instance.params.remove("ENABLE_ICEGATE_PORTB");
                        removed_outputs.insert("PLLOUTCOREB");
                        removed_outputs.insert("PLLOUTGLOBALB");
                    } else if mode == "SB_PLL40_2_PAD" {
                        instance.params.remove("PLLOUT_SELECT_PORTA");
                    }
                    if mode.ends_with("_CORE") {
                        instance.ios.remove("PACKAGEPIN");
                    } else {
                        instance.inputs.remove("REFERENCECLK");
                    }
                    instance.kind = mode;
                }
                _ => (),
            }
            // perform output reconstruction surgery on the instance
            let mut new_output_map = vec![];
            let mut index = 0;
            instance.outputs.retain(|name, range| {
                if removed_outputs.contains(&name.as_str()) {
                    false
                } else {
                    let orig_range = range.clone();
                    let instance_range = index..index + range.len();
                    *range = instance_range.clone();
                    index = range.end;
                    new_output_map.push((orig_range, instance_range));
                    true
                }
            });
            let instance_output = design.add_other(instance);
            let mut new_output = Value::undef(cell_ref.output_len());
            for (orig_range, instance_range) in new_output_map {
                new_output[orig_range].copy_from_slice(&instance_output[instance_range]);
            }
            design.replace_value(cell_ref.output(), new_output);
            cell_ref.unalive();
        }
        design.compact();
    }

    fn synthesize(&self, design: &mut Design) -> Result<(), ()> {
        prjunnamed_generic::decision(design);
        prjunnamed_generic::canonicalize(design);
        self.lower_memories(design);
        prjunnamed_generic::canonicalize(design);
        design.rewrite(&[&prjunnamed_generic::LowerLt, &prjunnamed_generic::LowerMul, &prjunnamed_generic::LowerShift]);
        prjunnamed_generic::canonicalize(design);
        design.rewrite(&[
            &prjunnamed_generic::Normalize,
            &prjunnamed_generic::LowerEq,
            &prjunnamed_generic::LowerMux,
            &prjunnamed_generic::SimpleAigOpt,
        ]);
        self.lower_ffs(design);
        self.lower_iobufs(design);
        chain_rebalance(design);
        prjunnamed_generic::canonicalize(design);
        tree_rebalance(design);
        self.lower_luts(design);
        prjunnamed_generic::canonicalize(design);
        Ok(())
    }
}

impl SiliconBlueTarget {
    pub fn lower_iobufs(&self, design: &mut Design) {
        let prototype = self.prototype(SB_IO).unwrap();
        for cell_ref in design.iter_cells() {
            let Cell::IoBuf(io_buffer) = &*cell_ref.get() else { continue };
            let _guard = design.use_metadata_from(&[cell_ref]);
            let enable = io_buffer.enable.into_pos(design);
            let mut output_value = Value::new();
            for bit_index in 0..io_buffer.output.len() {
                let mut target_cell = TargetCell::new(SB_IO, prototype);
                if io_buffer.enable.is_always(false) {
                    // no output
                    prototype.apply_param(&mut target_cell, "PIN_TYPE", Const::lit("000001"));
                } else if io_buffer.enable.is_always(true) {
                    // always-on output
                    prototype.apply_input(&mut target_cell, "D_OUT_0", io_buffer.output[bit_index]);
                    prototype.apply_param(&mut target_cell, "PIN_TYPE", Const::lit("011001"));
                } else {
                    // tristate output
                    prototype.apply_input(&mut target_cell, "D_OUT_0", io_buffer.output[bit_index]);
                    prototype.apply_input(&mut target_cell, "OUTPUT_ENABLE", enable);
                    prototype.apply_param(&mut target_cell, "PIN_TYPE", Const::lit("101001"));
                }
                prototype.apply_io(&mut target_cell, "PACKAGE_PIN", io_buffer.io[bit_index]);
                let target_output = design.add_target(target_cell);
                output_value.extend(prototype.extract_output(&target_output, "D_IN_0"));
            }
            design.replace_value(cell_ref.output(), output_value);
            cell_ref.unalive();
        }
        design.compact();
    }

    pub fn lower_ffs(&self, design: &mut Design) {
        let prototype = self.prototype(SB_DFF).unwrap();
        for cell_ref in design.iter_cells() {
            let Cell::Dff(flip_flop) = &*cell_ref.get() else { continue };
            let _guard = design.use_metadata_from(&[cell_ref]);
            let mut flip_flop = flip_flop.clone();
            if !flip_flop.reset.is_always(false) && !flip_flop.clear.is_always(false) {
                flip_flop.unmap_reset(design);
            }
            flip_flop.remap_enable_over_reset(design);
            let output = cell_ref.output();
            let enable = flip_flop.enable.into_pos(design);
            let (is_reset_async, reset) = if !flip_flop.clear.is_always(false) {
                assert!(flip_flop.reset.is_always(false));
                let clear: ControlNet = flip_flop.clear.clone().try_into().expect("clear net");
                (true, clear.into_pos(design))
            } else {
                (false, flip_flop.reset.into_pos(design))
            };

            for index in 0..flip_flop.data.len() {
                let mut ff_slice = flip_flop.slice(index..index + 1);
                let mut output_slice = output.slice(index..index + 1);
                if ff_slice.init_value[0] == Trit::One {
                    output_slice = ff_slice.invert(design, &output_slice);
                }
                let reset_value = if reset == Net::ZERO {
                    Trit::Undef
                } else if is_reset_async {
                    ff_slice.clear_value[0]
                } else {
                    ff_slice.reset_value[0]
                };
                let mut target_cell = TargetCell::new(SB_DFF, prototype);
                prototype.apply_param(&mut target_cell, "RESET_VALUE", reset_value);
                prototype.apply_param(&mut target_cell, "IS_RESET_ASYNC", is_reset_async);
                prototype.apply_param(&mut target_cell, "IS_C_INVERTED", ff_slice.clock.is_negative());
                prototype.apply_input(&mut target_cell, "D", ff_slice.data);
                prototype.apply_input(&mut target_cell, "C", ff_slice.clock.net());
                prototype.apply_input(&mut target_cell, "E", enable);
                prototype.apply_input(&mut target_cell, "R", reset);
                let target_output = design.add_target(target_cell);
                let q = prototype.extract_output(&target_output, "Q");
                design.replace_value(output_slice, q);
            }
            cell_ref.unalive();
        }
        design.compact();
    }

    // a fanfic of https://people.eecs.berkeley.edu/~alanmi/publications/2007/tech07_fast.pdf
    pub fn lower_luts(&self, design: &mut Design) {
        let mut use_count: HashMap<_, u32> = HashMap::new();
        for cell in design.iter_cells() {
            if matches!(&*cell.get(), Cell::Debug(..)) {
                continue;
            }
            cell.visit(|net| *use_count.entry(net).or_default() += 1);
        }

        let mut adc_carries = HashMap::new();
        for cell in design.iter_cells() {
            if let Cell::Adc(_, _, ci) = &*cell.get() {
                let output = cell.output();
                let mut carry = Value::from(ci);
                if output.len() > 1 {
                    carry.extend(design.add_void(output.len() - 2));
                    carry.extend([output.msb()]);
                }
                adc_carries.insert(cell, carry);
            }
        }

        // TODO (later): emit void nets for mux intermediate products

        #[derive(Debug, Clone)]
        enum NetDisposition {
            Lut(u32, Lut),
            LutCarry(u32, Lut, Net, Net),
            CarryOut(u32),
        }

        impl NetDisposition {
            fn depth(&self) -> u32 {
                match *self {
                    NetDisposition::Lut(depth, _) => depth,
                    NetDisposition::LutCarry(depth, _, _, _) => depth,
                    NetDisposition::CarryOut(depth) => depth,
                }
            }
        }

        let mut net_dispositions: BTreeMap<Net, (NetDisposition, MetaItemRef)> = BTreeMap::new();
        for cell in design.iter_cells_topo() {
            match &*cell.get() {
                Cell::Buf(..)
                | Cell::Not(..)
                | Cell::And(..)
                | Cell::Or(..)
                | Cell::Xor(..)
                | Cell::Mux(..)
                | Cell::Aig(..) => {
                    let output = cell.output();
                    'cell_bits: for index in 0..output.len() {
                        let mut lut = if matches!(&*cell.get(), Cell::Aig(..)) {
                            Lut::from_cell(cell.get()).unwrap()
                        } else {
                            Lut::from_cell(cell.get().slice(index..index + 1).unwrap()).unwrap()
                        };
                        let mut meta = cell.metadata();
                        let mut full_merge_lut = lut.clone();
                        let mut full_merge_meta = meta;
                        for net in lut.inputs() {
                            if let Some((NetDisposition::Lut(_depth, input_lut), input_meta)) =
                                net_dispositions.get(&net)
                            {
                                let Some(input_index) = full_merge_lut.inputs().iter().position(|input| input == net)
                                else {
                                    // input disappeared — optimized out by earlier merge?
                                    continue;
                                };
                                full_merge_lut = full_merge_lut.merge(input_index, input_lut);
                                full_merge_meta = full_merge_meta.merge(*input_meta);
                            }
                        }
                        if full_merge_lut.inputs().len() <= 4 {
                            lut = full_merge_lut;
                            meta = full_merge_meta;
                        } else {
                            let mut inputs_by_depth = Vec::from_iter(lut.inputs().iter().map(|net| {
                                (net_dispositions.get(&net).map(|(disp, _meta)| disp.depth()).unwrap_or(0), net)
                            }));

                            inputs_by_depth.sort_by_key(|&(depth, net)| (std::cmp::Reverse(depth), net));
                            for (_, net) in inputs_by_depth {
                                let Some(input_disposition) = net_dispositions.get(&net) else {
                                    continue;
                                };
                                match *input_disposition {
                                    (NetDisposition::CarryOut(_), _) => (),
                                    (NetDisposition::Lut(_depth, ref input_lut), input_meta) => {
                                        let Some(input_index) = lut.inputs().iter().position(|input| input == net)
                                        else {
                                            // input disappeared — optimized out by earlier merge?
                                            continue;
                                        };
                                        let merged_lut = lut.merge(input_index, input_lut);
                                        if merged_lut.inputs().len() <= 4 {
                                            lut = merged_lut;
                                            meta = meta.merge(input_meta);
                                        }
                                    }
                                    (NetDisposition::LutCarry(depth, ref input_lut, ci, co), input_meta) => {
                                        if use_count[&net] != 1 {
                                            continue;
                                        }
                                        let Some(input_index) = lut.inputs().iter().position(|input| input == net)
                                        else {
                                            // input disappeared — optimized out by earlier merge?
                                            continue;
                                        };

                                        if let Some(merged_lut) =
                                            lut.merge(input_index, input_lut).expand_with_fixed(&[
                                                None,
                                                Some(input_lut.inputs()[1]),
                                                Some(input_lut.inputs()[2]),
                                                if input_lut.inputs()[3] == ci {
                                                    Some(input_lut.inputs()[3])
                                                } else {
                                                    None
                                                },
                                            ])
                                        {
                                            net_dispositions.remove(&net);
                                            let depth = (merged_lut
                                                .inputs()
                                                .iter()
                                                .map(|net| {
                                                    net_dispositions
                                                        .get(&net)
                                                        .map(|(disp, _meta)| disp.depth())
                                                        .unwrap_or(0)
                                                })
                                                .max()
                                                .unwrap_or(0)
                                                + 1)
                                            .max(depth);
                                            net_dispositions.insert(
                                                output[index],
                                                (
                                                    NetDisposition::LutCarry(depth, merged_lut, ci, co),
                                                    meta.merge(input_meta),
                                                ),
                                            );
                                            continue 'cell_bits;
                                        }
                                    }
                                }
                            }
                        }
                        let depth = lut
                            .inputs()
                            .iter()
                            .map(|net| net_dispositions.get(&net).map(|(disp, _meta)| disp.depth()).unwrap_or(0))
                            .max()
                            .unwrap_or(0)
                            + 1;
                        net_dispositions.insert(output[index], (NetDisposition::Lut(depth, lut), meta));
                    }
                    cell.unalive();
                }
                Cell::Adc(arg1, arg2, ci) => {
                    // TODO: check if doable in one step
                    let output = cell.output();
                    let carry = &adc_carries[&cell];
                    let mut max_depth = net_dispositions.get(ci).map(|(disp, _meta)| disp.depth()).unwrap_or(0);
                    for index in 0..output.len() - 1 {
                        for net in [arg1[index], arg2[index]] {
                            max_depth = max_depth
                                .max(net_dispositions.get(&net).map(|(disp, _meta)| disp.depth()).unwrap_or(0));
                        }
                        net_dispositions.insert(
                            output[index],
                            (
                                NetDisposition::LutCarry(
                                    max_depth + 1,
                                    Lut::new_fixed(
                                        Value::from_iter([Net::UNDEF, arg1[index], arg2[index], carry[index]]),
                                        Const::lit("1100001100111100"),
                                    ),
                                    carry[index],
                                    carry[index + 1],
                                ),
                                cell.metadata(),
                            ),
                        );
                    }
                    net_dispositions.insert(output.msb(), (NetDisposition::CarryOut(max_depth + 1), cell.metadata()));
                    cell.unalive();
                }
                _ => {}
            }
        }

        let sb_lut4 = self.prototype(SB_LUT4).unwrap();
        let sb_lut4_carry = self.prototype(SB_LUT4_CARRY).unwrap();
        for (net, (disposition, metadata)) in net_dispositions {
            match disposition {
                NetDisposition::CarryOut(_) => {}
                NetDisposition::Lut(_, lut) => {
                    let lut = lut.expand_to(4).unwrap();
                    let mut target_cell = TargetCell::new(SB_LUT4, sb_lut4);
                    sb_lut4.apply_param(&mut target_cell, "LUT_INIT", lut.table());
                    sb_lut4.apply_input(&mut target_cell, "I", lut.inputs());
                    let target_output = design.add_cell_with_metadata_ref(Cell::Target(target_cell), metadata);
                    let o = sb_lut4.extract_output(&target_output, "O");
                    design.replace_value(net, o);
                }
                NetDisposition::LutCarry(_, lut, net_ci, net_co) => {
                    let lut = lut
                        .expand_with_fixed(&[
                            None,
                            Some(lut.inputs()[1]),
                            Some(lut.inputs()[2]),
                            if lut.inputs()[3] == net_ci { Some(lut.inputs()[3]) } else { None },
                        ])
                        .unwrap();
                    let mut target_cell = TargetCell::new(SB_LUT4_CARRY, sb_lut4_carry);
                    sb_lut4_carry.apply_param(&mut target_cell, "LUT_INIT", lut.table());
                    let inputs = lut.inputs();
                    sb_lut4_carry.apply_param(&mut target_cell, "IS_I3_CI", inputs[3] == net_ci);
                    sb_lut4_carry.apply_input(&mut target_cell, "I", inputs);
                    sb_lut4_carry.apply_input(&mut target_cell, "CI", net_ci);
                    let target_output = design.add_cell_with_metadata_ref(Cell::Target(target_cell), metadata);
                    let o = sb_lut4_carry.extract_output(&target_output, "O");
                    design.replace_value(net, o);
                    let co = sb_lut4_carry.extract_output(&target_output, "CO");
                    design.replace_value(net_co, co);
                }
            }
        }

        design.compact();
    }
}
