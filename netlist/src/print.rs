use std::{borrow::Cow, fmt::Display, vec};

use crate::{
    metadata::MetaItemIndex, value::ControlNets, Cell, CellRef, Const, ControlNet, Design, IoNet, IoValue, MemoryPortRelation, Net, TargetOutput, Trit, Value
};

struct DisplayFn<'a, F: for<'b> Fn(&Design, &mut std::fmt::Formatter<'b>) -> std::fmt::Result>(&'a Design, F);

impl<F: Fn(&Design, &mut std::fmt::Formatter) -> std::fmt::Result> Display for DisplayFn<'_, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.1(self.0, f)
    }
}

impl Design {
    // For display purposes only: go from a net to a `%x:y`, taking into account that the range may have
    // to designate only a part of a multi-output cell's combined output.
    fn find_cell_output(&self, net: Net) -> Result<(Value, usize, usize), Trit> {
        let (cell_ref, offset) = self.find_cell(net)?;
        let cell_index = cell_ref.debug_index();
        match &*cell_ref.get() {
            Cell::Other(instance) => {
                for (_name, range) in &instance.outputs {
                    if range.contains(&offset) {
                        return Ok((
                            cell_ref.output().slice(range.clone()),
                            cell_index + range.start,
                            offset - range.start,
                        ));
                    }
                }
                unreachable!()
            }
            Cell::Target(target_cell) => {
                let prototype = self.target_prototype(target_cell);
                for TargetOutput { range, .. } in &prototype.outputs {
                    if range.contains(&offset) {
                        return Ok((
                            cell_ref.output().slice(range.clone()),
                            cell_index + range.start,
                            offset - range.start,
                        ));
                    }
                }
                unreachable!()
            }
            Cell::Memory(memory) => {
                let mut port_offset = 0;
                for port in &memory.read_ports {
                    let port_range = port_offset..port_offset + port.data_len;
                    if port_range.contains(&offset) {
                        return Ok((
                            cell_ref.output().slice(port_range.clone()),
                            cell_index + port_range.start,
                            offset - port_range.start,
                        ));
                    }
                    port_offset += port.data_len;
                }
                unreachable!()
            }
            _ => Ok((cell_ref.output(), cell_index, offset)),
        }
    }

    pub(crate) fn write_string(f: &mut std::fmt::Formatter, str: &str) -> std::fmt::Result {
        write!(f, "\"")?;
        for byte in str.as_bytes() {
            if (byte.is_ascii_graphic() || matches!(byte, b' ' | b'\t')) && ![b'\\', b'"'].contains(byte) {
                write!(f, "{}", *byte as char)?;
            } else {
                assert!(byte.is_ascii());
                write!(f, "\\{:02x}", byte)?;
            }
        }
        write!(f, "\"")?;
        Ok(())
    }

    pub(crate) fn write_io_net(&self, f: &mut std::fmt::Formatter, io_net: IoNet) -> std::fmt::Result {
        if io_net.is_floating() {
            write!(f, "&_")
        } else {
            write!(f, "&")?;
            match self.find_io(io_net) {
                Some((name, offset)) => {
                    Design::write_string(f, name)?;
                    if self.get_io(name).unwrap().len() > 1 {
                        write!(f, "+{}", offset)?;
                    }
                    Ok(())
                }
                None => write!(f, "??"),
            }
        }
    }

    pub(crate) fn write_io_value(&self, f: &mut std::fmt::Formatter, io_value: &IoValue) -> std::fmt::Result {
        if io_value.is_empty() {
            write!(f, "[]")
        } else if io_value.len() == 1 {
            self.write_io_net(f, io_value[0])
        } else if io_value.iter().all(IoNet::is_floating) {
            write!(f, "&_:{}", io_value.len())
        } else {
            if let Some((name, _offset)) = self.find_io(io_value[0]) {
                if self.get_io(name).unwrap() == *io_value {
                    write!(f, "&")?;
                    Design::write_string(f, name)?;
                    write!(f, ":{}", io_value.len())?;
                    return Ok(());
                }
            }
            write!(f, "[")?;
            for io_net in io_value.iter().rev() {
                write!(f, " ")?;
                self.write_io_net(f, io_net)?;
            }
            write!(f, " ]")
        }
    }

