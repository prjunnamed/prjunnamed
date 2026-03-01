- Feature Name: debuginfo
- Start Date: 2026-02-16
- RFC PR: [prjunnamed/prjunnamed#0000](https://github.com/prjunnamed/prjunnamed/0000)

# Summary
[summary]: #summary

Add a well-defined debuginfo model to the prjunnamed IR, describing the nets and registers present in the netlist, their mapping to the original HDL code, and their data types.

# Motivation
[motivation]: #motivation

The core job of prjunnamed is to synthesize the netlist into target primitives and emit a bitstream performing the described function.  However, it is important that the resulting netlist can be navigated by the user when diagnosing an issue.  When the design fails to meet the timing constraints, the user must be able to make sense of the critical path presented by the tool and how it relates to the HDL they wrote.

As always, when an optimizing compiler is involved, such mapping may be partial and will be maintained on a best-effort basis: the compiler may decide to optimize out an unused net, or transform it in such a way that recovering its original value is impossible (such as retiming).

Further, there are secondary usecases where a processed netlist will be manipulated by tools that need to understand how it relates to the original HDL.  These include (but are not limitted to):

1. Direct simulation.  The design is not mapped into target primitives (a target may not be selected at all).  Some lightweight optimization may or may not be performed to improve simulation speed.

   1. A waveform file (such as VCD) will be emitted.  The signals in the waveform file correspond to nets from the original design, and should be named as such.  Furthermore, the type information of the signal should be used to format the signal values.
   2. The testbench (the external client process running the simulation) can access the internal signals of the design by name: it can read the values of the nets, and it can write the values of registers.

2. Post-synthesis simulation: like above, but the full synthesis flow is run, and simulation is performed on the final netlist.

3. In-circuit debugging: the full synthesis flow is run, and the bitstream is loaded on the device.  By the means of some debug interface, the device is halted and the current values of nets and registers are read and/or modified.  Such debug interfaces include:

   1. FPGA vendor-specific debug via JTAG or similar interfaces (shut down the FPGA, capture state of registers, read back the bitstream, potentially modify the bitstream and strobe the new values into registers).
   2. Automatically inserted scan-chains (a future possibility).

4. Late binding of memory contents: a block RAM that stores the boot code for a SoC is left uninitialized in HDL, and the synthesis and P&R are run without the boot code present.  The actual boot code is provided late, substituting the BRAM initialization values in post-P&R netlist, right before emitting the bitstream.

   Note that this usecase can be considered to be a special form of in-circut debugging: the debuginfo provides the mapping of the original HDL memory bits to physical bitstream bits containing the BRAM contants, and the bitstream is modified.  The only difference is that it happens before the bitstream is loaded into the device.

5. Formal verification: just like the first testcase, no target-specific flow is run, and the design is transformed into a form suitable for formal tools.  The assertion failure and other models emitted by the formal tool must be mapped back to the original HDL signals.

6. Netlist export: it is expected that, when feasible, nets and cells in an exported netlist will keep the names used to define them in original HDL.  Likewise, the hierarchy should be preserved.

7. Applying user constraints (floorplanning, I/O pad mapping, timing constraints) to the design.

Depending on the exact usecase, the user may want several optimization levels (which affect how well the debug information will be preserved):

1. Best-effort: an optimization can remove a named net or register, or transform it in a way that makes it impossible to recover the original value.  A "tombstone" debuginfo is maintained in the IR, marking an optimized-out object.
2. Preserve selected: specific nets or registers (such as the ones that are known to be accessed in testbench code, or supposed to contain late-bound firmware) are marked as "preserved".  They must be transformed only in ways that keep them functionally equivalent:

   - if a net is preserved, it must be possible to compute its value from the current state of the design
   - if a register or memory is preserved, it must be possible to modify its current value, and have the design use the new value for all purposes
     - corollary: even if a preserved register is obviously always 0 (it is initialized to 0, and the D input is tied to 0), the synthesis flow must not delete it, and must not assume its value to be 0 in any optimization passes (since the debugger may want to set it to 1)

3. Preserve all: all named nets and registers are considered to be preserved as above.


# Guide-level explanation
[guide-level-explanation]: #guide-level-explanation

The debug information model in prjunnamed is composed of three interconnected layers:

1. A tree of *scopes* describing the hierarchy of the design (corresponding to HDL modules and blocks).  Scopes provide the skeleton to which named objects are attached.
2. Named objects:

   - *named nets*, which are points that allow a value to be read
   - *named signals* and *named registers*, which are points that allow a value to be read or written
   - *named cells*, which allow specific target cell instances to be located for the purpose of access by other tooling (the nature of which we leave unspecified)
   - *ports*, which are represented as named nets, signals, or registers with a special annotation

3. *Data types*, which describe how the values of named nets and registers are to be interpretted or displayed.

## Scopes

The prjunammed IR is flat by nature: there are no modules, only a single design.  Scopes are a type of metadata that describes the hierarchy of the design as originally elaborated, and can be used to associate named objects and IR cells with their location in the hierarchy.  Every scope and named object has a *hierarchical name*, which is unique in the design.  The hierarchical name is essentially a path from the root scope, consisting of names and indices of all parent scopes.

Scopes come in three kinds:

- the singular implicit *root scope*, which has an empty hierarchical name (corresponds to `$root` in verilog)
- a *named scope*, which is contained in another scope, and is identified by a unique *name* within it.  A name can be any UTF-8 string, though it is usually expected to be a valid identifier.
- an *indexed scope*, which is contained in another scope, and is identified by a unique *index* within it.  The index can be an arbitrary `i64` integer.

Consider an example hierarchy:

```
(* top *)
module top;
    genver i;
    for (i = 0; i < 3; i = i + 1) begin: loop
        xyz inst0;
        xyz inst1;
    end

    abc inst2[2];
endmodule

module abc;

    a: begin
    end

endmodule

module xyz;
endmodule
```

It would be described as follows in the IR:

```
# a named scope, within the implicit root scope, representing the top module instance
!1 = scope "top" type=module("top")
# a named scope, within the above top scope, representing the loop
!2 = scope "loop" parent=!1 type=array
# an indexed scope representing a single iteration of the loop
!3 = scope #0 parent=!2 type=block
!4 = scope "inst0" parent=!3 type=module("abc")
!5 = scope "inst1" parent=!3 type=module("abc")
!6 = scope #1 parent=!2 type=block
!7 = scope "inst0" parent=!6 type=module("abc")
!8 = scope "inst1" parent=!6 type=module("abc")
!9 = scope #2 parent=!2 type=block
!10 = scope "inst0" parent=!9 type=module("abc")
!11 = scope "inst1" parent=!9 type=module("abc")
# a named scope representing a module instance array
!12 = scope "inst2" parent=!1 type=array
# an indexed scopr representing a specific instance in the array
!13 = scope #0 parent=!12 type=module("xyz") 
!14 = scope "a" parent=!13 type=block
!15 = scope #1 parent=!12 type=module("xyz") 
!16 = scope "a" parent=!15 type=block
!17 = scope #2 parent=!12 type=module("xyz") 
!18 = scope "a" parent=!17 type=block
```

The hierarchical names of the scopes (written in Verilog convention) would be:

- `!1`: top
- `!2`: top.loop
- `!3`: top.loop[0]
- `!4`: top.loop[0].inst0
- `!5`: top.loop[0].inst1
- `!6`: top.loop[1]
- `!7`: top.loop[1].inst0
- `!8`: top.loop[1].inst1
- `!9`: top.loop[2]
- `!10`: top.loop[2].inst0
- `!11`: top.loop[2].inst1
- `!12`: top.inst2
- `!13`: top.inst2[0]
- `!14`: top.inst2[0].a
- `!15`: top.inst2[0]
- `!16`: top.inst2[0].a
- `!17`: top.inst2[0]
- `!18`: top.inst2[0].a

Scopes can have one of several predefined types (module, block, interface, struct, array).  They can also have source locations.  Scopes are metadata and as such can be attached to cells to signify their origin, but their main role is to contain named objects.

Module scopes are the main building blocks of hierarchy.  Block scopes represent subdivisions of logic within a module.  Interface scopes are used to group together related module ports.  Struct scopes are used to represent a group of nets, signals or registers that was a single named struct-typed object in the HDL.  Array scopes represent arrays of any of the other type of scopes.

## Named objects

Named objects represent "points of interest" that can be referenced by their hierarchical name by tools operating on the netlist, such as a simulator testbench accessing the state of the design.

### Named nets

A *named net* associates a name with a value in the netlist.  For example, it can represent a wire:

```
module top (input[3:0] a, b);
wire[3:0] y = a + b;
endmodule
```

```
!1 = scope "top"
%0:4 = [...] # a
%1:4 = [...] # b
%2:5 = adc %0:4 %1:4 0
# the metadata declares a net of width 4 in a scope
!2 = net "y" width=4 scope=!1
# the name cell is used to actually associate a value with the named net
%3:0 = name %2:4 !2
```

Named nets may be *unavailable*: if the value has been optimized out, it is no longer possible to read it from the design state.  This is represented by a net metadata without an associated name cell.

A name cell may be *partial*, and describe a slice of the associated named net.  This is used where the value may be only partially known:

```
# as above, but two top bits have been optimized out
%2:3 = adc %0:2 %1:2 0
!2 = net "y" width=4 scope=!1
%3:0 = name %2:2 !2+0:2
```

There may be several name cells associated with a net, to cover disjoint slices of a partially known value.

Normally, a name cell is effecively a "weak reference" to a value: the flow can delete a name, or any subset of its bits, if it is unused or stands in the way of optimization.  To prevent a value from being optimized out (making the name cell a strong reference), a net may be marked as "keep":

```
!2 = net "y" width=4 scope=!1 keep
%3:0 = name %2:4 !2
```

The name is associated with a value, not with the cell driving it.  Even if marked as "keep", the flow is free to optimize the logic driving it in any way that preserves its semantics.  For example, if it determines that a net always have a constant value, one may end up with a named constant:

```
!2 = net "y" width=4 scope=!1
%3:0 = name 0101 !2
```

### Named signals

A *named signal* is a variant of a named net that can also be used to *drive* a value into the design.  In normal circumstances, it acts as a buffer forwarding its input to output.  However, it can be overriden by an external entity to drive a different value:

```
wire[3:0] x = a + b;
wire[3:0] y = x + c;
```

```
%0:4 = [...] # a
%1:4 = [...] # b
%2:4 = [...] # c
# add a + b
%3:5 = adc %0:4 %1:4 0
# the named signal, consuming the above value
%4:4 = signal %3:4 "x" scope=!1
# add x + c
%5:5 = adc %4:4 %2:4 0
```

A named signal acts as an optimization barrier: the flow cannot assume that the value of `x` flowing into the second addition is the same as the result of the first addition, as it could be overriden by something external.

Named signals are a debugging tool that is not normally used, and should only be emitted by explicit user request.  They always act as a strong reference, and will not be optimized out.


### Named registers

A *named register* associates a name with writable storage elements in the netlist, such as flip-flops:

```
reg [3:0] q = 0;
always @(posedge clk)
    q <= d;
```

```
%0:1 = <clk>
%1:4 = <d>
!2 = reg "q" width=4 scope=!1
%2:4 = dff %1:4 clk=%0 init=0 name=!2
```

Other storage elements that can be associated with named registers include:

- the data of the `memory` cell (as far as debuginfo is concerned, a memory is just a very wide flip-flop)
- synchronous memory read ports
- elements of target cells (as specified in the target definition), for example:
  - the contents of a target FF cell
  - all pipeline registers of a DSP cell
  - BRAM: the memory contents, the read port latches, and the pipeline registers
- an inverted version of any storage element (to support technology mapping steps that require a value to be stored inverted)

The technology mapping process will take care to preserve the names as the cells are being lowered to target-specific versions.

A given bit of a named register may be associated with more than one storage element in the netlist.  This is used to represent registers or memories that were duplicated as part of the synthesis flow — when such a register is being written by an external tool, it must write all copies of the register for correct behavior.

Like named nets, by default named registers are considered to be "weak references" and can be destroyed by optimizations whenever convenient.  This can be changed with one of the "keep" annotations:

1. `keep`: the register must remain both readable and writable by external entities.  Every bit of the register must be mapped to at least one bit of storage in the netlist, and (like `signal`) the output of the register must be treated as an optimization barrier (to ensure an externally written value is properly observed)
2. `keep_write`: the register must remain writable by external entities, but read access is not required.  Behaves like `keep`, but allows the synthesis tool to delete bits that are unused by the netlist (an external entity can write to them, which is treated as a no-op).
3. `keep_read`: the register must remain readable by external entities, but write access is not required.  If convenient for optimization, this register (or any subset of its bits) can be "decayed" into a named net with the `keep` annotation.
4. Default: the register or any subset of its bits can be deleted, or decayed into a named net, as necessary.


### Representing ports

Ports between modules in the hierarchy are represented by named nets, signals, or registers with specific annotations:

```
module top;
    wire ia, iy;
    inv inst0(.a(ia), .y(iy));
endmodule

module inv(input a, output y);
    assign y = !a;
endmodule
```

```
!1 = scope "top" type=module("top")
!2 = scope "inst0" type="module("inv") parent=!1
%0:1 = [...] # ia
%1:1 = signal %0 "a" scope=!2 input
%2:1 = not %1
%3:1 = signal %2 "y" scope=!2 output
```

Likewise, named signals are used for top-level ports, obsoleting the current `input` and `output` cells:

```
module top(input a, output y);
    assign y = !a;
endmodule
```

```
!1 = scope "top"
# top-level input port represented as a signal cell with undefined input
%0:1 = signal X "a" scope=!1 input
%1:1 = not %0
# top-level output port represented as a signal cell with unused output
%2:1 = signal %1 "y" scope=!2 output

A named register can be used to represent ports when an output is directly driven by a register (such as Verilog `output reg ...`).

The way to represent ports depends on the debugging needs.  In the usual circumstances (synth flow with no ICD features requested):

- internal ports: represented as nets, subject to optimization (can be just deleted)
  - opportunistically, output ports can also be represented as registers if there happens to be one driving the port with the same name, likewise subject to optimization
- top-level ports:
  - top-level outputs: represented as a `signal` (with its output value unused); cannot be deleted
    - alternatively, a net with `keep` would effectively work the same
  - top-level inputs: represented as a `signal` (with input of `X`, as it is expected to be overriden); cannot be deleted

In a full-debug flow, the internal ports would be represented as signals (or registers) instead of nets.


### Named cells

A "named cell" associates a name with an instance, or a target cell.

```
module top;
DSP48E2 mydsp([...]);
endmodule
```

```
!1 = scope "top"
!2 = cell "mydsp" scope=!1
%_ = target "DSP48E2" !2 {
    ...
}
```

A named cell can be marked as `keep`, which prevents the flow from deleting or transforming it.


## Type system

The final layer of debuginfo deals with describing the *types* of the named nets, signals, or registers.  The type system is only used for presentation, and has no effect on the synthesis flow — as far as synthesis semantics are concerned, the only type that exists is vectors of bits.

The following types are supported:

- simple types:
  - bool
  - unsigned integer
  - signed integer
  - abstract bitvector
- composite types:
  - enum
  - structure
  - tagged union
  - array

### Simple types

The `bool` type can be used only on single-bit values.  The values are displayed as `true` or `false`.

```
!1 = type bool
!2 = reg "a" type=!1
```

The `signed` and `unsigned` types can be used on values of any width.  The values are displayed as decimal by default.

The `bitvec` type can be used on values of any width.  The values are displayed as hex or binary.  If a named net or register has no metadata, the `bitvec` type is assumed.


### Enum types

Matches the value to a predefined list to display it.  Has an associated width, and can only be used on values of that width.

```
!1 = type enum #4 {
    "OUT" = 0001
    "IN" = 1001
    "SOF" = 0101
    "SETUP" = 1101
    "DATA0" = 0011
    "DATA1" = 1011
    "DATA2" = 0111
    "MDATA" = 1111
    "ACK" = 0010
    "NAK" = 1010
    "STALL" = 1110
    "NYET" = 0110
    "PRE" = 1100
    "ERR" = 1100
    "SPLIT" = 1000
    "PING" = 0100
}
!2 = reg "pid" width=4 type=!1
```

### Structure types

Display the value by splitting it into fields, then recursing into fields.  Has an associated width, and can only be used on values of that width.  The starting offset of each field is given explicitly.  Unions can be described as structs where each field happens to be at offset 0.

```
!1 = type unsigned
!2 = type struct #16 {
    field "b" start=#0 width=#5 type=!1
    field "g" start=#5 width=#6 type=!1
    field "r" start=#11 width=#5 type=!1
}
```

### Array types

Used for memories, as well as packed arrays within structs.

```
reg [31:0] gprs[32];
```

```
!1 = type bitvec
!2 = type array width=#32 depth=#32 type=!1
!3 = reg "gprs" width=#1024 type=!2
%0:_ = memory width=#32 depth=#32 {
    name !3:1024
    [...]
}
```


### Tagged union

Compares the value against a list of bit patterns, takes the first matching one, then decodes the value as a structure according to the matched variant.

```
!1 = type union #16 {
    011000xxxxxxxxxx = "ADD" {
        field "dst" start=#0 width=#5
        field "src" start=#5 width=#10
    }
    1011xxxxxxxxxxxx = "JUMP" {
        field "target" start=#0 width=#12
    }
    [...]
}
```

## Simulator interface

This RFC does not define a particular simulator or ICD interface.  However, we provide a guideline for the abstract operations such interfaces can provide.

A simulator interface has the following operations:

- hierarchically iterate available scopes and named objects
- get meta-information about scopes and named objects (names, types, widths, input/output marks, other attributes)
- read the current value of a named net
  - input: hierarchical name (must refer to a net)
  - input: slice of the value to read (ie. can select all of the value, or just a subset of bits)
  - output: for each bit of the selected slice of the net, one of the following:
    - the value of the net (`0`, `1`, `X`)
    - "value not available" error: there is no value in the netlist corresponding to the named net, or such value is inaccessible (eg. for ICD if the value is not at a location that can be tapped)
- read the current value of a named signal: similar to the above
  - TODO: if the signal is overriden, do we return the cell input, or the current override? both?
- override a signal:
  - input: hierarchical name (must refer to a signal)
  - input: slice of the signal to override
  - input: value to be driven onto the signal slice (must have width matching signal)
  - from now on, the `signal` cell in the netlist will have the given value on output, until explicitly released or overriden again
- release a signal:
  - input: hierarchical name (must refer to a signal)
  - removes the override previously put on the signal, if any; from now on, the `signal` cell once again passes its input through to its output
- read the current value of a named register: similar to reading a named net or signal
- write the value of a named register
  - input: hierarchical name (must refer to a register)
  - input: slice of the register to write
  - input: value to put into the register slice
  - the register slice is instantaneously set to the given value, similar to what happens on an active and enabled clock edge
  - as opposed to overriding named signals, this does *not* result in a persistent override: the value written via the debug interface will be replaced by the next active clock edge, or similar triggers from the netlist
  - in particular, this is a no-op if an async reset is currently active (as the reset will immediately flip the changed value back)
  - an error is returned if any of the selected register bits is not writable
- run target-specific or simulator-specific operation on a named cell
  - input: hierarchical name (must refer to a named cell)
  - example: "set the analog input on this ADC cell to the given f64"

The above set of available operations is just a guideline, and may be heavily adapted to the given usecase (eg. operating on symbolic values for formal verification).


# Reference-level explanation
[reference-level-explanation]: #reference-level-explanation

The following metadata items are added to the IR:

- `scope <name or index> [parent=<scope>] [type=<scope type>] [src=<source>]` (slight modification to existing metadata)
  - `type=module[(<name>)]`
  - `type=block`
  - `type=interface[(<name>)]`
  - `type=array`
  - `type=struct[(<name>)]`
- `net <name or index> [scope=<scope>] [width=<int>] [type=<type>] [lsb=#<num>] [from_msb] [src=<source>] [keep] [input | output]`
- `reg <name or index> [scope=<scope>] [width=<int>] [type=<type>] [lsb=#<num>] [from_msb] [src=<source>] [keep | keep_read | keep_write] [decayed=<mask>] [output]`
- `cell <name or index> [scope=<scope>] [keep]`
- `type (bool | bitvec | unsigned | signed)`
- `type enum #<width> { <values> }`
- `type struct #<width> { <fields> }`
- `type union #<width> { <variants> }`
- `type array width=#<num> depth=#<num> [type=<type>]`

The following metadata items are removed:

- `ident` (replaced by `net` and `cell`)

The following cells are added to the IR:

- `%<id>:<width> = signal <value> <name or index> [scope=<scope>] [type=<type>] [lsb=#<num>] [from_msb] [input | output]`
- `%<id>:0 = name <value> <meta or meta slice>` (replaces current name cell)

The following cells are removed from the IR:

- `input` (replaced by `signal`)
- `output` (replaced by `signal`)
- `name` (replaced by the new `name` cell with different semantics)
- `debug` (replaced by `name`)

Furthermore, an optional `reg` metadata reference is added to the following cells:

- `dff`
- `memory` (memory contents and synchronous read registers)
- `target`


## Scopes

Scopes must form a tree.  A given hierarchical name must correspond to at most one scope or named object.

The IR puts no constraints on the names beyond being valid UTF-8 strings.

Usually there will be exactly one top-level module scope containing all other scopes and named objects in the design (the `top` module).  However, this is not a hard requirement.  It is allowed to have multiple top-level scopes. It is also allowed, though heavily discouraged, to put named objects directly inside the root scope.

The following scope types are defined:

- `module` or `module(<name>)`: represents a Verilog `module` instance, a VHDL `entity` instance, or a similar construct; optionally, the name of the module type may be given for informational purposes
- `block`: represents a block within a module; corresponds to a Verilog `block`
- `struct` or `struct(<name>)`: represents a struct-typed signal, wire, or similar construct that got split into individual named nets or signals by the backend; the name of the struct type may be given for informational purposes
- `interface` or `interface(<name>)`: represents a grouping of module's input / output ports; like `module`, the name of the interface type may be given for informational purposes
- `array`: represents an array of blocks, interfaces, or module instances

It is expected that indexed scopes will only be contained within a scope of type `array`, which may be a named scope or another (nested) indexed scope.  Likewise, it is expected that `array` scopes will contain only indexed scopes and named objects.  However, this is not a hard requirement.

The IR has two distinct tools to represent composite data types: the `struct` scope type, which represents fields as individual IR objects, and the `type struct`, which represents the entire structure as a single entity, treating fields as a matter of display processing.  The HDL frontend makes the choice between them as appropriate (the two choices roughly correspond to `packed` and `unpacked` SystemVerilog types).

The `struct` scope type has special constraints:

- it can only contain (directly or indirectly): named nets, named signals, named registers, `struct` scopes, `array` scopes

The `interface` type has special constraints:

- it can only be contained within a `module` or another `interface`, directly or inside `array` scopes (cannot be contained within a `block` or `struct`)
- it can only contain (directly or indirectly): named nets, named signals, `interface` scopes, `array` scopes, `struct` scopes (cannot contain `block` or `module` scopes, nor other kinds of cells)

Netlist exporters are expected to deal with arbitrary names and scope hierarchies.  However, scope structures and names not representable in the target format or language may not be preserved (scopes may be flattened or inserted as necessary to legalize the hierarchy; names may be mangled, escaped, or stripped).  Therefore, unusual hierarchy structures and identifier characters should be avoided.

## Named nets

A named net is defined by the metadata item.  It has the following attributes:

- the name or index of the net (indexed nets are expected to be placed in `array` scopes)
- `scope=<scope>`: the parent scope, if any
- `width=#<int>`: the size of the wire, in bits (assumed to be 1 if absent)
- `lsb=#<int>`: the HDL-level index of the LSB, for display purposes (assumed to be 0 if absent); note that prjunnamed always internally counts bits from 0
- `from_msb`: specifies that HDL-level bit indices grow from MSB up; therefore, physical bit `i` should be considered to have HDL-level index of `lsb - i` instead of `lsb + i`
  - `wire [0:63] a;` would be described as `lsb=63 from_msb`
- `src`: the source location of the net, as usual
- `type`: describes how to decode and display (or parse) values of the net
- `keep`: disallows removing `name` cells referencing this net
- `input`: marks the net as an input of the closest containing `module` scope
- `output`: marks the net as an output of the closest containing `module` scope

The named net is associated with its value by means of `name` cells in the netlist.  A `name` cell has:

- no output
- input: a value of arbitrary width
- associated named net or register
- the slice (start + length) within that name or register

A single `name` cell associates a contiguous slice of a named net with a value.  For non-contiguous slices, multiple `name` cells can be used.

The mapping of named net bits to values is many-to-many:

- a given bit of named net may have no value in the netlist, representing a bit that has been deleted by the synthesis flow
- a given bit of named net may have multiple values in the netlist, if the cells driving it have been duplicated by the flow
- a given netlist value may have multiple names, if the names were aliased in the netlist, or have been determined to be equivalent by the flow
- a given netlist value may have no names at all


## Named signals

A named signal is defined by its cell.  Since it is a non-removable non-duplicatable optimization barrier, there is no need to split the definition into a metadata item.  It has the following attributes, which are essentially the same as for named nets:

- name or index
- `scope=<scope>`
- `width=#<int>`
- `lsb=#<int>`
- `from_msb`
- `src`
- `type`
- `input`
- `output`

It is expected that design ports (which should be connected to I/O) are the ports of its top module.


## Named registers

A named register is defined in essentially the same way as a named net.  It has the following additional or modified attributes:

- `keep_read`: disallows any transformation that would prevent externally reading the register (each bit of the register that has a mapping to the netlist must retain a mapping to the netlist)
- `keep_write`: disallows any transformation that would prevent externally writing the register (each bit of the register that is associated with a storage element must remain associated with a storage element unless it is determined to have no effect on the netlist and is removed entirely; ie. prevents adding bits to the `decayed` mask)
- `keep`: combined effect of `keep_read` and `keep_write`
- `decayed=<mask>` marks a given subset of bits as "decayed", ie. non-writable and treated as named net instead of named register; such bits are associated with `name` cells in the netlist instead of storage elements
- `input`: not allowed

Various cell types are considered to contain "storage elements", which can be associated with register bits.  Register bits are mapped to storage elements in a one-to-many manner: a register bit can be associated with any number of duplicated storage elements, including zero if it has been deleted.  However, a storage element can be associated with at most one register bit (since registers cannot alias).

Individual bits of a register can be "decayed".  Whenever a transformation is made that would prevent soundly writing a bit, it is marked as "decayed", and is effectively treated as if it was a named net bit.  A decayed bit cannot be externally written.  If the transformation still allows the value to be read, the decayed bit is associated with a value by means of a `name` cell.

Storage elements can be associated to register bits in an arbitrary manner (registers can be split across many cells, and several registers can be packed into a single cell).

A `dff` cell has one storage element per bit, as expected:

```
!1 = reg "a"
!2 = reg "b" width=4

# a DFF cell; bit 0 corresponds to register "a", bits 2 and 3 correspond to LSBs of register "b", bit 1 doesn't correspond to any named register
%0:4 = dff [%xxx %xxx %xxx %xxx] clk=%xxx init=0000 name=[!3+0:2 _ !1]
```

A `memory` cell has one storage element per memory bit.  In addition, the embedded DFF associated with each synchronous read port has storage elements just like the `dff` cell.

Target cells have their storage elements defined as part of their prototype.  It is expected that target cells that can be reasonably used to map standard HDL constructs (ie. flops, DSP blocks, RAMs) will have defined storage elements as appropriate.  More exotic cells (such as funny gearboxes) may not.

## Named cells

A named cell is defined by the metadata item.  It has the following attributes:

- index or name
- containing scope
- `src`
- `keep`: disallows deleting this cell, even if unused

The actual cell is associated with its name by applying the metadata to the cell.  This is allowed only on instances and target cells.

If the cell is deleted, the named cell metadata stays in the netlist, serving as a tombstone.

TODO: does it make any sense to have several cells sharing the name?  In what circumstances would we want to duplicate a named cell?

No particular purpose is specified for named cells, other than informational.


# Drawbacks
[drawbacks]: #drawbacks

The model is very complex.

Much of the proposed functionality doesn't directly correspond to anything supported in current (or currently planned) frontends or backends.  It will take a long while before we can make full use of it, and there will always be an impedance mismatch when exporting or importing netlists.

The debuginfo is not capable of representing many HDL-level constructs one may want represented (notably, it doesn't allow faithfully recovering the exact high-level type used for a net).

The code is more what you'd call "guidelines" than actual rules.


# Rationale and alternatives
[rationale-and-alternatives]: #rationale-and-alternatives

Debug information, or metadata, is a core feature of the netlist format, and must have well-defined semantics.

By nature, debuginfo must attempt to describe the design in an HDL-agnostic way.  The hierarchy and data type system of any specific language may be unable to represent some of our data types, or have a richer system that cannot be fully represented in debuginfo.  This is a necessary compromise.

Many aspects of scopes are very loosely defined, or considered only a suggestion.  This is meant to allow language frontends the freedom to describe the input as faithfully as possible, even when it diverges from the usual shape of module-based scopes that would be used in Verilog.

The set of characters accepted in identifiers is not restricted, other than "valid unicode".  While clearly we'd want them to be something like actual identifiers in programming languages, any subset we pick would have problems:

- if the character set is restrictive, there's going to be an HDL that has permissive naming rules for identifiers, and we end up with valid HDL input with identifiers that are not representable in our model
- if the character set is permissive, there's going to be an export format (or HDL) that doesn't support some characters we do, and we end up having to escape or mangle names

We consider the first one to be a bigger problem, and therefore would rather choose a permissive option.  Since basically any exporter must be preparad to deal with unsupported characters somehow anyway, HDLs commonly support very weird characters in names (Verilog supports anything non-whitespace; VHDL can even include spaces), and we're not in the business of defining a new programming language, we choose to just allow anything.

The types in our type system are structural, and deduplicated.  An alternative would be to use nominal types (types where identity is determined by the type name, not just internal its structure).  This alternative was not chosen for two reasons:

1. Our "type system" is a sham, and is more like a "display system": the types have no actual semantics in synthesis, and are solely used for presentation.  We have no business doing any type checking (ie. comparing two types) in the first place.
2. The names of types in the source HDL may be quite complex.  In SystemVerilog, types may be contained in packages (making their names hierarchical), or even in classes (additionally making values of the class's generic parameters part of the fully qualified type name).  Representing that faithfully would take an inordinate amount of work.

The "optimization level" is determined solely by the frontend choosing whether to use named signals and `keep` attributes when emitting the initial netlists.  We do not have an actual concept of an optimization level as a paramter to the synthesis procss.  Having such a parameter that would define the netlist *semantics* (ie. what is considered a sound transformation) is something we'd rather avoid.

The many "moving parts" involved in named nets and registers (the net metadata, the name cells, and the actual value) are necessary to represent partially deleted named nets and duplicated registers.  Additionally, the full slice & concatenate capability of register mapping is necessary to faithfully describe the layout of a memory that went through the (often quite violent) memory mapping process.

The `keep_write` attribute in particular is motivated by the usecase of "link firmware image into the bitstream after P&R": it is perfectly valid to delete a ROM range that is determined to be unused (perhaps as part of an unused core), and nop out the conceptual "writes" to the unused memory bits that happen during linking, but it is not valid to do any transformations relying on the data in memory (which at synthesis time is likely a dummy all-0 placeholder).

The distinction between abstract bitvector and an unsigned integer is provided mainly to have a way of forcing a value to be printed in decimal (eg. a delay counter value), or to not be printed in decimal (eg. a pointer or bitmask).  It is not based on any deep philosophy.

The convention used to represent wire indexing (`lsb` and `from_msb`) differs from the one used by yosys for the from_msb case (yosys stores the smaller index of lsb and msb; we always store the lsb). However, it simplifies calculations (the width of the wire is no longer part of the conversion formula, and the difference between the LSB-first and MSB-first cases consists solely of switching an addition into a subtraction).

The syntax for the "containing scope" attribute of metadata annotations is changed from `in=` to `parent=` (for scopes) or `scope=` (for things that aren't scopes) to avoid confusion with the new `input` flag.


# Prior art
[prior-art]: #prior-art

We essentially want DWARF for circuits.

The core scope system described here (`module`/`block`/`array`, named and indexed scopes) is loosely based on Verilog.  However, the `interface` scopes are not based on SystemVerilog interfaces, and instead loosely model how Amaranth interfaces would look if represented as first-class concepts.

The `keep` name comes from the (very vaguely defined) Verilog attribute of the same name.

The idea of the "type system" being essentially only a matter of presentation is roughly similar to Amaranth in concept and in supported data types.


# Unresolved questions
[unresolved-questions]: #unresolved-questions

How should the metadata be stored in memory?  It is clear we want to be able to look up objects by their names, and find all `name` / `dff` / ... cells referencing a given named net or named register.  This will require maintaining an index.

What should be the exact semantics of named cells, if any?  Is it allowed and/or useful to duplicate a named cell?

What should be the exact syntax for the new metadata?  In particular, storage elements will require referencing (a potentially very large amount of) permuted named register bits.

Should we have a way to specify `keep` attributes that only apply to some part of the flow?  (example usecase: keep wires represented as named signals until the constraint file is parsed, convert them to named nets afterwards).

This RFC subsumes the current `input` and `output` cells by the unified named object model.  However, I/O values (used for bidirectional toplevel ports) are left untouched, despite also being identified by name.  Is there a good way to include them in the model?  Can they be made less special in some way?

The `signal` cell provides an optimization barrier, which is a primitive that currently cannot be obtained otherwise.  Likewise, a named net with `keep` can prevent an arbitrary logic cone from being deleted.  Should we have cells that provide these semantics without requiring a name?


# Future possibilities
[future-possibilities]: #future-possibilities

Having proposed an abstract simulator interface, the obvious next step would be to implement an actual simulator.

We should have a proper interface for formal verification that builds on debuginfo to provide access to the values.

Currently, keeping a named net available for readout requires actually materializing it in the final design.  However, this is not strictly necessary: all that is needed is having a way to compute it from the available design state.  We could allow "ghost cells" in the netlist, which are combinational cells used only by `name` cells to compute named net value from other values in the design, when the actual circuitry of the net was optimized out, but the underlying storage it is computed from was not.  Such cells would be ignored by technology mapping, P&R, and emitting the bitstream, effectively being a kind of metadata.

While the type system described here is fairly capable, it is far from enough to describe all sorts of crimes commonly committed when packing abstract data types into a vector of bits.  One could imagine all sorts of extension to the system, or perhaps replacing it with actual formatting-based machinery (based on ghost cells?) to allow fancy custom types.

While a `keep` attribute is reasonably well-defined for simulation and similar use-cases, it is not necessarily enough for manipulating bitstreams or inspecting a target via JTAG: the storage elements in an FPGA are not all created equal, and some may not be readable and/or writable from the outside.  We may want more specific `keep` attributes that specifically request the value is placed in an inspectable register, or that memory initialization data can actually be controlled via the bitstream (which might not be the case eg. for memories lowered to FFs on targets where FF initialization value is not directly settable).  Likewise, we may want a way to request that a `signal` be lowerd to something that can be controlled via in-circuit debugging.

At some point, prjunnamed will need an elaboration driver that gathers partial netlists from various HDL frontends, and links them together to the final design.  Such linking will need to be name-based, and have a similar naming and scope model to this RFC.

Named nets, signals, and registers could be used for automatic scan chain insertion, allowing target-independent ICD access.
