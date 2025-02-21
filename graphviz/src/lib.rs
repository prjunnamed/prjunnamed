use std::collections::BTreeSet;
use std::fmt::{self, Write};
use std::io;
use prjunnamed_netlist::{Cell, CellRef, ControlNet, Design, Net, Value};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Edge<'a> {
    from_cell: CellRef<'a>,
    to_arg: Option<usize>,
}

impl<'a> From<CellRef<'a>> for Edge<'a> {
    fn from(cell: CellRef<'a>) -> Self {
        Self {
            from_cell: cell,
            to_arg: None,
        }
    }
}

struct Node<'a> {
    design: &'a Design,
    index: usize,
    label: String,
    args: Vec<String>,
    inputs: BTreeSet<Edge<'a>>,
}

impl fmt::Display for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut label = format!("<out> {}", self.label);
        for (i, arg) in self.args.iter().enumerate() {
            write!(&mut label, " | <arg{i}> {arg}")?;
        }

        writeln!(f, "  node_{} [shape=record label=\"{}\"];", self.index, label.escape_default())?;

        for input in &self.inputs {
            let input_index = input.from_cell.debug_index();
            let port = match input.to_arg {
                Some(n) => format!("arg{n}"),
                None => format!("out"),
            };
            writeln!(f, "  node_{}:out -> node_{}:{};", input_index, self.index, port)?;
        }

        Ok(())
    }
}

impl<'a> Node<'a> {
    fn new(design: &'a Design, index: usize, label: String) -> Self {
        Self {
            design,
            index,
            label,
            args: Vec::new(),
            inputs: BTreeSet::new(),
        }
    }

    fn from_name(cell: CellRef<'a>, name: &str) -> Self {
        let index = cell.debug_index();
        let width = cell.output_len();
        let label = format!("%{index}:{width} = {name}");
        Self::new(cell.design(), index, label)
    }

    fn add_input(&mut self, input: impl Into<Edge<'a>>) {
        self.inputs.insert(input.into());
    }

    fn arg(mut self, input: impl ToString) -> Self {
        self.args.push(input.to_string());
        self
    }

    fn net(mut self, input: &Net) -> Self {
        let to_arg = Some(self.args.len());
        if let Ok((cell, _)) = self.design.find_cell(*input) {
            self.add_input(Edge {
                from_cell: cell,
                to_arg,
            });
        }

        let s = self.design.display_net(input).to_string();
        self.arg(s)
    }

    fn value(mut self, input: &Value) -> Self {
        let to_arg = Some(self.args.len());
        input.visit(|net| {
            if let Ok((cell, _)) = self.design.find_cell(net) {
                self.add_input(Edge {
                    from_cell: cell,
                    to_arg,
                });
            }
        });

        let s = self.design.display_value(input).to_string();
        self.arg(s)
    }

    fn control(mut self, name: &str, input: ControlNet, extra: Option<String>) -> Self {
        let to_arg = Some(self.args.len());
        if let Ok((cell, _)) = self.design.find_cell(input.net()) {
            self.add_input(Edge {
                from_cell: cell,
                to_arg,
            });
        }

        let mut s = format!("{name}={}", self.design.display_control_net(input));
        if let Some(extra) = extra {
            write!(&mut s, ",{extra}").unwrap();
        }
        self.arg(s)
    }
}

pub fn describe(writer: &mut impl io::Write, design: &Design) -> io::Result<()> {
    writeln!(writer, "digraph {{")?;
    writeln!(writer, "  rankdir=LR;")?;
    writeln!(writer, "  node [fontname=\"monospace\"];")?;
    for cell in design.iter_cells_topo() {
        let node = match &*cell.get() {
            Cell::Name(_, _) | Cell::Debug(_, _) => continue,
            Cell::Buf(a) => Node::from_name(cell, "buf").value(a),
            Cell::Not(a) => Node::from_name(cell, "not").value(a),
            Cell::And(a, b) => Node::from_name(cell, "and").value(a).value(b),
            Cell::Or(a, b) => Node::from_name(cell, "or").value(a).value(b),
            Cell::Xor(a, b) => Node::from_name(cell, "xor").value(a).value(b),
            Cell::Mux(a, b, c) => Node::from_name(cell, "mux").net(a).value(b).value(c),
            Cell::Adc(a, b, c) => Node::from_name(cell, "adc").value(a).value(b).net(c),
            Cell::Eq(a, b) => Node::from_name(cell, "eq").value(a).value(b),
            Cell::ULt(a, b) => Node::from_name(cell, "ult").value(a).value(b),
            Cell::SLt(a, b) => Node::from_name(cell, "slt").value(a).value(b),
            Cell::Shl(a, b, c) => Node::from_name(cell, "shl").value(a).value(b).arg(c),
            Cell::UShr(a, b, c) => Node::from_name(cell, "ushr").value(a).value(b).arg(c),
            Cell::SShr(a, b, c) => Node::from_name(cell, "sshr").value(a).value(b).arg(c),
            Cell::XShr(a, b, c) => Node::from_name(cell, "xshr").value(a).value(b).arg(c),
            Cell::Mul(a, b) => Node::from_name(cell, "mul").value(a).value(b),
            Cell::UDiv(a, b) => Node::from_name(cell, "udiv").value(a).value(b),
            Cell::UMod(a, b) => Node::from_name(cell, "umod").value(a).value(b),
            Cell::SDivTrunc(a, b) => Node::from_name(cell, "sdiv_trunc").value(a).value(b),
            Cell::SDivFloor(a, b) => Node::from_name(cell, "sdiv_floor").value(a).value(b),
            Cell::SModTrunc(a, b) => Node::from_name(cell, "smod_trunc").value(a).value(b),
            Cell::SModFloor(a, b) => Node::from_name(cell, "smod_floor").value(a).value(b),
            Cell::Dff(flop) => {
                let mut node = Node::from_name(cell, "dff")
                    .value(&flop.data)
                    .control("clk", flop.clock, None);

                if flop.has_clear() {
                    let has_value = flop.clear_value != flop.init_value;
                    node = node.control("clr", flop.clear, has_value.then(|| flop.clear_value.to_string()));
                }

                if flop.has_reset() {
                    let has_value = flop.reset_value != flop.init_value;
                    node = node.control("rst", flop.reset, has_value.then(|| flop.reset_value.to_string()));
                }

                if flop.has_enable() {
                    node = node.control("en", flop.enable, None);
                }

                if flop.has_reset() && flop.has_enable() {
                    if flop.reset_over_enable {
                        node = node.arg("rst/en");
                    } else {
                        node = node.arg("en/rst");
                    }
                }

                if flop.has_init_value() {
                    node = node.arg(format!("init={}", flop.init_value));
                }

                node
            }
            _ => {
                let index = cell.debug_index();
                let label = design.display_cell(cell).to_string();
                let mut node = Node::new(design, index, label);

                cell.visit(|net| {
                    if let Ok((cell, _index)) = design.find_cell(net) {
                        node.add_input(cell);
                    }
                });

                node
            }
        };

        writeln!(writer, "  {node}")?;
    }

    writeln!(writer, "}}")
}