    pub(crate) fn write_net(&self, f: &mut std::fmt::Formatter, net: Net) -> std::fmt::Result {
        if let Ok(index) = net.as_cell_index() {
            if !self.is_valid_cell_index(index) {
                return write!(f, "%_{index}");
            }
        }
        match self.find_cell_output(net) {
            Ok((output, index, offset)) => {
                if output.len() == 1 {
                    write!(f, "%{index}")
                } else {
                    write!(f, "%{index}+{offset}")
                }
            }
            Err(trit) => write!(f, "{trit}"),
        }
    }

    pub(crate) fn write_control_net(&self, f: &mut std::fmt::Formatter, control_net: ControlNet) -> std::fmt::Result {
        if control_net.is_negative() {
            write!(f, "!")?;
        };
        self.write_net(f, control_net.net())
    }

    pub(crate) fn write_value(&self, f: &mut std::fmt::Formatter, value: &Value) -> std::fmt::Result {
        enum Chunk {
            Slice { cell_index: usize, offset: usize, width: usize, repeat: usize },
            WholeGrainCell { cell_index: usize, width: usize, repeat: usize },
            NewNet { net: Net, repeat: usize },
            Const(Const),
        }
        let mut index = 0;
        let mut chunks = vec![];
        while index < value.len() {
            if value[index].as_cell_index().map(|index| !self.is_valid_cell_index(index)).unwrap_or(false) {
                // Value contains newly added cells that we can't look up. Don't try to make
                // the display nicer, just make sure it doesn't panic.
                let mut repeat = 1;
                while index + repeat < value.len() && value[index] == value[index + repeat] {
                    repeat += 1;
                }
                chunks.push(Chunk::NewNet { net: value[index], repeat });
                index += repeat;
            } else if let Ok((output, index_a, offset_a)) = self.find_cell_output(value[index]) {
                let count = value[index..]
                    .iter()
                    .enumerate()
                    .take_while(|(addend, net)| {
                        if let Ok((_, index_b, offset_b)) = self.find_cell_output(**net) {
                            index_a == index_b && offset_a + *addend == offset_b
                        } else {
                            false
                        }
                    })
                    .count();
                assert!(count != 0);
                let mut repeat = 1;
                while index + (repeat + 1) * count <= value.len()
                    && value[index..index + count] == value[index + repeat * count..index + (repeat + 1) * count]
                {
                    repeat += 1;
                }
                if offset_a == 0 && output.len() == count {
                    chunks.push(Chunk::WholeGrainCell { cell_index: index_a, width: count, repeat });
                } else {
                    chunks.push(Chunk::Slice { cell_index: index_a, offset: offset_a, width: count, repeat });
                }
                index += count * repeat;
            } else {
                let const_value = Const::from_iter(
                    value[index..].iter().take_while(|net| net.as_const().is_some()).map(|net| net.as_const().unwrap()),
                );
                assert!(!const_value.is_empty());
                index += const_value.len();
                chunks.push(Chunk::Const(const_value));
            }
        }
        if chunks.is_empty() {
            return write!(f, "[]");
        }
        let single_chunk = chunks.len() == 1;
        if !single_chunk {
            write!(f, "[")?;
        }
        for chunk in chunks.into_iter().rev() {
            if !single_chunk {
                write!(f, " ")?;
            }
            match chunk {
                Chunk::Slice { cell_index, offset, width, repeat } => {
                    write!(f, "%{cell_index}+{offset}")?;
                    if width != 1 {
                        write!(f, ":{width}")?;
                    }
                    if repeat != 1 {
                        write!(f, "*{repeat}")?;
                    }
                }
                Chunk::WholeGrainCell { cell_index, width, repeat } => {
                    write!(f, "%{cell_index}")?;
                    if width != 1 {
                        write!(f, ":{width}")?;
                    }
                    if repeat != 1 {
                        write!(f, "*{repeat}")?;
                    }
                }
                Chunk::Const(const_value) => {
                    write!(f, "{const_value}")?;
                }
                Chunk::NewNet { net, repeat } => {
                    self.write_net(f, net)?;
                    if repeat != 1 {
                        write!(f, "*{repeat}")?;
                    }
                }
            }
        }
        if !single_chunk {
            write!(f, " ]")?;
        }
        Ok(())
    }

