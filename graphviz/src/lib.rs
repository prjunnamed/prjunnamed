use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::io;
use prjunnamed_netlist::{Cell, CellRef, ControlNet, Design, Net, Value};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Edge<'a> {
    from_cell: CellRef<'a>,
    to_arg: Option<usize>,
}

impl<'a> From<CellRef<'a>> for Edge<'a> {
    fn from(cell: CellRef<'a>) -> Self {
        Self { from_cell: cell, to_arg: None }
    }
}

struct Node<'a> {
    cell: CellRef<'a>,
    label: String,
    args: Vec<String>,
    inputs: BTreeSet<Edge<'a>>,
}

impl<'a> Node<'a> {
    fn new(cell: CellRef<'a>, label: String) -> Self {
        Self { cell, label, args: Vec::new(), inputs: BTreeSet::new() }
    }

    fn from_name(cell: CellRef<'a>, name: &str) -> Self {
        let index = cell.debug_index();
        let width = cell.output_len();
        let label = format!("%{index}:{width} = {name}");
        Self::new(cell, label)
    }

    fn add_input(&mut self, input: impl Into<Edge<'a>>) {
        self.inputs.insert(input.into());
    }

    fn arg(mut self, input: impl ToString) -> Self {
        self.args.push(input.to_string());
        self
    }

    fn net_input(&mut self, net: Net, to_arg: Option<usize>) {
        if let Ok((cell, _)) = self.cell.design().find_cell(net) {
            self.add_input(Edge { from_cell: cell, to_arg });
        }
    }

    fn net(mut self, input: &Net) -> Self {
        let to_arg = Some(self.args.len());
        self.net_input(*input, to_arg);

        let s = self.cell.design().display_net(input).to_string();
        self.arg(s)
    }

    fn value(mut self, input: &Value) -> Self {
        let to_arg = Some(self.args.len());
        for net in input.iter() {
            self.net_input(net, to_arg);
        }

        let s = self.cell.design().display_value(input).to_string();
        self.arg(s)
    }

    fn prefix_value(mut self, prefix: &str, input: &Value) -> Self {
        let to_arg = Some(self.args.len());
        for net in input.iter() {
            self.net_input(net, to_arg);
        }

        let s = format!("{prefix}{}", self.cell.design().display_value(input));
        self.arg(s)
    }

    fn control_net(mut self, input: ControlNet) -> Self {
        let to_arg = Some(self.args.len());
        self.net_input(input.net(), to_arg);

        let s = self.cell.design().display_control_net(input);
        self.arg(s)
    }

    fn control(mut self, name: &str, input: ControlNet, extra: Option<String>) -> Self {
        let to_arg = Some(self.args.len());
        self.net_input(input.net(), to_arg);

        let mut s = format!("{name}={}", self.cell.design().display_control_net(input));
        if let Some(extra) = extra {
            write!(&mut s, ",{extra}").unwrap();
        }
        self.arg(s)
    }
}

struct Context<'a> {
    /// Name that will be used to refer to high-fanout cells
    best_name: BTreeMap<CellRef<'a>, String>,
    fanout: BTreeMap<CellRef<'a>, BTreeSet<CellRef<'a>>>,
    nodes: Vec<Node<'a>>,
}

impl<'a> Context<'a> {
    fn add_node(&mut self, node: Node<'a>) {
        for input in &node.inputs {
            self.fanout.entry(input.from_cell).or_default().insert(node.cell);
        }

        self.nodes.push(node);
    }

    fn high_fanout(&self, cell: CellRef<'_>) -> Option<usize> {
        let fanout = self.fanout.get(&cell).map(BTreeSet::len).unwrap_or(0);
        let threshold = if self.best_name.contains_key(&cell) { 5 } else { 10 };

        if fanout >= threshold { Some(fanout) } else { None }
    }

    fn print(&self, writer: &mut impl io::Write) -> io::Result<()> {
        writeln!(writer, "digraph {{")?;
        writeln!(writer, "  rankdir=LR;")?;
        writeln!(writer, "  node [fontname=\"monospace\"];")?;
        for node in &self.nodes {
            self.print_node(writer, node)?;
        }
        writeln!(writer, "}}")
    }

