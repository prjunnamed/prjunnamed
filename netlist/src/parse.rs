use std::{collections::BTreeMap, fmt::Display, ops::Range, str::FromStr, sync::Arc};

use yap::{one_of, types::WithContext, IntoTokens, TokenLocation, Tokens};

use crate::{
    create_target, AssignCell, Cell, Const, ControlNet, Design, FlipFlop, Instance, IoBuffer, IoNet, IoValue,
    MatchCell, Memory, MemoryPortRelation, MemoryReadFlipFlop, MemoryReadPort, MemoryWritePort, MetaItem, Net,
    ParamValue, Target, TargetCell, Value,
};
use crate::metadata::{MetaStringIndex, MetaItemIndex, SourcePosition};

#[derive(Debug)]
struct Context {
    design: Design,
    meta_map: BTreeMap<usize, MetaItemIndex>,
    def_map: BTreeMap<usize, Value>,        // definition: index -> value
    use_map: BTreeMap<(usize, usize), Net>, // reference:  index + offset -> void (only if above definition)
}

impl Context {
    fn new(target: Option<Arc<dyn Target>>) -> Context {
        Context {
            design: Design::with_target(target),
            meta_map: BTreeMap::new(),
            def_map: BTreeMap::new(),
            use_map: BTreeMap::new(),
        }
    }

    fn add_meta(&mut self, index: usize, item_index: MetaItemIndex) {
        assert_eq!(self.meta_map.insert(index, item_index), None, "metadata indices cannot be reused");
    }

    fn get_meta(&self, index: usize) -> MetaItemIndex {
        *self.meta_map.get(&index).expect("index should reference a metadata item")
    }

    fn add_io(&mut self, name: String, width: usize) -> IoValue {
        self.design.add_io(name, width)
    }

    fn get_io(&self, name: impl AsRef<str>) -> IoValue {
        self.design.get_io(name).expect("name should reference an IO")
    }

    fn get_io1(&self, name: impl AsRef<str>) -> IoNet {
        let value = self.get_io(name);
        assert_eq!(value.len(), 1, "IO width should be 1");
        value[0]
    }

    fn add_def(&mut self, index: usize, width: usize, value: Value) {
        assert_eq!(value.len(), width, "cell width should match declaration width");
        assert_eq!(self.def_map.insert(index, value.clone()), None, "cell indices cannot be reused");
    }

    fn get_use(&mut self, index: usize, offsets: Range<usize>) -> Value {
        if let Some(value) = self.def_map.get(&index) {
            value.slice(offsets)
        } else {
            let mut nets = vec![];
            for offset in offsets {
                let net = self.use_map.entry((index, offset)).or_insert_with(|| self.design.add_void(1).unwrap_net());
                nets.push(*net);
            }
            Value::from(nets)
        }
    }

    fn apply(mut self) -> Design {
        for ((index, offset), net) in self.use_map.into_iter() {
            if let Some(output) = self.def_map.get(&index) {
                if offset < output.len() {
                    self.design.replace_net(net, output[offset]);
                } else {
                    panic!("reference %{}+{} out of bounds for definition %{}:{}", index, offset, index, output.len());
                }
            } else {
                panic!("unresolved reference %{}", index)
            }
        }
        self.design.apply();
        self.design
    }
}