    pub(crate) fn write_cell(
        &self,
        f: &mut std::fmt::Formatter,
        prefix: &str,
        index: usize,
        cell: &Cell,
        metadata: MetaItemIndex,
    ) -> std::fmt::Result {
        let newline = &format!("\n{prefix}");

        let write_metadata = |f: &mut std::fmt::Formatter| -> std::fmt::Result {
            if metadata != MetaItemIndex::NONE { write!(f, " {metadata}") } else { Ok(()) }
        };

        let write_control = |f: &mut std::fmt::Formatter, name: &str, control_net: ControlNet| -> std::fmt::Result {
            write!(f, "{}=", name)?;
            self.write_control_net(f, control_net)
        };

        let write_control_nets = |f: &mut std::fmt::Formatter, name: &str, control_nets: &ControlNets| -> std::fmt::Result {
            write!(f, "{}=", name)?;
            let control_nets_vec: Vec<ControlNet> = control_nets.clone().control_nets();
            for (_, control_net) in control_nets_vec.iter().enumerate() {
                self.write_control_net(f, *control_net)?;
            }
            Ok(())
        };

        let write_common = |f: &mut std::fmt::Formatter, name: &str, args: &[&Value]| -> std::fmt::Result {
            write!(f, "{}", name)?;
            for arg in args {
                write!(f, " ")?;
                self.write_value(f, arg)?;
            }
            Ok(())
        };

        let write_shift =
            |f: &mut std::fmt::Formatter, name: &str, arg1: &Value, arg2: &Value, stride: u32| -> std::fmt::Result {
                write!(f, "{} ", name)?;
                self.write_value(f, arg1)?;
                write!(f, " ")?;
                self.write_value(f, arg2)?;
                write!(f, " #{stride}")?;
                Ok(())
            };

        let write_cell_argument = |f: &mut std::fmt::Formatter, keyword: &str, name: &str| -> std::fmt::Result {
            write!(f, "  {keyword} ")?;
            Design::write_string(f, name)?;
            write!(f, " = ")?;
            Ok(())
        };

        let single_output = match &cell {
            Cell::Other(..) => false,
            Cell::Target(target_cell) if self.target_prototype(target_cell).outputs.len() > 1 => false,
            Cell::Memory(..) => false,
            _ => true,
        };
        if single_output {
            write!(f, "{prefix}%{}:{} = ", index, cell.output_len())?;
        } else {
            write!(f, "{prefix}%{}:_ = ", index)?; // to be able to find any cell by its index
        }
        match &cell {
            Cell::Buf(arg) => write_common(f, "buf", &[arg])?,
            Cell::Not(arg) => write_common(f, "not", &[arg])?,
            Cell::And(arg1, arg2) => write_common(f, "and", &[arg1, arg2])?,
            Cell::Or(arg1, arg2) => write_common(f, "or", &[arg1, arg2])?,
            Cell::Xor(arg1, arg2) => write_common(f, "xor", &[arg1, arg2])?,
            Cell::Mux(arg1, arg2, arg3) => {
                write!(f, "mux ")?;
                self.write_net(f, *arg1)?;
                write!(f, " ")?;
                self.write_value(f, arg2)?;
                write!(f, " ")?;
                self.write_value(f, arg3)?;
            }
            Cell::Adc(arg1, arg2, arg3) => {
                write!(f, "adc ")?;
                self.write_value(f, arg1)?;
                write!(f, " ")?;
                self.write_value(f, arg2)?;
                write!(f, " ")?;
                self.write_net(f, *arg3)?;
            }
            Cell::Aig(arg1, arg2) => {
                write!(f, "aig ")?;
                self.write_control_net(f, *arg1)?;
                write!(f, " ")?;
                self.write_control_net(f, *arg2)?;
            }

            Cell::Eq(arg1, arg2) => write_common(f, "eq", &[arg1, arg2])?,
            Cell::ULt(arg1, arg2) => write_common(f, "ult", &[arg1, arg2])?,
            Cell::SLt(arg1, arg2) => write_common(f, "slt", &[arg1, arg2])?,

            Cell::Shl(arg1, arg2, stride) => write_shift(f, "shl", arg1, arg2, *stride)?,
            Cell::UShr(arg1, arg2, stride) => write_shift(f, "ushr", arg1, arg2, *stride)?,
            Cell::SShr(arg1, arg2, stride) => write_shift(f, "sshr", arg1, arg2, *stride)?,
            Cell::XShr(arg1, arg2, stride) => write_shift(f, "xshr", arg1, arg2, *stride)?,

            Cell::Mul(arg1, arg2) => write_common(f, "mul", &[arg1, arg2])?,
            Cell::UDiv(arg1, arg2) => write_common(f, "udiv", &[arg1, arg2])?,
            Cell::UMod(arg1, arg2) => write_common(f, "umod", &[arg1, arg2])?,
            Cell::SDivTrunc(arg1, arg2) => write_common(f, "sdiv_trunc", &[arg1, arg2])?,
            Cell::SDivFloor(arg1, arg2) => write_common(f, "sdiv_floor", &[arg1, arg2])?,
            Cell::SModTrunc(arg1, arg2) => write_common(f, "smod_trunc", &[arg1, arg2])?,
            Cell::SModFloor(arg1, arg2) => write_common(f, "smod_floor", &[arg1, arg2])?,

            Cell::Match(match_cell) => {
                write!(f, "match")?;
                if match_cell.enable != Net::ONE {
                    write!(f, " en=")?;
                    self.write_net(f, match_cell.enable)?;
                }
                write!(f, " ")?;
                self.write_value(f, &match_cell.value)?;
                let multiline = match_cell
                    .patterns
                    .iter()
                    .map(|alternates| alternates.iter().map(Const::len).sum::<usize>())
                    .sum::<usize>()
                    > 80;
                write_metadata(f)?;
                write!(f, " {{")?;
                if multiline {
                    write!(f, "{newline}")?;
                }
                for alternates in &match_cell.patterns {
                    if multiline {
                        write!(f, "  ")?;
                    } else {
                        write!(f, " ")?;
                    }
                    if alternates.len() == 1 {
                        let pattern = &alternates[0];
                        write!(f, "{pattern}")?;
                    } else {
                        write!(f, "(")?;
                        for (index, pattern) in alternates.iter().enumerate() {
                            if index > 0 {
                                write!(f, " ")?;
                            }
                            write!(f, "{pattern}")?;
                        }
                        write!(f, ")")?;
                    }
                    if multiline {
                        write!(f, "{newline}")?;
                    }
                }
                if !multiline {
                    write!(f, " ")?;
                }
                write!(f, "}}")?;
            }
            Cell::Assign(assign_cell) => {
                write!(f, "assign")?;
                if assign_cell.enable != Net::ONE {
                    write!(f, " en=")?;
                    self.write_net(f, assign_cell.enable)?;
                }
                write!(f, " ")?;
                self.write_value(f, &assign_cell.value)?;
                write!(f, " ")?;
                self.write_value(f, &assign_cell.update)?;
                if assign_cell.offset != 0 {
                    write!(f, " at=#{}", assign_cell.offset)?;
                }
            }

            Cell::Dff(flip_flop) => {
                write_common(f, "dff", &[&flip_flop.data])?;
                write_control(f, " clk", flip_flop.clock)?;
                if flip_flop.has_clear() {
                    write_control_nets(f, " clr", &flip_flop.clear)?;
                    if flip_flop.clear_value != flip_flop.init_value {
                        write!(f, ",{}", flip_flop.clear_value)?;
                    }
                }
                if flip_flop.has_reset() {
                    write_control(f, " rst", flip_flop.reset)?;
                    if flip_flop.reset_value != flip_flop.init_value {
                        write!(f, ",{}", flip_flop.reset_value)?;
                    }
                }
                if flip_flop.has_enable() {
                    write_control(f, " en", flip_flop.enable)?;
                }
                if flip_flop.has_reset() && flip_flop.has_enable() {
                    if flip_flop.reset_over_enable {
                        write!(f, " rst/en")?;
                    } else {
                        write!(f, " en/rst")?;
                    }
                }
                if flip_flop.has_init_value() {
                    write!(f, " init={}", flip_flop.init_value)?;
                }
            }
            Cell::Memory(memory) => {
                write!(f, "memory depth=#{} width=#{}", memory.depth, memory.width)?;
                write_metadata(f)?;
                write!(f, " {{{newline}")?;
                for write_port in &memory.write_ports {
                    write!(f, "  write addr=")?;
                    self.write_value(f, &write_port.addr)?;
                    write!(f, " data=")?;
                    self.write_value(f, &write_port.data)?;
                    if !write_port.mask.is_ones() {
                        write!(f, " mask=")?;
                        self.write_value(f, &write_port.mask)?;
                    }
                    write_control(f, " clk", write_port.clock)?;
                    write!(f, "{newline}")?;
                }
                let mut port_offset = 0;
                for read_port in &memory.read_ports {
                    write!(f, "  %{}:{} = read addr=", index + port_offset, read_port.data_len)?;
                    self.write_value(f, &read_port.addr)?;
                    if let Some(ref flip_flop) = read_port.flip_flop {
                        write_control(f, " clk", flip_flop.clock)?;
                        if flip_flop.has_clear() {
                            write_control(f, " clr", flip_flop.clear)?;
                            if flip_flop.clear_value != flip_flop.init_value {
                                write!(f, ",{}", flip_flop.clear_value)?;
                            }
                        }
                        if flip_flop.has_reset() {
                            write_control(f, " rst", flip_flop.reset)?;
                            if flip_flop.reset_value != flip_flop.init_value {
                                write!(f, ",{}", flip_flop.reset_value)?;
                            }
                        }
                        if flip_flop.has_enable() {
                            write_control(f, " en", flip_flop.enable)?;
                        }
                        if flip_flop.has_reset() && flip_flop.has_enable() {
                            if flip_flop.reset_over_enable {
                                write!(f, " rst/en")?;
                            } else {
                                write!(f, " en/rst")?;
                            }
                        }
                        if flip_flop.has_init_value() {
                            write!(f, " init={}", flip_flop.init_value)?;
                        }
                        write!(f, " [")?;
                        for (index, relation) in flip_flop.relations.iter().enumerate() {
                            match relation {
                                MemoryPortRelation::Undefined => write!(f, "undef")?,
                                MemoryPortRelation::ReadBeforeWrite => write!(f, "rdfirst")?,
                                MemoryPortRelation::Transparent => write!(f, "trans")?,
                            }
                            if index < flip_flop.relations.len() - 1 {
                                write!(f, " ")?;
                            }
                        }
                        write!(f, "]")?;
                    }
                    write!(f, "{newline}")?;
                    port_offset += read_port.data_len;
                }
                let mut rows = Vec::new();
                for index in 0..memory.depth {
                    rows.push(memory.init_value.slice((index * memory.width)..((index + 1) * memory.width)));
                }
                let mut index = 0;
                while index < rows.len() {
                    write!(f, "  init {}", rows[index])?;
                    let mut repeats = 1;
                    while rows.get(index) == rows.get(index + repeats) {
                        repeats += 1;
                    }
                    if repeats > 1 {
                        write!(f, "*{repeats}")?;
                    }
                    write!(f, "{newline}")?;
                    index += repeats;
                }
                write!(f, "}}")?;
            }
            Cell::IoBuf(io_buffer) => {
                write!(f, "iobuf ")?;
                self.write_io_value(f, &io_buffer.io)?;
                write!(f, " o=")?;
                self.write_value(f, &io_buffer.output)?;
                write_control(f, " en", io_buffer.enable)?;
            }
            Cell::Other(instance) => {
                Design::write_string(f, instance.kind.as_str())?;
                write_metadata(f)?;
                write!(f, " {{{newline}")?;
                for (name, value) in instance.params.iter() {
                    write_cell_argument(f, "param", name)?;
                    write!(f, "{value}{newline}")?;
                }
                for (name, value) in instance.inputs.iter() {
                    write_cell_argument(f, "input", name)?;
                    self.write_value(f, value)?;
                    write!(f, "{newline}")?;
                }
                for (name, range) in instance.outputs.iter() {
                    write!(f, "  %{}:{} = output ", index + range.start, range.len())?;
                    Design::write_string(f, &name)?;
                    write!(f, "{newline}")?;
                }
                for (name, value) in instance.ios.iter() {
                    write_cell_argument(f, "io", name)?;
                    self.write_io_value(f, value)?;
                    write!(f, "{newline}")?;
                }
                write!(f, "}}")?;
            }
            Cell::Target(target_cell) => {
                write!(f, "target ")?;
                let prototype = self.target_prototype(target_cell);
                Design::write_string(f, &target_cell.kind)?;
                write_metadata(f)?;
                write!(f, " {{{newline}")?;
                for (param, value) in prototype.params.iter().zip(target_cell.params.iter()) {
                    write_cell_argument(f, "param", &param.name)?;
                    write!(f, "{value}{newline}")?;
                }
                for target_input in &prototype.inputs {
                    write_cell_argument(f, "input", &target_input.name)?;
                    self.write_value(f, &target_cell.inputs.slice(target_input.range.clone()))?;
                    write!(f, "{newline}")?;
                }
                if prototype.outputs.len() > 1 {
                    for target_output in &prototype.outputs {
                        write!(f, "  %{}:{} = output ", index + target_output.range.start, target_output.range.len())?;
                        Design::write_string(f, &target_output.name)?;
                        write!(f, "{newline}")?;
                    }
                }
                for target_io in &prototype.ios {
                    write_cell_argument(f, "io", &target_io.name)?;
                    self.write_io_value(f, &target_cell.ios.slice(target_io.range.clone()))?;
                    write!(f, "{newline}")?;
                }
                write!(f, "}}")?;
            }

            Cell::Input(name, _size) => {
                write!(f, "input ")?;
                Design::write_string(f, name)?;
            }
            Cell::Output(name, value) => {
                write!(f, "output ")?;
                Design::write_string(f, name)?;
                write!(f, " ")?;
                self.write_value(f, value)?;
            }
            Cell::Name(name, value) => {
                write!(f, "name ")?;
                Design::write_string(f, name)?;
                write!(f, " ")?;
                self.write_value(f, value)?;
            }
            Cell::Debug(name, value) => {
                write!(f, "debug ")?;
                Design::write_string(f, name)?;
                write!(f, " ")?;
                self.write_value(f, value)?;
            }
        }
        match cell {
            Cell::Match(..) | Cell::Memory(..) | Cell::Other(..) | Cell::Target(..) => (),
            _ => write_metadata(f)?,
        }
        Ok(())
    }

    pub fn display_net(&self, net: impl Into<Net>) -> impl Display + '_ {
        let net = net.into();
        DisplayFn(self, move |design: &Design, f| design.write_net(f, net))
    }

    pub fn display_control_net(&self, net: impl Into<ControlNet>) -> impl Display + '_ {
        let net = net.into();
        DisplayFn(self, move |design: &Design, f| design.write_control_net(f, net))
    }

    pub fn display_value<'a, 'b: 'a>(&'a self, value: impl Into<Cow<'b, Value>>) -> impl Display + 'a {
        let value = value.into();
        DisplayFn(self, move |design: &Design, f| design.write_value(f, &value))
    }

    pub fn display_cell<'a>(&'a self, cell_ref: CellRef<'a>) -> impl Display + 'a {
        DisplayFn(self, move |design: &Design, f| {
            design.write_cell(f, "", cell_ref.debug_index(), &*cell_ref.get(), cell_ref.metadata().index())
        })
    }
}
