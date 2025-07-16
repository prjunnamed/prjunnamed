use std::{
    collections::BTreeMap,
    error::Error,
    fs::File,
    io::BufWriter,
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
};

use prjunnamed_generic::{chain_rebalance, tree_rebalance};
use prjunnamed_netlist::{Design, Target};

fn process(design: &mut Design) {
    match design.target() {
        None => {
            prjunnamed_generic::decision(design);
            prjunnamed_generic::canonicalize(design);
            design.rewrite(&[
                &prjunnamed_generic::LowerLt,
                &prjunnamed_generic::LowerMul,
                &prjunnamed_generic::LowerShift,
            ]);
            prjunnamed_generic::canonicalize(design);
            design.rewrite(&[
                &prjunnamed_generic::LowerEq,
                &prjunnamed_generic::LowerMux,
                &prjunnamed_generic::SimpleAigOpt,
                &prjunnamed_generic::Normalize,
            ]);
            chain_rebalance(design);
            prjunnamed_generic::canonicalize(design);
            tree_rebalance(design);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputType {
    YosysJson,
    UIR,
    GraphvizDot,
    GraphvizSvg,
}

impl OutputType {
    fn for_filename(name: &str) -> Self {
        if name.ends_with(".uir") || name.is_empty() {
            Self::UIR
        } else if name.ends_with(".json") {
            Self::YosysJson
        } else if name.ends_with(".dot") {
            Self::GraphvizDot
        } else if name.ends_with(".svg") {
            Self::GraphvizSvg
        } else {
            panic!("don't know what to do with output {name:?}");
        }
    }
}

fn write_output(mut design: Design, name: String, export: bool) -> Result<(), Box<dyn Error>> {
    let output_type = OutputType::for_filename(&name);
    let statistics = design.statistics();

    if export || output_type == OutputType::YosysJson {
        if let Some(target) = design.target() {
            target.export(&mut design);
        }
    }

    let output = || -> Result<_, Box<dyn Error>> {
        let output: Box<dyn Write> =
            if name.is_empty() { Box::new(std::io::stdout()) } else { Box::new(File::create(&name)?) };

        Ok(BufWriter::new(output))
    };

    match output_type {
        OutputType::UIR => write!(output()?, "{design}")?,
        OutputType::YosysJson => {
            let designs = BTreeMap::from([("top".to_owned(), design)]);
            prjunnamed_yosys_json::export(&mut output()?, designs)?;
        }
        OutputType::GraphvizDot => {
            prjunnamed_graphviz::describe(&mut output()?, &design)?;
        }
        OutputType::GraphvizSvg => {
            let output: Stdio = if name.is_empty() { std::io::stdout().into() } else { File::create(&name)?.into() };

            let mut dot = Command::new("dot").arg("-Tsvg").stdin(Stdio::piped()).stdout(output).spawn()?;

            {
                let mut stdin = BufWriter::new(dot.stdin.take().unwrap());
                prjunnamed_graphviz::describe(&mut stdin, &design)?;
                stdin.flush()?;
            }

            dot.wait()?;
        }
    }

    eprintln!("cell counts:");
    for (class, amount) in statistics {
        eprintln!("{:>7} {}", amount, class);
    }

    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut version = false;
    let mut input = String::new();
    let mut output = String::new();
    let mut target = None::<String>;
    let mut export = false;
    {
        let mut parser = argparse::ArgumentParser::new();
        parser.refer(&mut version).add_option(&["--version"], argparse::StoreTrue, "Display version");
        parser.refer(&mut target).add_option(&["-t", "--target"], argparse::StoreOption, "Target platform");
        parser.refer(&mut export).add_option(&["-e", "--export"], argparse::StoreTrue, "Export target cells");
        parser.refer(&mut input).required().add_argument("INPUT", argparse::Store, "Input file");
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
    write_output(design, output, export)?;
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