fn parse_space(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> bool {
    t.skip_while(|c| *c == ' ' || *c == '\t') > 0
}

fn parse_comment(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> bool {
    if !t.token(';') {
        return false;
    }
    t.skip_while(|c| *c != '\n');
    true
}

fn parse_blank(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> bool {
    let space = parse_space(t);
    let comment = parse_comment(t);
    space || comment
}

#[must_use]
fn parse_symbol(t: &mut WithContext<impl Tokens<Item = char>, Context>, symbol: char) -> Option<()> {
    debug_assert!(symbol != '\n', "use parse_newline()");
    if !t.token(symbol) {
        return None;
    }
    Some(())
}

#[must_use]
fn parse_newline(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<()> {
    if one_of!(t;
        t.token('\n'),
        t.token('\r') && t.token('\n')
    ) {
        Some(())
    } else {
        None
    }
}

fn parse_decimal<T: FromStr>(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<T> {
    t.take_while(|c| c.is_digit(10) || *c == '-').parse::<T, String>().ok()
}

fn parse_integer<T: FromStr>(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<T> {
    parse_symbol(t, '#')?;
    parse_decimal(t)
}

fn parse_string_char(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<u8> {
    match t.next() {
        Some('"' | '\\') => None,
        Some(char) if char.is_ascii() => Some(char as u8),
        _ => None,
    }
}

fn parse_string_escape(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<u8> {
    parse_symbol(t, '\\')?;
    if let (Some(hi @ ('0'..='9' | 'a'..='f')), Some(lo @ ('0'..='9' | 'a'..='f'))) = (t.next(), t.next()) {
        u8::from_str_radix(&format!("{hi}{lo}"), 16).ok()
    } else {
        None
    }
}

fn parse_string(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<String> {
    parse_symbol(t, '"')?;
    let bytes = t
        .many(|t| {
            one_of!(t;
                parse_string_char(t),
                parse_string_escape(t)
            )
        })
        .collect::<Vec<u8>>();
    parse_symbol(t, '"')?;
    String::from_utf8(bytes).ok()
}

fn parse_const(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<Const> {
    t.take_while(|c| *c == 'X' || *c == '0' || *c == '1')
        .parse::<Const, String>()
        .ok()
        .and_then(|value| if !value.is_empty() { Some(value) } else { None })
}

fn parse_keyword(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<String> {
    let name: String = t.take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '/')).collect();
    if name.is_empty() {
        return None;
    }
    Some(name)
}

fn parse_keyword_eq(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<String> {
    let keyword = parse_keyword(t)?;
    parse_blank(t);
    parse_symbol(t, '=')?;
    parse_blank(t);
    Some(keyword)
}

#[must_use]
fn parse_keyword_expect(t: &mut WithContext<impl Tokens<Item = char>, Context>, expected: &str) -> Option<()> {
    let keyword = parse_keyword(t)?;
    if keyword != expected {
        return None;
    }
    Some(())
}

#[must_use]
fn parse_keyword_eq_expect(t: &mut WithContext<impl Tokens<Item = char>, Context>, expected: &str) -> Option<()> {
    let keyword = parse_keyword_eq(t)?;
    if keyword != expected {
        return None;
    }
    Some(())
}

fn parse_metadata_index(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<usize> {
    parse_symbol(t, '!')?;
    parse_decimal(t)
}

fn parse_io_name(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<String> {
    parse_symbol(t, '&')?;
    parse_string(t)
}

fn parse_io_name_size(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<(String, usize)> {
    let name = parse_io_name(t)?;
    parse_symbol(t, ':')?;
    let size = parse_decimal(t)?;
    Some((name, size))
}

fn parse_io_name_offset(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<(String, usize)> {
    let name = parse_io_name(t)?;
    parse_symbol(t, '+')?;
    let offset = parse_decimal(t)?;
    Some((name, offset))
}

fn parse_io_net(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<IoNet> {
    one_of!(t;
        parse_symbol(t, '&').and_then(|()| parse_symbol(t, '_')).map(|()| IoNet::FLOATING),
        parse_io_name_offset(t).map(|(name, offset)| t.context().get_io(name)[offset]),
        parse_io_name(t).map(|name| t.context().get_io1(name))
    )
}

fn parse_io_value_floating(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<IoValue> {
    parse_symbol(t, '&')?;
    parse_symbol(t, '_')?;
    parse_symbol(t, ':')?;
    let size = parse_decimal(t)?;
    Some(IoValue::from_iter(std::iter::repeat_n(IoNet::FLOATING, size)))
}

fn parse_io_value_concat(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<IoValue> {
    let mut value = IoValue::new();
    parse_symbol(t, '[')?;
    let parts = Vec::from_iter(
        t.many(|t| {
            parse_blank(t);
            parse_io_net(t)
        })
        .as_iter(),
    );
    for part in parts.into_iter().rev() {
        value.extend([part]);
    }
    parse_blank(t);
    parse_symbol(t, ']')?;
    Some(value)
}

fn parse_io_value(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<IoValue> {
    one_of!(t;
        parse_io_value_concat(t),
        parse_io_value_floating(t),
        parse_io_name_size(t).and_then(|(name, size)| Some(t.context().get_io(name).slice(..size))),
        parse_io_net(t).map(IoValue::from),
    )
}

fn parse_cell_index(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<usize> {
    parse_symbol(t, '%')?;
    parse_decimal(t)
}

fn parse_cell_index_placeholder(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<usize> {
    let cell_index = parse_cell_index(t)?;
    parse_symbol(t, ':')?;
    parse_symbol(t, '_')?;
    Some(cell_index)
}

fn parse_cell_index_width(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<(usize, usize)> {
    let cell_index = parse_cell_index(t)?;
    parse_symbol(t, ':')?;
    let width = parse_decimal(t)?;
    Some((cell_index, width))
}

fn parse_cell_index_offset_width(
    t: &mut WithContext<impl Tokens<Item = char>, Context>,
) -> Option<(usize, usize, usize)> {
    let cell_index = parse_cell_index(t)?;
    let offset = if parse_symbol(t, '+').is_some() { parse_decimal(t)? } else { 0 };
    let width = if parse_symbol(t, ':').is_some() { parse_decimal(t)? } else { 1 };
    Some((cell_index, offset, width))
}

fn parse_value_part(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<Value> {
    let value = one_of!(t;
        parse_const(t).map(Value::from),
        parse_cell_index_offset_width(t).and_then(|(cell_index, offset, width)| {
            Some(t.context_mut().get_use(cell_index, offset..offset+width))
        }),
    )?;
    if parse_symbol(t, '*').is_some() {
        let repeat = parse_decimal(t)?;
        Some(value.repeat(repeat))
    } else {
        Some(value)
    }
}

fn parse_value_concat(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<Value> {
    let mut value = Value::new();
    parse_symbol(t, '[')?;
    let parts = Vec::from_iter(
        t.many(|t| {
            parse_blank(t);
            parse_value_part(t)
        })
        .as_iter(),
    );
    for part in parts.into_iter().rev() {
        value.extend(part);
    }
    parse_blank(t);
    parse_symbol(t, ']')?;
    Some(value)
}

fn parse_param_value(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<ParamValue> {
    one_of!(t;
        parse_const(t).map(ParamValue::Const),
        parse_integer(t).map(ParamValue::Int),
        parse_string(t).map(ParamValue::String)
    )
}

fn parse_target_option(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<(String, String)> {
    parse_blank(t);
    let name = parse_string(t)?;
    parse_blank(t);
    parse_symbol(t, '=')?;
    parse_blank(t);
    let value = parse_string(t)?;
    Some((name, value))
}

fn parse_target(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<()> {
    parse_keyword_expect(t, "target")?;
    parse_blank(t);
    let name = parse_string(t)?;
    let mut options = BTreeMap::new();
    while let Some((name, value)) = parse_target_option(t) {
        if options.insert(name.clone(), value).is_some() {
            panic!("target option {name} is specified more than once");
        }
    }
    parse_blank(t);
    parse_newline(t)?;
    let context = t.context_mut();
    if !context.design.is_empty() {
        panic!("target specification must come before any definitions");
    }
    context.design = Design::with_target(Some(create_target(&name, options).unwrap()));
    Some(())
}

fn parse_metadata_string(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<MetaStringIndex> {
    let string = parse_string(t)?;
    Some(t.context_mut().design.add_metadata_string(&string).index())
}

fn parse_metadata_set(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<MetaItemIndex> {
    let mut indices = Vec::new();
    parse_symbol(t, '{')?;
    parse_blank(t);
    while let Some(()) = t.optional(|t| {
        indices.push(parse_metadata_index(t)?);
        parse_blank(t);
        Some(())
    }) {}
    parse_symbol(t, '}')?;
    let ctx = t.context_mut();
    let items = indices.into_iter().map(|index| ctx.design.ref_metadata_item(ctx.get_meta(index))).collect();
    Some(ctx.design.add_metadata_item(&MetaItem::Set(items)).index())
}

fn parse_metadata_position(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<SourcePosition> {
    parse_symbol(t, '(')?;
    parse_blank(t);
    let line = parse_integer(t)?;
    parse_blank(t);
    let column = parse_integer(t)?;
    parse_blank(t);
    parse_symbol(t, ')')?;
    Some(SourcePosition { line, column })
}

fn parse_metadata_source(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<MetaItemIndex> {
    parse_keyword_expect(t, "source")?;
    parse_blank(t);
    let file = parse_metadata_string(t)?;
    parse_blank(t);
    let start = parse_metadata_position(t)?;
    parse_blank(t);
    let end = parse_metadata_position(t)?;
    let ctx = t.context_mut();
    let file = ctx.design.ref_metadata_string(file);
    Some(ctx.design.add_metadata_item(&MetaItem::Source { file, start, end }).index())
}

fn parse_metadata_scope(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<MetaItemIndex> {
    enum Scope {
        Named(MetaStringIndex),
        Indexed(i32),
    }

    parse_keyword_expect(t, "scope")?;
    parse_blank(t);
    let scope = one_of!(t;
        parse_metadata_string(t).map(Scope::Named),
        parse_integer(t).map(Scope::Indexed),
    )?;
    let parent_index = t.optional(|t| {
        parse_blank(t);
        parse_keyword_eq_expect(t, "in")?;
        parse_metadata_index(t)
    });
    let source_index = t.optional(|t| {
        parse_blank(t);
        parse_keyword_eq_expect(t, "src")?;
        parse_metadata_index(t)
    });
    let ctx = t.context_mut();
    let parent_index = parent_index.map(|parent_index| ctx.get_meta(parent_index)).unwrap_or(MetaItemIndex::NONE);
    let parent = ctx.design.ref_metadata_item(parent_index);
    let source_index = source_index.map(|source_index| ctx.get_meta(source_index)).unwrap_or(MetaItemIndex::NONE);
    let source = ctx.design.ref_metadata_item(source_index);
    match scope {
        Scope::Named(name_index) => {
            let name = ctx.design.ref_metadata_string(name_index);
            Some(ctx.design.add_metadata_item(&MetaItem::NamedScope { name, parent, source }).index())
        }
        Scope::Indexed(index) => {
            Some(ctx.design.add_metadata_item(&MetaItem::IndexedScope { index, parent, source }).index())
        }
    }
}

fn parse_metadata_ident(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<MetaItemIndex> {
    parse_keyword_expect(t, "ident")?;
    parse_blank(t);
    let name = parse_metadata_string(t)?;
    parse_blank(t);
    parse_keyword_eq_expect(t, "in")?;
    let scope_index = parse_metadata_index(t)?;
    let ctx = t.context_mut();
    let name = ctx.design.ref_metadata_string(name);
    let scope = ctx.design.ref_metadata_item(ctx.get_meta(scope_index));
    Some(ctx.design.add_metadata_item(&MetaItem::Ident { name, scope }).index())
}

fn parse_metadata_attr(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<MetaItemIndex> {
    parse_keyword_expect(t, "attr")?;
    parse_blank(t);
    let name = parse_metadata_string(t)?;
    parse_blank(t);
    let value = parse_param_value(t)?;
    let ctx = t.context_mut();
    let name = ctx.design.ref_metadata_string(name);
    Some(ctx.design.add_metadata_item(&MetaItem::Attr { name, value }).index())
}

fn parse_metadata(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<()> {
    let index = parse_metadata_index(t)?;
    parse_blank(t);
    parse_symbol(t, '=')?;
    parse_blank(t);
    let item_index = one_of!(t;
        parse_metadata_set(t),
        parse_metadata_source(t),
        parse_metadata_scope(t),
        parse_metadata_ident(t),
        parse_metadata_attr(t),
    )?;
    parse_blank(t);
    parse_newline(t)?;
    t.context_mut().add_meta(index, item_index);
    Some(())
}

fn parse_io(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<IoValue> {
    let (name, size) = parse_io_name_size(t)?;
    parse_blank(t);
    parse_symbol(t, '=')?;
    parse_blank(t);
    parse_keyword_expect(t, "io")?;
    parse_newline(t)?;
    let ctx = t.context_mut();
    let io_value = ctx.add_io(name, size);
    ctx.design.apply();
    Some(io_value)
}

fn parse_cell(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<()> {
    fn parse_metadata_ref(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<MetaItemIndex> {
        let index = parse_metadata_index(t)?;
        Some(t.context().get_meta(index))
    }

    fn parse_value_arg(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<Value> {
        parse_blank(t);
        one_of!(t;
            parse_value_part(t),
            parse_value_concat(t)
        )
    }

    fn parse_net_arg(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<Net> {
        parse_blank(t);
        parse_value_part(t).map(|value| {
            assert_eq!(value.len(), 1, "reference should be a single net");
            value[0]
        })
    }

    fn parse_control_net_arg(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<ControlNet> {
        parse_blank(t);
        let negated = parse_symbol(t, '!').is_some();
        let net = parse_net_arg(t)?;
        if negated { Some(ControlNet::Neg(net)) } else { Some(ControlNet::Pos(net)) }
    }

    fn parse_control_arg(t: &mut WithContext<impl Tokens<Item = char>, Context>, name: &str) -> Option<ControlNet> {
        parse_blank(t);
        parse_keyword_eq_expect(t, name)?;
        parse_control_net_arg(t)
    }

    fn parse_int_arg(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<usize> {
        parse_blank(t);
        parse_integer(t)
    }

    fn parse_string_arg(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<String> {
        parse_blank(t);
        parse_string(t)
    }

    fn parse_dff_reset_control_net_arg(
        t: &mut WithContext<impl Tokens<Item = char>, Context>,
        name: &str,
    ) -> Option<(ControlNet, Option<Const>)> {
        parse_control_arg(t, name).map(|control_net| {
            let init_value = t.optional(|t| {
                parse_blank(t);
                parse_symbol(t, ',')?;
                parse_blank(t);
                parse_const(t)
            });
            (control_net, init_value)
        })
    }

    fn parse_reset_over_enable_arg(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<bool> {
        parse_blank(t);
        one_of!(t;
            parse_keyword(t).filter(|kw| kw == "rst/en").map(|_| true),
            parse_keyword(t).filter(|kw| kw == "en/rst").map(|_| false),
        )
    }

    fn parse_dff_init_value_arg(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<Const> {
        parse_blank(t);
        parse_keyword_eq_expect(t, "init")?;
        parse_const(t)
    }

    fn parse_instance_param(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<(String, ParamValue)> {
        parse_keyword_expect(t, "param")?;
        parse_blank(t);
        let name = parse_string(t)?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        parse_blank(t);
        let value = parse_param_value(t)?;
        Some((name, value))
    }

    fn parse_instance_input(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<(String, Value)> {
        parse_keyword_expect(t, "input")?;
        parse_blank(t);
        let name = parse_string(t)?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        let value = parse_value_arg(t)?;
        Some((name, value))
    }

    fn parse_instance_output(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<(String, usize, usize)> {
        let (cell_index, width) = parse_cell_index_width(t)?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        parse_blank(t);
        parse_keyword_expect(t, "output")?;
        parse_blank(t);
        let name = parse_string(t)?;
        Some((name, cell_index, width))
    }

    fn parse_instance_io(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<(String, IoValue)> {
        parse_keyword_expect(t, "io")?;
        parse_blank(t);
        let name = parse_string(t)?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        parse_blank(t);
        let io_value = parse_io_value(t)?;
        Some((name, io_value))
    }

    fn parse_instance(
        t: &mut WithContext<impl Tokens<Item = char>, Context>,
    ) -> Option<(Instance, MetaItemIndex, Value)> {
        let mut instance = Instance {
            kind: parse_string(t)?,
            params: BTreeMap::new(),
            inputs: BTreeMap::new(),
            outputs: BTreeMap::new(),
            ios: BTreeMap::new(),
        };
        let mut output = Value::new();
        parse_blank(t);
        let metadata = parse_metadata_ref(t).unwrap_or(MetaItemIndex::NONE);
        parse_blank(t);
        parse_symbol(t, '{')?;
        parse_blank(t);
        parse_newline(t)?;
        while let Some(()) = t.optional(|t| {
            parse_blank(t);
            one_of!(t;
                parse_instance_param(t).map(|(name, value)|
                    assert!(instance.params.insert(name, value).is_none(), "duplicate parameter name in instance")),
                parse_instance_input(t).map(|(name, value)|
                    assert!(instance.inputs.insert(name, value).is_none(), "duplicate input name in instance")),
                parse_instance_output(t).map(|(name, index, width)| {
                    let start = instance.output_len();
                    assert!(
                        instance.outputs.insert(name, start..start + width).is_none(),
                        "duplicate output name in instance"
                    );
                    let ctx = t.context_mut();
                    let value = ctx.design.add_void(width);
                    ctx.add_def(index, width, value.clone());
                    output = output.concat(value);
                }),
                parse_instance_io(t).map(|(name, io_value)|
                    assert!(instance.ios.insert(name, io_value).is_none(), "duplicate IO name in instance"))
            );
            parse_blank(t);
            parse_newline(t)?;
            Some(())
        }) {}
        parse_blank(t);
        parse_symbol(t, '}')?;
        Some((instance, metadata, output))
    }

    fn parse_simple_cell(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<()> {
        let (index, width) = parse_cell_index_width(t)?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        parse_blank(t);
        let keyword = parse_keyword(t)?;
        let cell = match keyword.as_ref() {
            "buf" => Cell::Buf(parse_value_arg(t)?),
            "not" => Cell::Not(parse_value_arg(t)?),
            "and" => Cell::And(parse_value_arg(t)?, parse_value_arg(t)?),
            "or" => Cell::Or(parse_value_arg(t)?, parse_value_arg(t)?),
            "xor" => Cell::Xor(parse_value_arg(t)?, parse_value_arg(t)?),
            "mux" => Cell::Mux(parse_net_arg(t)?, parse_value_arg(t)?, parse_value_arg(t)?),
            "adc" => Cell::Adc(parse_value_arg(t)?, parse_value_arg(t)?, parse_net_arg(t)?),
            "aig" => Cell::Aig(parse_control_net_arg(t)?, parse_control_net_arg(t)?),
            "eq" => Cell::Eq(parse_value_arg(t)?, parse_value_arg(t)?),
            "ult" => Cell::ULt(parse_value_arg(t)?, parse_value_arg(t)?),
            "slt" => Cell::SLt(parse_value_arg(t)?, parse_value_arg(t)?),
            "shl" => Cell::Shl(parse_value_arg(t)?, parse_value_arg(t)?, parse_int_arg(t)? as u32),
            "ushr" => Cell::UShr(parse_value_arg(t)?, parse_value_arg(t)?, parse_int_arg(t)? as u32),
            "sshr" => Cell::SShr(parse_value_arg(t)?, parse_value_arg(t)?, parse_int_arg(t)? as u32),
            "xshr" => Cell::XShr(parse_value_arg(t)?, parse_value_arg(t)?, parse_int_arg(t)? as u32),
            "mul" => Cell::Mul(parse_value_arg(t)?, parse_value_arg(t)?),
            "udiv" => Cell::UDiv(parse_value_arg(t)?, parse_value_arg(t)?),
            "umod" => Cell::UMod(parse_value_arg(t)?, parse_value_arg(t)?),
            "sdiv_trunc" => Cell::SDivTrunc(parse_value_arg(t)?, parse_value_arg(t)?),
            "sdiv_floor" => Cell::SDivFloor(parse_value_arg(t)?, parse_value_arg(t)?),
            "smod_trunc" => Cell::SModTrunc(parse_value_arg(t)?, parse_value_arg(t)?),
            "smod_floor" => Cell::SModFloor(parse_value_arg(t)?, parse_value_arg(t)?),
            "assign" => {
                let enable = t
                    .optional(|t| {
                        parse_blank(t);
                        parse_keyword_eq_expect(t, "en")?;
                        parse_net_arg(t)
                    })
                    .unwrap_or(Net::ONE);
                let value = parse_value_arg(t)?;
                let update = parse_value_arg(t)?;
                let offset = t
                    .optional(|t| {
                        parse_blank(t);
                        parse_keyword_eq_expect(t, "at")?;
                        parse_integer(t)
                    })
                    .unwrap_or(0);
                Cell::Assign(AssignCell { value, enable, update, offset })
            }
            "dff" => {
                let data = parse_value_arg(t)?;
                let data_len = data.len();
                let clock = parse_control_arg(t, "clk")?;
                let (clear, clear_value) = t
                    .optional(|t| parse_dff_reset_control_net_arg(t, "clr"))
                    .unwrap_or((ControlNet::Pos(Net::ZERO), None));
                let (reset, reset_value) = t
                    .optional(|t| parse_dff_reset_control_net_arg(t, "rst"))
                    .unwrap_or((ControlNet::Pos(Net::ZERO), None));
                let enable = t.optional(|t| parse_control_arg(t, "en")).unwrap_or(ControlNet::Pos(Net::ONE));
                let reset_over_enable = t.optional(|t| parse_reset_over_enable_arg(t)).unwrap_or(false);
                let init_value =
                    t.optional(|t| parse_dff_init_value_arg(t)).unwrap_or_else(|| Const::undef(data_len));
                Cell::Dff(FlipFlop {
                    data,
                    clock,
                    clear,
                    load: ControlNet::ZERO,
                    clear_value: clear_value.unwrap_or_else(|| init_value.clone()),
                    load_data: Value::undef(data_len),
                    reset,
                    reset_value: reset_value.unwrap_or_else(|| init_value.clone()),
                    enable,
                    reset_over_enable,
                    init_value,
                })
            }
            "iobuf" => {
                parse_blank(t);
                let io = parse_io_value(t)?;
                parse_blank(t);
                parse_keyword_eq_expect(t, "o")?;
                let output = parse_value_arg(t)?;
                let enable = parse_control_arg(t, "en")?;
                Cell::IoBuf(IoBuffer { io, output, enable })
            }
            "input" => Cell::Input(parse_string_arg(t)?, width),
            "output" => Cell::Output(parse_string_arg(t)?, parse_value_arg(t)?),
            "name" => Cell::Name(parse_string_arg(t)?, parse_value_arg(t)?),
            "debug" => Cell::Debug(parse_string_arg(t)?, parse_value_arg(t)?),
            _ => return None,
        };
        parse_blank(t);
        let metadata = parse_metadata_ref(t).unwrap_or(MetaItemIndex::NONE);
        let ctx = t.context_mut();
        let output = ctx.design.add_cell_with_metadata_index(cell, metadata);
        assert_eq!(output.len(), width, "cell output width must match declaration width");
        ctx.add_def(index, width, output);
        Some(())
    }

    fn parse_match_cell(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<()> {
        let (index, width) = parse_cell_index_width(t)?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        parse_blank(t);
        parse_keyword_expect(t, "match")?;
        parse_blank(t);
        let enable = t
            .optional(|t| {
                parse_blank(t);
                parse_keyword_eq_expect(t, "en")?;
                parse_net_arg(t)
            })
            .unwrap_or(Net::ONE);
        let value = parse_value_arg(t)?;
        let mut patterns = Vec::new();
        parse_blank(t);
        let metadata = parse_metadata_ref(t).unwrap_or(MetaItemIndex::NONE);
        parse_blank(t);
        parse_symbol(t, '{')?;
        parse_blank(t);
        let _ = parse_newline(t);
        while let Some(()) = t.optional(|t| {
            parse_blank(t);
            let mut alternates = Vec::new();
            if let Some(()) = parse_symbol(t, '(') {
                while let Some(()) = t.optional(|t| {
                    parse_blank(t);
                    alternates.push(parse_const(t)?);
                    Some(())
                }) {}
                parse_blank(t);
                parse_symbol(t, ')')?;
            } else {
                alternates.push(parse_const(t)?);
            }
            parse_blank(t);
            let _ = parse_newline(t);
            patterns.push(alternates);
            Some(())
        }) {}
        parse_blank(t);
        parse_symbol(t, '}')?;
        let ctx = t.context_mut();
        let output =
            ctx.design.add_cell_with_metadata_index(Cell::Match(MatchCell { value, enable, patterns }), metadata);
        assert_eq!(output.len(), width, "cell output width must match declaration width");
        ctx.add_def(index, width, output);
        Some(())
    }

    fn parse_memory_init(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<Const> {
        parse_keyword_expect(t, "init")?;
        parse_blank(t);
        let value = parse_const(t)?;
        let repeats = t
            .optional(|t| {
                parse_symbol(t, '*')?;
                parse_decimal(t)
            })
            .unwrap_or(1);
        Some(value.repeat(repeats))
    }

    fn parse_memory_write(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<MemoryWritePort> {
        parse_keyword_expect(t, "write")?;
        parse_blank(t);
        parse_keyword_eq_expect(t, "addr")?;
        let addr = parse_value_arg(t)?;
        parse_blank(t);
        parse_keyword_eq_expect(t, "data")?;
        let data = parse_value_arg(t)?;
        parse_blank(t);
        let mask = t
            .optional(|t| {
                parse_keyword_eq_expect(t, "mask")?;
                parse_value_arg(t)
            })
            .unwrap_or_else(|| Value::ones(data.len()));
        let clock = parse_control_arg(t, "clk")?;
        Some(MemoryWritePort { addr, data, mask, clock })
    }

    fn parse_memory_read(
        t: &mut WithContext<impl Tokens<Item = char>, Context>,
    ) -> Option<(MemoryReadPort, usize, usize)> {
        let (cell_index, width) = parse_cell_index_width(t)?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        parse_blank(t);
        parse_keyword_expect(t, "read")?;
        parse_blank(t);
        parse_keyword_eq_expect(t, "addr")?;
        let addr = parse_value_arg(t)?;
        let flip_flop = t.optional(|t| {
            let clock = parse_control_arg(t, "clk")?;
            let (clear, clear_value) =
                t.optional(|t| parse_dff_reset_control_net_arg(t, "clr")).unwrap_or((ControlNet::Pos(Net::ZERO), None));
            let load = ControlNet::Pos(Net::ZERO);
            let load_data = Value::undef(width);

            let (reset, reset_value) =
                t.optional(|t| parse_dff_reset_control_net_arg(t, "rst")).unwrap_or((ControlNet::Pos(Net::ZERO), None));
            let enable = t.optional(|t| parse_control_arg(t, "en")).unwrap_or(ControlNet::Pos(Net::ONE));
            let reset_over_enable = t.optional(|t| parse_reset_over_enable_arg(t)).unwrap_or(false);
            let init_value = t.optional(|t| parse_dff_init_value_arg(t)).unwrap_or_else(|| Const::undef(width));
            let mut relations = vec![];
            parse_blank(t);
            parse_symbol(t, '[')?;
            while let Some(()) = t.optional(|t| {
                parse_blank(t);
                let keyword = parse_keyword(t)?;
                relations.push(match keyword.as_str() {
                    "undef" => MemoryPortRelation::Undefined,
                    "rdfirst" => MemoryPortRelation::ReadBeforeWrite,
                    "trans" => MemoryPortRelation::Transparent,
                    _ => return None,
                });
                Some(())
            }) {}
            parse_blank(t);
            parse_symbol(t, ']')?;
            Some(MemoryReadFlipFlop {
                clock,
                clear,
                clear_value: clear_value.unwrap_or_else(|| init_value.clone()),
                load,
                load_data,
                reset,
                reset_value: reset_value.unwrap_or_else(|| init_value.clone()),
                enable,
                reset_over_enable,
                init_value,
                relations,
            })
        });
        Some((MemoryReadPort { addr, data_len: width, flip_flop }, cell_index, width))
    }

    fn parse_memory_cell(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<()> {
        parse_cell_index_placeholder(t)?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        parse_blank(t);
        parse_keyword_expect(t, "memory")?;
        parse_blank(t);
        parse_keyword_eq_expect(t, "depth")?;
        let memory_depth = parse_integer(t)?;
        parse_blank(t);
        parse_keyword_eq_expect(t, "width")?;
        let memory_width = parse_integer(t)?;
        parse_blank(t);
        let metadata = parse_metadata_ref(t).unwrap_or(MetaItemIndex::NONE);
        parse_blank(t);
        parse_symbol(t, '{')?;
        parse_blank(t);
        parse_newline(t)?;
        let mut init_value = Const::new();
        let mut write_ports = Vec::new();
        let mut read_ports = Vec::new();
        let mut output = Value::new();
        while let Some(()) = t.optional(|t| {
            parse_blank(t);
            one_of!(t;
                parse_memory_init(t).map(|value| init_value.extend(value)),
                parse_memory_write(t).map(|port| write_ports.push(port)),
                parse_memory_read(t).map(|(port, index, width)| {
                    read_ports.push(port);
                    let ctx = t.context_mut();
                    let value = ctx.design.add_void(width);
                    ctx.add_def(index, width, value.clone());
                    output = output.concat(value);
                }),
            );
            parse_blank(t);
            parse_newline(t)?;
            Some(())
        }) {}
        parse_blank(t);
        parse_symbol(t, '}')?;
        let ctx = t.context_mut();
        ctx.design.replace_value(
            output,
            ctx.design.add_cell_with_metadata_index(
                Cell::Memory(Memory {
                    depth: memory_depth,
                    width: memory_width,
                    init_value: init_value
                        .concat(Const::undef((memory_depth * memory_width).checked_sub(init_value.len()).unwrap())),
                    write_ports,
                    read_ports,
                }),
                metadata,
            ),
        );
        Some(())
    }

    fn parse_target_cell(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<()> {
        let (index, width) = one_of!(t;
            parse_cell_index_width(t).map(|(index, width)| (index, Some(width))),
            parse_cell_index_placeholder(t).map(|index| (index, None))
        )?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        parse_blank(t);
        parse_keyword_expect(t, "target")?;
        parse_blank(t);
        let (instance, metadata, output) = parse_instance(t)?;
        let target = t.context().design.target().expect("no target specified");
        let prototype = target.prototype(&instance.kind).expect("no prototype for target cell");
        let mut target_cell = TargetCell::new(instance.kind.clone(), prototype);
        for (name, value) in instance.params {
            let target_param = prototype.get_param(&name).expect("unknown parameter");
            if !target_param.kind.is_valid(&value) {
                panic!("invalid value for parameter {name}");
            }
            target_cell.params[target_param.index] = value;
        }
        for (name, value) in instance.inputs {
            let target_input = prototype.get_input(&name).expect("unknown input");
            if value.len() != target_input.len() {
                panic!("width mismatch for input {name}");
            }
            target_cell.inputs[target_input.range.clone()].copy_from_slice(&value[..]);
        }
        for (name, value) in instance.ios {
            let target_io = prototype.get_io(&name).expect("unknown io");
            if value.len() != target_io.len() {
                panic!("width mismatch for io {name}");
            }
            target_cell.ios[target_io.range.clone()].copy_from_slice(&value[..]);
        }
        let ctx = t.context_mut();
        if let Some(width) = width {
            // %0:1 = target "SB_LUT" { .. }
            if !(instance.outputs.is_empty() && prototype.outputs.len() == 1 && prototype.output_len == width) {
                panic!("target instance should have a single implicit output of the right size")
            }
            ctx.add_def(index, width, ctx.design.add_cell_with_metadata_index(Cell::Target(target_cell), metadata));
        } else {
            // %0:_ = target "SB_LUT" { %0:1 = output "Y" .. }
            let target_cell_output = ctx.design.add_cell_with_metadata_index(Cell::Target(target_cell), metadata);
            for (name, range) in instance.outputs {
                let target_output = prototype.get_output(&name).expect("unknown output");
                if range.len() != target_output.len() {
                    panic!("width mismatch for output {name}");
                }
                ctx.design
                    .replace_value(output.slice(range.clone()), target_cell_output.slice(target_output.range.clone()));
            }
        }
        Some(())
    }

    fn parse_other_cell(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> Option<()> {
        parse_cell_index_placeholder(t)?;
        parse_blank(t);
        parse_symbol(t, '=')?;
        parse_blank(t);
        let (instance, metadata, output) = parse_instance(t)?;
        let ctx = t.context_mut();
        ctx.design.replace_value(output, ctx.design.add_cell_with_metadata_index(Cell::Other(instance), metadata));
        Some(())
    }

    one_of!(t;
        parse_simple_cell(t),
        parse_match_cell(t),
        parse_memory_cell(t),
        parse_target_cell(t),
        parse_other_cell(t),
    )?;
    parse_blank(t);
    parse_newline(t)?;
    Some(())
}

fn parse_line(t: &mut WithContext<impl Tokens<Item = char>, Context>) -> bool {
    parse_blank(t);
    one_of!(t;
        parse_target(t).is_some(),
        parse_metadata(t).is_some(),
        parse_io(t).is_some(),
        parse_cell(t).is_some(),
        parse_newline(t).is_some()
    )
}

#[derive(Debug)]
pub struct ParseError {
    source: String,
    offset: usize,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to parse near offset {}: {:?}", self.offset, &self.source[self.offset..])
    }
}

impl std::error::Error for ParseError {}

// Primarily used for testing, since an unregistered target can be provided directly.
pub fn parse(target: Option<Arc<dyn Target>>, source: &str) -> Result<Design, ParseError> {
    let context = Context::new(target);
    let mut tokens = source.into_tokens().with_context(context);
    while parse_line(&mut tokens) {}
    parse_blank(&mut tokens);
    let (mut tokens, context) = tokens.into_parts();
    if !tokens.eof() {
        return Err(ParseError { source: String::from(source), offset: tokens.location().offset() });
    }
    Ok(context.apply())
}

impl FromStr for Design {
    type Err = crate::ParseError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        crate::parse(None, source)
    }
}
