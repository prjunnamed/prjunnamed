Language reference
==================

Project Unnamed is based around an [intermediate representation][ir]: a kind of compiler developer readable and writable programming language designed for convenience of implementing transformations. These transformations include optimization, technology mapping, verification, and more.

The Project Unnamed intermediate representation is called "Unnamed IR", and it has two forms: the in-memory form composed out of the data structures in the [prjunnamed-netlist][] crate, and the text form described in this document. The text form is a part of the public interface, and incompatible changes to it are considered breaking changes to Project Unnamed itself.

<div class="warning">

This document is incomplete.

</div>

[ir]: https://en.wikipedia.org/wiki/Intermediate_representation
[prjunnamed-netlist]: /api/prjunnamed_netlist/


## Typographical conventions {#typography}

The examples of the text IR in this document include placeholders delimited by the `<` and `>` characters. These characters are only used to denote placeholders, and do not appear in the syntax of the text IR itself. For example:

```
%<cell-idx>:<width> = and <a> <b>
```

The snippet above describes the syntax of the `and` cell, including the fixed elements (the percent, colon, and equals characters, and the `add` keyword) and the variable elements (cell index `<cell-idx>`, output width `<width>`, arguments `<a>` and `<b>`).

Whenever a syntactic element is followed by the `...` characters, it means that it can be repeated an arbitrary amount of times, with the repetitions delimited by [whitespace](#whitespace). For example:

```
[ <part>... ]
```

The snippet above describes the syntax of a [concatenation](#concat), where it is equivalent to any of the following:

```
[]
[<part>]
[ <part> <part> ]
```

And so on.


## General principles {#general}

The text IR is designed for ease of reading of large, unstructured netlists, and for ease of writing or generating them, in that order. It is partly inspired by the well-known [LLVM IR][llvm-langref].

All introduced identifiers are namespaced, with the namespace indicated by a sigil (a prefix character like `!` or `%`). For identifiers that refer to objects that can be referenced in part, each reference unambiguously indicates the size of the partial reference so that it is not necessary to consult the declaration to find it out.

[llvm-langref]: https://llvm.org/docs/LangRef.html


## Lexical structure {#lexical}

The text IR has a conservative lexical structure, where the first character of a non-whitespace sequence of characters determines its lexical category.

The byte encoding of the text IR is always [UTF-8].

[utf-8]: https://www.unicode.org/versions/Unicode16.0.0/core-spec/chapter-2/#G11165


### Whitespace {#whitespace}

The only characters recognized as whitespace are `U+0020 SPACE`, `U+0009 CHARACTER TABULATION`, and `U+000A LINE FEED`. All of them may be used for separating other tokens, and `U+000A LINE FEED` is used to indicate the end of a syntactic construct. The file must end with a `U+000A LINE FEED` character.

A `U+000D CARRIAGE RETURN` character followed by a `U+000A LINE FEED` character is treated as the latter character alone.

<div class="warning">

The [prjunnamed-netlist] parser currently handles whitespace in a questionable and inconsistent way. This will be fixed in the future.

</div>


### Comments {#comment}

```
;<text>
```

*Comments* are sequences of characters starting with `;` and ending with `U+000A LINE FEED`. They are treated the same as a `U+000A LINE FEED` character.


### Strings {#string}

```
"<bytes>"
```

For example:

```
""
"meow"
"this\0ais a anewline"
"\22quoted\22"
"\5cescaped"
```

*Strings* are delimited by `"` on both ends and denote byte sequences (not character sequences). Within a string, `` \ `` followed by two hexadecimal digits `0`..`9` or `a`..`f` (lowercase only) corresponds to that particular byte, and any other character corresponds to its unique [UTF-8] encoding. No normalization is performed.

A string containing a `` \ `` followed by two characters that are not hexadecimal digits is ill-formed.


### Decimal numbers {#decimal}

```
#<digits>
```

For example:

```
#0
#10
#-1
```

*Decimal numbers* start with a `#` sigil, followed by an optional `-` sign and a non-empty sequence of `0`..`9` digits. No whitespace is allowed between any of the characters.

In some contexts, only non-negative decimal numbers, or decimal numbers up to a certain magnitude, may be accepted.

Decimal numbers are not interchangeable with [constants](#constant); where both are accepted, the meaning of the syntactic construct differs between the two.


### Constants {#constant}

```
<trits>
```

For example:

```
0
1
X
100
1XX0
XXXX
```

*Constants* are made up of `0`, `1`, and `X` (uppercase only) characters. No whitespace is allowed between any of the characters.

Constants denote bit vectors with some positions being "don't care" or "undefined" (depending on the context), and are written in the usual order where the most significant position is on the left. That is, `1010` has the numeric value 10 decimal, its first bit is `0` and last bit is `1`.

Constants are not interchangeable with [decimal numbers](#decimal); where both are accepted, the meaning of the syntactic construct differs between the two.


### Metadata identifiers {#metadata}

```
!<index>
```

For example:

```
!0
!15
```

*Metadata identifiers* start with a `!` sigil followed by a non-empty sequence of `0`..`9` digits, where leading `0` digits are insignificant. No whitespace is allowed between any of the characters.


### I/O identifiers {#io}

```
&<string>
&<string>:<width>
&<string>+<offset>
&_
&_:<width>
```

For example:

```
&"pin"
&"gpio":8
&"gpio"+3
&_
&_:16
```

*I/O identifiers* start with a `&` sigil followed by a non-empty [string](#string) (in which case the I/O identifier is *named*) or a `_` character (in which case it is *floating*). No whitespace is allowed between the sigil and the string or `_` character, or any of the following characters for the cases below.

Named I/O identifiers may be followed by a `:` character and a non-empty sequence of `0`..`9` digits, which denotes the width of the declaration or the reference.

Alternatively, named I/O identifiers may be followed by a `+` character and a non-empty sequence of `0`..`9` digits, which denotes a 1-wide reference at the given offset.

Floating I/O identifiers may be followed by a `:` character and a non-empty sequence of `0`..`9` digits, which denotes the width of the reference. They are used in [concatenations](#concat) to declare a partial connection to an I/O port.


### Cell identifiers {#cell}

```
%<index>
%<index>:<width>
%<index>+<offset>
%<index>+<offset>:<width>
%<index>:_
```

For example:

```
%0
%1:4
%1+0
%1+2:2
%2:0
%3:_
```

*Cell identifiers* start with a `%` sigil followed by a non-empty sequence of `0`..`9` digits, optionally followed by a `+` character and a non-empty sequence of `0`..`9` digits (denoting an offset into the wide output of a cell), optionally followed by a `:` character and a non-empty sequence of `0`..`9` digits (denoting the width of a cell declaration or reference). No whitespace is allowed between any of the characters.

A *placeholder* cell identifier starts with a `%` sigil followed by a non-empty sequence of `0`..`9` digits, a `:` character, and a `_` character. No whitespace is allowed between any of the characters. The placeholder cell identifier is only used when declaring cells with multiple outputs.

<div class="warning">

While the in-memory IR has the property that a cell with a wide output occupies a contiguous range of cell indices (i.e. that `%0+1` and `%1` always refer to the same net), the same is not true in the text IR. Instead, each unique `<index>` can refer to a distinct cell with an output of any width.

To avoid confusion, the IR printer ensures that in references, `<index>` is equal to the lowest index occupied by a cell with a wide output (i.e. it will never format `%0+1` as `%1`).

</div>


### Repetitions {#repeat}

```
<value>*<count>
```

For example:

```
X*0
%0*4
XXX*8
```

*Repetitions* start with a [constant](#constant) or a [cell identifier](#cell), followed by the `*` character and a non-empty sequence of `0`..`9` digits. No whitespace is allowed between `<value>`, `*`, and `<count>`.


### Concatenations {#concat}

```
[ <part>... ]
```

For example:

```
[]
[ ]
[&"gpio":1 &"gpio":0]
[ &"pin" &_ ]
[1001]
[10 01]
[ %0 %1:4 000 ]
[ XXXX ]
[ %0*4 %0:4 0*4 ]
```

*Concatenations* are delimited by `[` and `]` characters on either end and contain zero or more parts delimited by [whitespace](#whitespace). The first part of the concatenation in the source code order corresponds to the most significant positions of the resulting reference. The two types of concatenations are *I/O concatenations* and *value concatenations*.

Each part of an I/O concatenation must be an [I/O identifier](#io), which could be named or floating.

Each part of a value concatenation must be a [constant](#constant), a [cell identifier](#cell), or a [repetition](#repeat).


### Value references {#value}

For example:

```
0
1010
XXX
[ 10 %5 ]
%5:10
%1*10
[]
```

*Value references* are used as operands within [cell declarations](#cell-decls). A value reference must be a [constant](#constant), a [cell identifier](#cell), a [concatenation](#concat), or a [repetition](#repeat).

The width of a value reference can be determined solely from its syntax.


### Other special characters {#special}

Special characters `[`, `]`, `(`, `)`, `{`, `}`, `=`, and `,` are used as delimiters within [declarations](#decls).

These characters may be optionally surrounded by the [whitespace](#whitespace) characters `U+0020 SPACE` and `U+0009 CHARACTER TABULATION` without altering their meaning. In addition, the [whitespace](#whitespace) character `U+000A LINE FEED` may appear between the matching `[`, `]`, `(`, `)`, `{`, or `}` delimiter pairs, in which case it does not indicate the end of the syntactic construct.


## Header {#header}

The header must be located at the beginning of the file, after any initial [whitespace](#whitespace) or [comments](#comment). It describes how the remainder of the file must be interpreted.


### Target specification {#target}

```
set target <target> <option>=<value>...
```

For example:

```
set target "siliconblue"
set target "siliconblue" "device"="ice40hx8k"
set target "siliconblue" "device"="ice40up5k" "dsp"="off"
```

The target specification defines the specific device for which the netlist is intended, as well as target-specific configuration options for the flow. The `<target>`, `<option>`, and `<value>` are all [strings](#string) that are passed to the [builder function][target-builder] as-is.

[target-builder]: /api/prjunnamed_netlist/fn.register_target.html


## Declarations {#decls}

*Declarations* are syntactic constructs that, taken together, describe the entire netlist. There are several kinds of declarations:
- [*metadata declarations*](#metadata-decls),
- [*I/O declarations*](#io-decls),
- [*cell declarations*](#cell-decls).

The general structure of a declaration is:

```
<ident> = <keyword> <operand>...
```

For example:

```
!1 = ident "clk" in=!0
%0:1 = input "clk" !2
```

In the snippet above, `!1` and `%0:1` are identifiers *introduced* by the declaration, `ident` and `input` are *keywords*, and `"clk"`, `in=!0`, and `!2` are *operands*.

A [metadata identifier](#metadata) can only reference a metadata declaration introduced at an earlier point in the file. In contrast, a [cell identifier](#cell) can reference a cell declaration introduced anywhere in the file, including by the cell declaration it is an operand of.


### Metadata declarations {#metadata-decls}

Metadata declarations correspond to the [enum prjunnamed_netlist::MetaItem](/api/prjunnamed_netlist/enum.MetaItem.html).

<div class="warning">

The names that occur within metadata are opaque: they must not be examined by the tools for any reason other than communicating it to the user or to other tools.

In particular, it is forbidden to interpret names with certain suffixes such as `inst[0]` or `wire[2:0]` in a special way. If necessary, this may be done prior to generating Unnamed IR.

</div>


#### Metadata sets {#metadata-set}

```
!<decl> = { !<ref> !<ref>... }
```

For example:

```
!10 = { !2 !3 }
!11 = { !2 !3 !4 }
```

*Metadata sets* are used to group multiple pieces of metadata to be referenced in [cell declarations](#cell-decls) with a single index.

A metadata set with less than two elements is ill-formed. A metadata set that refers to another metadata set is ill-formed.


#### Source metadata {#metadata-source}

```
!<decl> = source "<file>" (#<start-line> #<start-col>) (#<end-line> #<end-col>)
```

For example:

```
!1 = source "/home/whitequark/design/top.py" (#20 #4) (#20 #10)
```

*Source metadata* is used to refer to a contiguous possibly empty range of characters within a source file.

The `"<file>"` [string](#string) is an environment specific location of a source file, which may be absolute or relative. It cannot be empty, or the declaration is ill-formed.

The `#<start-line>` and `#<start-col>` [decimal numbers](#decimal) are zero-based non-negative indices into the contents of `"<file>"`, where `#<start-line>` is the number of `U+000A LINE FEED` characters before the start of the range, and `#<start-col>` is the number of the Unicode code points after the last `U+000A LINE FEED` character and before the beginning of the range.

The `#<end-line>` and `#<end-col>` [decimal numbers](#decimal) are zero-based non-negative indices defined in the same way, except they point to the end of the range. The index `#<end-line>` cannot be less than `#<start-line>`, and if the two are equal, the index `#<end-col>` cannot be less than `#<start-col>`, or the declaration is ill-formed.

If the IR generator does not have access to accurate location information, the range may be left empty. If only line number information is available, the range should be specified as `(#<line> #0) (#<line> #0)`.


#### Scope metadata {#metadata-scope}

```
!<decl> = scope "<name>" in=!<parent> src=!<source>
!<decl> = scope #<index> in=!<parent> src=!<source>
```

For example:

```
!10 = scope "top"
!11 = scope "cpu" in=!10
!12 = scope "alu" in=!11 src=!0
!13 = scope "io" src=!1
!14 = scope #0 in=!13
!14 = scope #1 in=!13 src=!2
```

*Scope metadata* is used to describe a hierarchy of regions within the source code. The nature of a region is not constrained here, and includes without limitation: modules, conditional blocks, instance declarations, and so on. By traversing the chain of scope metadata, it should be possible for tools like timing analyzers, source level debuggers, and so on to be able to unambiguously locate a declaration by its hierarchical name.

There are two kinds of scopes: *named scopes* and *indexed scopes*. Indexed scopes typically correspond to source-level declarations of arrays, whereas named scopes correspond to any other source-level declarations. Negative indices are accepted for indexed scopes.

Hierarchy arises when a scope is declared to be a part of another scope using the optional `in=!<parent>` operand. The `!<parent>` [metadata identifier](#metadata) must refer to scope metadata, or the declaration is ill-formed.

A scope may have a [source location](#metadata-source) attached using the optional `src=!<source>` operand. The `!<source>` [metadata identifier](#metadata) must refer to source metadata, or the declaration is ill-formed.

If the `"<name>"` [string](#string) is empty, the declaration is ill-formed.


#### Identifier metadata {#metadata-ident}

```
!<decl> = ident "<name>" in=!<scope>
```

For example:

```
!1 = ident "clk" in=!0
```

*Identifier metadata* is used to refer to a specific declaration within an elaborated hierarchy. The full hierarchical name is obtained by collecting the names or indices while recursively traversing [scope metadata](#metadata-scope) referenced by the `!<scope>` [metadata identifier](#metadata) and appending the `"<name>"` [string](#string) at the end.

If the `"<name>"` is empty, or the `!<scope>` does not refer to scope metadata, the declaration is ill-formed.


#### Attribute metadata {#metadata-attr}

```
!<decl> = attr "<name>" <const>
!<decl> = attr "<name>" #<int>
!<decl> = attr "<name>" "<string>"
```

For example:

```
!0 = attr "top" #1
!1 = attr "PIN_TYPE" 110000
!2 = attr "BEL" "X0/Y1"
```

*Attribute metadata* is used to encapsulate source-level attributes that are not otherwise recognized by the target-independent or target-specific parts of the flow. An attribute has a `"<name>"` [string](#string) and a payload. There are three possible types of payloads: [constants](#constant), [integers](#decimal), and [strings](#string).

Attribute metadata is strongly typed: the tools preserve the type of the payload and ensure it matches the expected type whenever the payload is examined.

If the `"<name>"` is empty, the declaration is ill-formed.


### I/O declarations {#io-decls}

```
&"<name>":<width> = io
```

For example:

```
&"clk":1 = io
&"gpio":8 = io
```

*I/O declarations* are used to specify the width of an I/O port (using an [I/O identifier](#io) in the form specified above).

If the `"<name>"` is empty, or a preceding I/O declaration uses the same `"<name>"`, the declaration is ill-formed.


### Cell declarations {#cell-decls}

Cell declarations correspond to the [enum prjunnamed_netlist::Cell](/api/prjunnamed_netlist/enum.Cell.html).

To be written.
