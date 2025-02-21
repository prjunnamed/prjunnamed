use std::collections::BTreeSet;
use std::io::Write;
use std::io;
use std::fmt;
use prjunnamed_netlist::Design;

struct Node {
    index: usize,
    label: String,
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "node_{} [shape=rect label=\"{}\"];", self.index, self.label.escape_default())
    }
}

pub fn describe(writer: &mut impl Write, design: &Design) -> io::Result<()> {
    writeln!(writer, "digraph {{")?;
    writeln!(writer, "  rankdir=LR;")?;
    for cell in design.iter_cells_topo() {
        let index = cell.debug_index();
        let node = Node {
            index,
            label: design.display_cell(cell).to_string(),
        };

        writeln!(writer, "  {node}")?;

        let mut inputs = BTreeSet::new();
        cell.visit(|net| {
            if let Ok((cell, _index)) = design.find_cell(net) {
                inputs.insert(cell.debug_index());
            }
        });

        for input in inputs {
            writeln!(writer, "  node_{input} -> node_{index};")?;
        }
    }

    writeln!(writer, "}}")
}
