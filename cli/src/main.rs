use clap::Parser;
use prjunnamed_ir::model::Design;
use std::{fs::File, path::PathBuf};

#[derive(Parser)]
struct Args {
    in_file: PathBuf,
    out_file: Option<PathBuf>,
    #[arg(short, long)]
    raw: bool,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let s = std::fs::read_to_string(args.in_file)?;
    let design = match Design::parse_text(&s) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "text parsing failed",
            ));
        }
    };
    if let Some(of) = args.out_file {
        let mut f = File::create(of)?;
        design.emit_text(&mut f, args.raw)?;
    } else {
        println!("{design:?}");
    }
    Ok(())
}
