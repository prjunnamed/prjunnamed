use std::{collections::BTreeMap, error::Error, fs::File, io::Write, sync::Arc};

use prjunnamed_netlist::{Design, Target};

fn process(design: &mut Design) {
    match design.target() {
        None => {
            prjunnamed_generic::decision(design);
            prjunnamed_generic::canonicalize(design);
            prjunnamed_generic::lower_arith(design);
            prjunnamed_generic::canonicalize(design);
        }
        Some(ref target) => {
            prjunnamed_generic::unname(design);
            target.synthesize(design).expect("synthesis failed")
        }
    }
}

fn read_input(target: Option<Arc<dyn Target>>, name: String) -> Result<Design, Box<dyn Error>> {
    if name.ends_with(".uir") {
        Ok(prjunnamed_netlist::parse(target, &std::fs::read_to_string(name)?)?)
    } else if name.ends_with(".json") {
        let designs = prjunnamed_yosys_json::import(target, &mut File::open(name)?)?;
        assert_eq!(designs.len(), 1, "can only convert single-module Yosys JSON to Unnamed IR");
        Ok(designs.into_values().next().unwrap())
    } else if name.is_empty() {
        panic!("no input provided")
    } else {
        panic!("don't know what to do with input {name:?}")
    }
}

fn write_output(design: Design, name: String) -> Result<(), Box<dyn Error>> {
    if name.ends_with(".uir") {
        write!(&mut File::create(name)?, "{design}")?;
    } else if name.ends_with(".json") {
        let designs = BTreeMap::from([("top".to_owned(), design)]);
        prjunnamed_yosys_json::export(&mut File::create(name)?, designs)?;
    } else if name.is_empty() {
        print!("{design}");
        println!("; cell counts:");
        for (class, amount) in design.statistics() {
            println!("; {:>7} {}", amount, class);
        }
    } else {
        panic!("don't know what to do with output {name:?}")
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut version = false;
    let mut input = String::new();
    let mut output = String::new();
    let mut target = None::<String>;
    {
        let mut parser = argparse::ArgumentParser::new();
        parser.refer(&mut version).add_option(&["--version"], argparse::StoreTrue, "Display version");
        parser.refer(&mut target).add_option(&["-t", "--target"], argparse::StoreOption, "Target platform");
        parser.refer(&mut input).add_argument("INPUT", argparse::Store, "Input file");
        parser.refer(&mut output).add_argument("OUTPUT", argparse::Store, "Output file");
        parser.parse_args_or_exit();
    }

    if version {
        println!("prjunnamed git-{}", env!("GIT_HASH"));
        return Ok(());
    }

    let target = match target {
        Some(name) => Some(prjunnamed_netlist::create_target(name.as_str(), BTreeMap::new())?),
        None => None,
    };

    let mut design = read_input(target, input)?;
    if let Some(target) = design.target() {
        target.import(&mut design)?;
    }
    process(&mut design);
    write_output(design, output)?;
    Ok(())
}

fn main() {
    env_logger::init();
    prjunnamed_siliconblue::register();
    if let Err(error) = run() {
        eprintln!("error: {}", error);
        std::process::exit(1)
    }
}