    fn print_node(&self, writer: &mut impl io::Write, node: &Node<'_>) -> io::Result<()> {
        let force = node.inputs.len() == 1;

        let mut clarify = vec![BTreeSet::new(); node.args.len()];
        for input in &node.inputs {
            if !force && self.high_fanout(input.from_cell).is_some() {
                let Some(name) = self.best_name.get(&input.from_cell) else { continue };
                let Some(arg) = input.to_arg else { continue };
                clarify[arg].insert(name);
            }
        }

        let mut label = format!("<out> {}", node.label);
        for (i, (arg, clarify)) in node.args.iter().zip(clarify).enumerate() {
            write!(&mut label, " | <arg{i}> {arg}").unwrap();
            if !clarify.is_empty() {
                write!(&mut label, " (").unwrap();
                let mut iter = clarify.into_iter();
                write!(&mut label, "{:?}", iter.next().unwrap()).unwrap();
                for input in iter {
                    write!(&mut label, ", {:?}", input).unwrap();
                }
                writeln!(&mut label, ")").unwrap();
            } else if !arg.ends_with('\n') {
                writeln!(&mut label).unwrap();
            }
        }

        let index = node.cell.debug_index();
        let label = label.escape_default().to_string().replace("\\n", "\\l");
        writeln!(writer, "  node_{index} [shape=record label=\"{label}\"];")?;

        for input in &node.inputs {
            if !force && self.high_fanout(input.from_cell).is_some() {
                continue;
            }

            let input_index = input.from_cell.debug_index();
            let port = match input.to_arg {
                Some(n) => format!("arg{n}"),
                None => format!("out"),
            };

            writeln!(writer, "  node_{input_index}:out -> node_{index}:{port};")?;
        }

        if let Some(fanout) = self.high_fanout(node.cell) {
            let mut label = format!("{fanout} uses");
            if let Some(name) = self.best_name.get(&node.cell) {
                write!(&mut label, "\n{name:?}").unwrap();
            }
            writeln!(writer, "  stub_{index} [label=\"{}\"];", label.escape_default())?;
            writeln!(writer, "  node_{index}:out -> stub_{index};")?;
        }

        Ok(())
    }
}

pub fn describe<'a>(writer: &mut impl io::Write, design: &'a Design) -> io::Result<()> {
    // for each cell, a list of name/debug cells that reference it
    let mut names: BTreeMap<CellRef<'_>, BTreeSet<CellRef<'_>>> = BTreeMap::new();
    // for each cell, the shortest name that refers to it
    let mut best_name: BTreeMap<CellRef<'a>, String> = BTreeMap::new();

    let mut consider_name = |cell: CellRef<'a>, name: &str| {
        best_name
            .entry(cell)
            .and_modify(|prev| {
                if prev.len() > name.len() {
                    *prev = name.to_string();
                }
            })
            .or_insert(name.to_string());
    };

    'outer: for cell in design.iter_cells() {
        match &*cell.get() {
            Cell::Name(name, value) | Cell::Debug(name, value) => {
                let mut prev = None;
                for net in value.iter() {
                    let Ok((target, _)) = design.find_cell(net) else { continue 'outer };
                    if let Some(prev) = prev {
                        if prev != target {
                            continue 'outer;
                        }
                    } else {
                        prev = Some(target);
                    }
                }

                let Some(target) = prev else { continue };
                if target.output() == *value {
                    consider_name(target, name);
                }

                for net in value.iter() {
                    if let Ok((target, _)) = design.find_cell(net) {
                        names.entry(target).or_default().insert(cell);
                    }
                }
            }
            Cell::Input(name, _) => {
                consider_name(cell, name);
            }
            _ => {}
        }
    }

    let mut ctx = Context { best_name, fanout: BTreeMap::new(), nodes: vec![] };

    for cell in design.iter_cells_topo() {
        let mut node = match &*cell.get() {
            Cell::Name(_, _) | Cell::Debug(_, _) => continue,
            Cell::Buf(a) => Node::from_name(cell, "buf").value(a),
            Cell::Not(a) => Node::from_name(cell, "not").value(a),
            Cell::And(a, b) => Node::from_name(cell, "and").value(a).value(b),
            Cell::Or(a, b) => Node::from_name(cell, "or").value(a).value(b),
            Cell::Xor(a, b) => Node::from_name(cell, "xor").value(a).value(b),
            Cell::Mux(a, b, c) => Node::from_name(cell, "mux").net(a).value(b).value(c),
            Cell::Adc(a, b, c) => Node::from_name(cell, "adc").value(a).value(b).net(c),
            Cell::Aig(arg1, arg2) => Node::from_name(cell, "aig").control_net(*arg1).control_net(*arg2),
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
            Cell::Output(name, value) => Node::from_name(cell, &format!("output {name:?}")).value(value),

            Cell::Dff(flop) => {
                let mut node = Node::from_name(cell, "dff").value(&flop.data).control("clk", flop.clock, None);

                if flop.has_clear() {
                    let has_value = flop.clear_value != flop.init_value;
                    node = node.control("clr", flop.clear.clone().try_into().expect("cannot graph multi bit control nets yet"), has_value.then(|| flop.clear_value.to_string()));
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
            Cell::Target(target_cell) => {
                let prototype = design.target_prototype(target_cell);
                let mut node = Node::from_name(cell, &format!("target {:?}", target_cell.kind));
                let mut params = String::new();
                for (param, value) in prototype.params.iter().zip(&target_cell.params) {
                    writeln!(&mut params, "param {:?} = {value}", param.name).unwrap();
                }

                if !params.is_empty() {
                    node = node.arg(params);
                }

                for input in &prototype.inputs {
                    let value = target_cell.inputs.slice(input.range.clone());
                    node = node.prefix_value(&format!("{:?} = ", input.name), &value);
                }

                node
            }
            Cell::Memory(memory) => {
                let header = format!("memory depth=#{} width=#{}", memory.depth, memory.width);
                let mut node = Node::from_name(cell, &header);
                for port in &memory.write_ports {
                    node = node.prefix_value("write addr=", &port.addr).prefix_value(". data=", &port.data);
                    if !port.mask.is_ones() {
                        node = node.prefix_value(". mask=", &port.mask);
                    }
                    node = node.control(". clk", port.clock, None);
                }

                for port in &memory.read_ports {
                    node = node.prefix_value("read addr=", &port.addr);
                    if let Some(flop) = &port.flip_flop {
                        node = node.control(". clk", flop.clock, None);
                        if flop.has_clear() {
                            let has_value = flop.clear_value != flop.init_value;
                            node = node.control(". clr", flop.clear, has_value.then(|| flop.clear_value.to_string()));
                        }

                        if flop.has_reset() {
                            let has_value = flop.reset_value != flop.init_value;
                            node = node.control(". rst", flop.reset, has_value.then(|| flop.reset_value.to_string()));
                        }

                        if flop.has_enable() {
                            node = node.control(". en", flop.enable, None);
                        }

                        if flop.has_reset() && flop.has_enable() {
                            if flop.reset_over_enable {
                                node = node.arg(". rst/en");
                            } else {
                                node = node.arg(". en/rst");
                            }
                        }

                        if flop.has_init_value() {
                            node = node.arg(format!(". init={}", flop.init_value));
                        }
                    }
                }
                node
            }
            _ => {
                let label = design.display_cell(cell).to_string();
                // this sucks but braces are special to Graphviz.
                // yes, even in strings
                let label = label.replace("{", "(").replace("}", ")");
                let mut node = Node::new(cell, label);

                cell.visit(|net| {
                    if let Ok((cell, _)) = design.find_cell(net) {
                        node.add_input(cell);
                    }
                });

                node
            }
        };

        if let Some(names) = names.get(&cell) {
            let mut exact_names = String::new();
            let mut approx_names = String::new();
            for name in names.iter() {
                let (Cell::Name(s, v) | Cell::Debug(s, v)) = &*name.get() else { unreachable!() };

                if cell.output() == *v {
                    writeln!(&mut exact_names, "{s:?}").unwrap();
                } else {
                    writeln!(&mut approx_names, "{s:?} = {}", design.display_value(v)).unwrap();
                }
            }

            if !exact_names.is_empty() {
                node = node.arg(exact_names);
            }

            if !approx_names.is_empty() {
                node = node.arg(approx_names);
            }
        }

        ctx.add_node(node);
    }

    ctx.print(writer)
}
