# Initial notes, scope

The IR described here is intended to be used for most of the flow, from right after elaboration to right before bitstream generation.

It is expected that the IR will be produced in one of the following ways:

- a standalone language frontend emitting the IR directly (eg. the amaranth backend)
- a converter from another IR in common use (eg. FIRRTL, RTLIL) for compatibility with external language front-ends
- the prjunnamed cross-language elaboration driver
  - a piece of software which starts from a top module in arbitrary supported language and, whenever an external module instantiation is encountered, relays the request to another language frontend in turn
  - the language frontends may be built in, or connected via a RPC protocol
  - as a special case, one supported input language is simply "pre-elaborated piece of IR"

The language frontends will presumably wish to use their own internal IRs.   The details of the such IRs, the elaboration process, the cross-language elaboration driver, and the RPC protocol are out of scope of this document.

Once the initial IR is produced by any of the above means and the synthesis flow starts, no further elaboration will be done (unless the user manually re-feeds synthesis output IR as an input to a future elaboration run).  One possible exception to this may be the elaboration of blackbox/whitebox cell modules, to be done in a target-specific way (which will probably not involve a "normal" HDL as an input anyway).

The allowed subset of this IR will change depending on the current flow stage, roughly:

- at the beginning, the netlist contains mostly target-independent coarse cells without any restrictions
- moving through the coarse stages, some vendor-specific cells (DSPs, BRAMs) will be mapped; target-independent memories will be forbidden at a certain point
- after bitblasting, only target-dependent cells and fine target-independent cells are allowed
- after fine techmapping, only target-dependent cells are allowed
- after P&R, only target-dependent cells with P&R annotations are allowed

The details of this will be described later.

Depending on flow mode, the above list of flow stages will be stopped at one of a set of predefined points, and output emitted:

- in the degenerate case, one may stop the flow right at the beginning (or after only a bunch of simple optimizations have run) — this is used for the case of simply converting the design to Verilog for an external toolchain, or for CXXRTL
- stop after bitblasting, to feed the design to an external logic optimizer (eg. abc for benchmarking purposes)
- stop after fine techmapping, to use unnamed synthesis with external P&R, or to perform OOC synthesis for further linking
- stop after P&R, to use the full integrated flow

We may also want an option to *start* the flow somewhere else than the beginning (with appropriate validation), but that may be considered a debug-only option.  Synthesis passes should be designed to be able to harmlessly pass through a later-stage netlist, so that pre-synthesized (and maybe even pre-P&R-ed) hierarchy chunks can be linked into freshly elaborated code.

TODO: list a bunch of example flow use cases here and how we would handle these; particularly interesting ones are:

- anything involving OOC synthesis
- integration with external tools (other than straightforward cases of "emit IR from frontend" and "feed Verilog into external synthesis/P&R")
- partial reconfiguration (for this, we'll need to invent some representation of "this PR slot covers this area on the FPGA and has the following input/outputs which are mapped to the following routing points")
- ECO-like uses
- scripting cases where the user wants to modify the netlist in a custom (but automated) way between some synthesis steps

## IR formats and versions

This IR actually manifests as several closely related but ultimately distinct constructs:

- the abstract model of the IR
- the in-memory data structures that Rust code operates on
- the scripting interface to manipulate those, as exposed to eg. Python or TCL
- the at-rest binary storage format
- the interchange text format

The text format will need compatibility guarantees, as it will be used for interchange purposes (in particular, it will be emitted by external frontends).  The IR versions will thus be assigned semver-compliant version numbers, which will be contained in the text and binary formats.  Starting with the first stable release, we will provide the following guarantees:

- a new minor version will only add new functionality (cell types, annotations, etc.)
- for the text format, a design can be trivially upgraded to a new minor version by simply bumping the number in the header
- the text format reader will transparently accept any earlier version within the current major
- a new major version can change the IR in an arbitrary backwards-incompatible way (except retaining just enough of the header format that the version itself can be unambiguously parsed)
- the text format reader may accept earlier major versions on a best-effort basis (it may reject older versions entirely, or reject just some specific constructs of the older version); this needs to be documented and commited to in the usual semver way
- bumping the text format major version requires bumping the major version of prjunnamed itself; likewise for the minor version
  - though we may also want to bump the prjunnamed version for other reasons, so they'll not be necessarily in sync

Note: this gives us the power to break backwards IR compatibility when it's really needed, though that should be used sparingly.

At this point, the binary storage format is not considered to be an interchange format, nor does it come with compatibility guarantees (in fact, it'll probably be just a serde dump of internaal structures).  This may change in the future.

No provision is currently made for version downgrading, or emitting an IR version other than current from prjunnamed.

The text format in particular should be designed for readability and manual edittability, as it will be used for prjunnamed testsuite and problem diagnosis.

## On fields, annotations, and attributes

IR entities have three kinds of data associated with them:

1. Fields
   - data that is stored for every instance of a given entity type
   - may be required or optional
   - used for data expected to be used on all or most instances, or for very-often-accesed data
   - corresponds directly to a struct field at the Rust level
   - corresponds to dedicated syntax in the containing entity in the text format
2. Annotations
   - optional data, but still represented by a first-class IR structure
   - in general, does not take memory if not present
   - in Rust, all annotations on a given entity are stored in one set field, containing a bunch of `enum Annotation` objects, which is an enum containing every possible annotation type
   - in text format, annotations are listed after an entity, are introduced by the `!` sigil, and have annotation-specific syntax
3. Attributes
   - optional data that is not natively understood by prjunnamed
   - is a key-value pair, where the key is a string and the value can be of one of several supported types (same variant type as parameters)
   - corresponds directly to Verilog and VHDL attributes that are not otherwise recognized
   - actually stored as an annotation subtype
   - is the key (and value) case-sensitive? do keys have to be unique? we do not know or care
   - emitted by frontend for unrecognized source attributes
   - when emitting Verilog (or similar format) at the output, serialized back to attributes
   - ... but we may have an early pass that converts common attributes to native annotations, so that the logic doesn't have to exist in every frontend

TODO: do we have target-specific annotations?  How aggressively do we want to deduplicate similar annotation types between targets (ie. is it `xilinx_iob_drive_strength` or general `iob_drive_strength`)?  This may cause an unwanted proliferation of first-class entities in the IR that eg. all text format parsers would need to understand, and it would also require us to bump the minor IR version way more often that would be otherwise necessary.

# Design

The design is:

- a set of modules
- an optional top module designation (designs without a top module may be useful as cell libraries or something)
- a set of design annotations

Design annotations:

- target selection
- assorted target-specific global attributes (eg. Spartan 6 VCCAUX setting)

The design, as the top-level IR entity, also contains the interning pools for strings and possibly other values used throughout the structure.  This is considered an implementation detail and is not reflected in eg. the text format.


## Target selection

It may seem that this could be passed out-of-band, like other synthesis-affecting options.  However, if we want to keep using the same IR for the whole flow, the netlist becomes heavily enough bound to a particular target in the late stages that we need this information available on hand.

We may have or need various kinds of information about the target:

- no information at all ("just run sim / generate Verilog from this and don't ask questions")
- the target toolchain ("generate the dialect of Verilog that Quartus will accept as input")
- the target FPGA family ("this is for Spartan 6, so use Spartan 6 cell library for checks and for tech mapping")
- the target FPGA device (as above, plus "grab BRAM counts for xc6slx9 to guide memory inference thresholds"; also obviously necessary for P&R)
- the target FPGA package
- the target FPGA speed grade

Some of this information is orthogonal (various combinations of toolchain and FPGA family are possible).  It's kind of analogous to a target triple, but I'd rather avoid this mess and just have 5 distinct annotations on the design, as above.

Fun note: the "toolchain" information can actually affect optimization / techmap passes — eg. Vivado on Series 7 actually accepts a few more LUT RAM cell kinds than ISE.

# Data types

There is only one data type used for actual synthesizable logic, and it is a three-value bit vector of statically known size.  The three bit values are `0`, `1`, `x`.  The use cases served by the value `z` in Verilog are instead served by the bus and bus driver objects.

TODO: there is a case to be made for first-class enum support, and I once believed that to be a good idea.  However, this complicates the model a lot and could be equally well supported by annotations.

The question of data types allowed for parameters and attributes is much more complex.  My proposal is:

- bit vector, as above
- integer
- string
- float
- language-specific data: a pair of (language id, serialized bytes)
  - opaque to prjunnamed proper, can only be interpretted by an actor with knowledge of the given language

As far as I know, this should be enough for vendor cell libraries, which is the main concern.  However, if we wanted to faithfully preserve elaboration-time values of parameters, we'd theoretically need to replicate the entire SystemVerilog and VHDL type systems, which is insane.  We may want to just serialize those into a string when they're of a "weird" type.

## Data interpretation

While synthesis doesn't care about bitvec value interpretation, we still want to remember it for informative purposes.  Whenever "data interpretation annotation" is mentioned, we store one of the following:

- (no type information)
- signed or unsigned integer
- enum, list of (value, name) pairs
- record, list of (start bit, len, field name, field type) tuples
- custom type (identified by user-defined name)

Such data should probably be stored deduplicated at top design level, and just referenced at the point of use.

TODO: this is a very rough and incomplete sketch of the interpretation type system (for one, needs tagged union support); a proper type system here is still TBD

# Modules

First of all, modules come in three kinds:

- user module
  - has contents
  - should be synthesized
  - interface can be mutated as required (unused ports removed, inouts converted to inputs or outputs, flattened outright, etc.)
- blackbox module
  - has no proper contents
    - but can still have various cells inside — blackbox port annotations and any computations needed for those
  - interface is immutable
  - can still have annotations about clock domains, combinatorial paths, etc.
  - can be a user-defined blackbox (for partial synthesis flows, dynamic reconfiguration flows, ...)
  - can be a vendor cell pulled from our big library of all vendor cells
- whitebox module
  - has contents
  - interface and contents should be considered immutable
  - should not be emitted when writing output for downstream synthesis tool
  - is a vendor cell pulled from our big library
  - contents can be used for simulation or synthesis of other modules

Note that blackbox and whitebox modules created for vendor cells may *not* have a one-to-one relationship — eg. the width of signals in Intel `altsyncram` interface varies depending on parameter values, so we'll need to instantiate a separate blackbox module for every used width combination.

[TODO: this will be a pain if we get a pass that emits one of these cursed cells — it'd have to generate blackbox/whitebox modules on the fly. Consider alternate solutions?]

A module consists of:

- module kind (see above)
- keep flag (if set, an instance of this module should not be removed just because its outputs are unused)
- keep hierarchy flag (if set, an instance of this module should not be flattened)
- a set of module annotations
- a list of module parameters
- a list of module input ports (actually indices into cell list)
- a list of module output ports (likewise)
- a list of module bus ports (likewise)
- a list of module cells

Module annotations:

- original HDL name
- original HDL library name (in the VHDL / Verilog sense)
- parameter values used to elaborate this module
- assorted key-value attributes

It is expected that module names will not be unique — after all, elaboration will duplicate a module for all parameter value sets.

## Parameters

There are two kinds of parameters in the netlist that need to be distinguished: baked-in parameters and proper (non-baked-in) parameters.

A baked-in parameter is one that has been used in the process of elaboration to create the contents of the module in the first place.  Its value is immutable for a given module, as that would require rerunning elaboration and result in a different module.  The value is stored as a module attribute, but should not affect synthesis in any way.  For blackbox and whitebox modules, it'll be pulled out of the attributes when emitting module instantiation in eg. Verilog writer.

A proper parameter is one that does not structurally affect the module.  Its value is passed in at the point of instantiation of the module.  For non-blackbox modules, the parameter value is visible as a cell and can be used eg. to drive muxes in a whitebox DSP model.

Proper parameters are a first-class object belonging to a module.  They are bound by position in the parameter list.  A parameter consists of:

- parameter type
  - string
  - bitvec, const width
  - bitvec, any width
  - integer
  - float
- parameter name
- default parameter value
- parameter value restrictions (optional), depending on type:
  - string: list of valid values
  - bitvec: list of valid value ranges
  - integer: list of valid value ranges
  - float: list of valid value ranges (start, end, start inclusive flag, end inclusive flag)
- data interpretation annotations, for bitvec-valued parameters
- assorted key-value attributes

TODO: it is debatable how much flexibility we want to allow regarding parameters.  Here are a few proposals, from lowest to highest effort:

1. Only blackbox modules are allowed to have parameters at all.  Whitebox cells need to be elaborated when instantiated.
2. Whitebox cells can also have parameters, and they can be used as a normal value to drive cells — means that whitebox models can be pulled straight from the library without elaboration in many cases.  String parameters can be used to drive cells once compared for equality with a const.
3. As above, and also parameters can be used to set FF or memory initial values (probably with swizzle capabilities) — even more simple whiteboxes.
4. As above, plus parameters can be used to set downstream parameters and be threaded through the hierarchy, plus some simple constant operations can be performed on them — this is a particularly fun one, since when supported throughout the entire flow, this can give us some nice features:
   - modules can be kept deduplicated through a big part of the flow when only superficial details differ (eg. CPU core instantiations differing only by the "core ID" parameter)
   - "firmware image" can be a top-level parameter wired to a memory intialization somewhere, remains a parameter throughout memory inference (which adds swizzles as needed) and P&R, can be eventually bound as late as right before bitgen
   - likewise, parameter const drivers can be inserted, to be filled right before bitgen (lowers to a LUT whose init is to be filled later, or to something fancier)


# Cells

The list of cells is the main meat of the module.  This is also simultanously the list of values.  For simple single-output cells, the cell and its output are considered synonymous.  Multi-output cells have separate entries in the list for each of their outputs.  Things like swizzles and constants also use up a slot in this list.  This way, all cell inputs are simply represented as a single index within this list.  Likewise, all uses of a value (including swizzles) can be represented as a (cell index, input index) tuple.

The cell kinds are:

- ports
  - input port
  - output port
  - bus port
- swizzle (and its special cases: const driver, zero extension, sign extension)
- combinatorial cells
  - buf
  - not
  - binary and, or, xor
  - unary (reduction) xor
  - simple mux
  - wide mux
  - switch/case
  - comparisons: eq, ne, slt, sge, ult, uge
  - binary add, sub
  - add/sub with carry
  - binary mul, udiv, umod, sdiv, smod, pow
  - shift
  - rotate
  - bit scan
  - popcnt
  - demux
- register (latch, FF, or synchronous memory read port)
- memory
- async memory read port
- bus-related cells
  - bus
  - bus driver
  - bus joiner
- module instance
- unresolved instance
- instance output
- clock gate
- annotation-like cells
  - wire name
  - blackbox clock
  - blackbox synchronous input
  - blackbox synchronous output
  - blackbox combinatorial path
  - blackbox asynchronous reset

A cell has:

- cell kind, as above
- data specific to cell kind
- attributes (custom KV pairs)

A cell may or may not be a value.  If it is a value, it has an associated width, which can be determined just from looking at the cell (usually either implied by cell kind or just stored as part of kind-specific data).

## Ports

Modules have three kinds of ports:

- input ports
- output ports
- (tristate) bus ports

All three kinds of ports are actually represented as special cell types and, for instantiation purposes, are bound by port index (with separate index namespaces for input, output, and bus port kinds).  They have the following data associated with them:

- port index (for easy bidirectional mapping with the corresponding port list)
- keep flag — if set, the port is not to be optimized away
- (optional) name
- (optional) data interpretation

Input ports are simply values made available from outside.  In addition to the above, they have the following data:

- data type
- default value when unconnected (x if not specified otherwise)
- "complete description" flag — if set, it means that the input usage is completely described by the blackbox usage annotations; if not set, it must be assumed that the input is used in other, unknown ways

Output ports make values available to the outside.  In addition to general port and cell data, they have:

- value reference (not in blackbox modules)
- "complete description" flag — if set, it means that the output sourcing is completely described by the blackbox usage annotations; if not set, it must be assumed that the output also changes in other, unknown ways

Output ports are not usable as values, but can be referenced by the special blackbox annotation cells (and by swizzle cells used by those).

Bus ports describe tristate, bidirectional connections to the outside. They behave like the "bus" cell described later, with the additional feature that they expose the bus to the instantiating module.  In addition to general port and cell data, they have:

- data type
- pull direction (see bus description)

Bus ports, just like bus cells, are generally to be avoided except for the important case of top-level bus ports (which describe both inout ports and tristate-capable output-only ports).

TODO: we may want to standardise a set of annotations/attributes on top-level ports for describing pin location, I/O standards, etc.

TODO: actually we may want to also have an "IO pad" cell type for use in later parts of the flow, instead of dealing with the generic "bus" construct.


## Swizzles

A swizzle is a (rather fake) cell kind that simply rearranges the bits in values.  There are three subkinds of swizzles:

- a plain value swizzle: each chunk can either refer to any value, or be a constant; the swizzle then counts as a value
- a bus swizzle: each chunk must refer to a bus or a bus port; the swizzle can only be used where a bus can be used
- an output port swizzle: each chunk must refer to an output port; the swizzle can only be used where an output port can be used

A swizzle consists of:

- swizzle subkind, as above
- a list of chunks to be concatenated, where each chunk is either of:
  - a const value
  - a tuple of:
    - value (or bus/output) reference
    - starting bit in source to slice from
    - number of bits to slice from the source
    - width to sign-extend the sliced bits to (only available for value swizzle)

The following special cases of swizzles are interesting (ie. we want matchers for them, special-cased user-facing representation, and maybe even special-cased in-memory representation):

- const driver (has a single chunk, which is a const)
- zero extension (takes a whole value, concatenates it with zeros)
- sign extension (takes a whole value and just sign extends it)
- truncation (takes a single slice, starting from bit 0)

NOTE: Representing swizzles (and in particular consts) as plain cells is one of the parts I'm the least convinced about.  Some alternatives:

1. do the yosys thing of essentially inlining a swizzle into every single cell input
2. treat swizzles (and consts?) as a separate space from cells, as discussed on the channel a few months ago; however, I see no advantages to doing it that way compared to a unified namespace
3. fine-optimized version: keep as-is, but specialize a few cell kinds important to fine netlists to have an inline swizzle version (probably supporting only single-bit references, not arbitrary chunks)

IMO the goals to optimize for are as follows:

- ease of use in coarse passes
- ease of use in fine passes
- memory usage in fine netlsts

For coarse netlists, we assentially have a small set of commonly used queries to answer:

- is this input simply connected to the output of that cell
  - this, but truncated
  - this, but zero-extended
  - this, but sign-extended
- is this input const
- which bits of this input are const
- which bits of this input are duplicates
- which bits of this output are actually used anywhere
- expand this input (whether swizzle or direct) to a list of (actual cell, bit index) tuples

For fine netlists... first of all, the cell kinds actually useful in fine netlists are:

- binary not/and/or/xor of width one
- mux of width one
- register of width one
- ASIC standard cells, or tiny FPGA hard blocks like carry chain
- wide-input (reduction) and/or/xor (arguable, in principle redundant with binary and/or/xor, but can be more convenient to look at the wide version; also a native CPLD primtiive)
- potentially wide mux for some funny FPGA architectures?
- a LUT, aka a wide mux with constant data input
- misc hard blocks that are not the result of plain logic mapping

The first four only deal with single-bit values anyway, so no swizzles will ever be applicable between them.  Misc hard blocks are rare and not of interest to most fine passes anyway.  Thus we need to optimize for LUTs and maybe wide gates.  I think alternative #3 above can take care of them nicely by basically having special-cased cell kinds for those for use in fine netlists.


## Combinatorial cells

These just compute a value from other values, mostly in obvious ways.

### Pass

Aka a buffer.  Usefulness is debatable, but can possibly be useful in late-stage netlists as a place to attach routing information.

- width
- source value

### Not

- width
- source value

TODO: we may want to have built-in inversion capability in many cells, to avoid having to explicitly look for an inverter on input every time, and to conceptually closely associate it with a cell.  The obvious cases which I already included are:

- register clock, reset, and other control inputs
- likewise for memory ports
- tristate driver control input
- and/or/xor gates inputs/outputs

Less obvious case:

- select blackbox cell instance ports (Xilinx in particular has a *lot* of freely-invertible ports, including things such as DSP mode inputs)

### Bitop (binary)

- subkind (and, or, xor, nand, nor, xnor, andn, orn)
- width
- source values ×2
- glitchless flag (if set, this operation should be considered used in async context, and must be carefully optimized and techmapped so as to not introduce glitches)

TODO: consider whether we want the basic three, or the full set.

### Unary (reduction) xor

- subkind (xor, xnor)
- source value

The output width is always 1.

Note that there are no reduction and/or cells: they are redundant with eq/ne cells.

TODO: consider whether we want a xnor.

TODO: as per the discussion in swizzle, we may want a "fine" version that has a vector of single-bit values instead of a single wide value.

### Simple mux

- width
- select value (single bit)
- source values ×2
- glitchless flag (like for bitops)

Does `s ? b : a`.

### Wide mux

- (output) width (`w`)
- select value (`s` bits wide)
- source data value (must be `w << s` bits wide)
- glitchless flag (like for bitops)

Splits the data value into an array of `w`-sized chunks, indexes into this array with the select value.

A LUT is a special case of this cell where the data is const (and usually the width is one, but then multi-out LUTs do exist).

TODO: as per the discussion in swizzle, we may want a dedicated cell for the "LUT" special case where select value is a vector of single-bit values, and the LUT const is embedded in the cell.

### Switch/case

- width
- select value
- list of cases
  - match value (a constant made of 0/1/- bits)
  - data value
- a default value
- parallel case flag (if set, outputs x when more than one case would match; if unset, it's a normal priority case)

There is no "full case" flag — just wire the default value to x if you want one.

Also usable as a parallel/priority mux by using one-hot match values — or do we want to have a dedicated cell for this?

### Equality comparison

- subkind (eq, ne)
- source value ×2
- glitchless flag (like for bitops)

The output width is always 1.

A special case of this (comparison with a constant) is equivalent to a reduction and/or gate, with arbitrary negation on its inputs.  The glitchless flag is provided for use with that.

TODO: as per the discussion in swizzle, we may want a special-case "fine" version that has a vector of single-bit values as first source, and an inline const as second source.

### Comparison

- subkind (ult, uge, slt, sge)
- source value ×2

The output width is always 1.

No ugt/ule/... variants — just swap the sources.

The provided subkinds are lt+ge so that negation is trivial.


### Add/sub

- width
- source value left
- source value right
- source value inversion (single bit)
- source value carry (single bit)

Calculates `a + (inv ? ~b : b) + c`. Simple `add` is a special case with the last two inputs tied to 0. Simple `sub` is a special case with the last two inputs tied to 1. `neg` is a special case of `sub` with the first input tied to 0.

TODO: do we want to expose carry out?  Is this cell simply too annoying to be used in general netlists?

### Multiplication

- width
- source value ×2

Source values are the same width as the result.  If an extending multiplication is needed, just add zext/sext on the sources.

### Arithmetic that you don't want in synthesizable netlists

- subkind (udiv, umod, sdiv, udiv, pow, ...)
- source value ×2

sdiv/smod need like 3 subvariants with various roundings. pow needs 4 different combinations of signedness or something.

Has the unfortunate property of outputting x on non-x input sometimes.  Use an explicit mux to avoid this.


### Shift

These one is a ride.  Basically, handling shifts (and, in particular, bitfield extractions) well requires an intermediate cell with lots of weird options.

- result width
- subkind
  - unsigned (fill on bottom and top with 0)
  - signed (fill on bottom with 0, fill on top with copy of sign bit)
  - X-filling (fill on bottom and top with x)
- source value (can be different width from result)
- shift amount value (can be any width)
- is shift amount signed
- shift amount scale factor (signed integer)
- shift amount bias factor (signed integer)

The cell computes `src >> (shamt * scale + bias)`, where negative shifts are interpretted as shifting left.  Arbitrarily large values are accepted for the shift amount, there is no wrapping.

### Rotate

Somewhat less insane than shift.

- result width
- source value (must be same width as the result this time)
- shift amount value (can be any width)
- is shift amount signed
- shift amount scale factor (signed integer)
- shift amount bias factor (signed integer)

Likewise, arbitrarily shift amount values are accepted, and they will effectively wrap modulo data width.  This will be inefficient for non-POT width and wide shift amount.  Oh well.

### Bit scans

Things like "count leading zeros".  Do we want them supported as first-class primitives?

TODO: fill me

### Popcnt

Likewise, we may want this one? idk.

TODO: fill me

### Demux

- *source* width
- *select* width
- source value
- select value

The output width is not specified directly, but is computed as `source_width << select_width`.  This cell basically does `source << (source_width * select)`, ie. fills one `source_width`-sized slot of the output with the source values, and all the others with 0s.

Probably not the most exciting cell, but it got some use in memory inference (write enable signal generation) and has non-trivial lowering.


## Register

To avoid combinatorial explosion, we have one (1) register cell kind:

- width
- init value
- list of async triggers, in decreasing priority order
  - condition value (single bit)
  - condition polarity
  - data value
- sync trigger, if any
  - clock value (single bit)
  - clock edge: posedge, negedge, or dual-edge
  - list of rules, in decreasing priority order
    - condition value (single bit)
    - condition polarity
    - data value, or a "no-op" token
  - default rule
    - data value

At all times, the async trigger list is traversed from top to bottom and the first rule with active condition sets the register value.  If none of these triggers are active, the register keeps its value.

When the sync trigger happens and none of the async triggers are active, the list of sync rules is likewise traversed from top to bottom and the first active one sets the register value.

This cell is horrifyingly generic.  There is a handful of special cases of this cell that are considered sane, and will actually be used by general optimizations (such as retiming):

1. Async-reset FF
   - async trigger on reset signal, with const data value
   - sync trigger on clock signal
   - sync rule on `!clock_en` with no-op token
   - default data value of `D` input
2. Sync-reset FF, CE over reset
   - sync trigger on clock signal
   - sync rule on `!clock_en` with no-op token
   - sync rule on reset signal, with const data value
   - default data value of `D` input
3. Sync-reset FF, reset over CE
   - sync trigger on clock signal
   - sync rule on reset signal, with const data value
   - sync rule on `!clock_en` with no-op token
   - default data value of `D` input
4. Any of the above, but without the clock enable sync rule
5. Any of the above, but without the reset rule / trigger
6. Arguably, latch
   - (optional) async trigger on reset signal, with const data value
   - async trigger on gate signal, with data value of `D` input

The special cases warrant matchers, and possibly optimized representation.

The target may want to legalize all registers to a uniform shape, or one of a handful of shapes, that fit its architecture.  For some targets, this shape may not be considered "sane" as per above (eg. the generic case of Spartan 3 FF has both set and reset signals).  Oh well.

TODO: this cell is clearly insane.  What are reasonable alternatives?

Note: funnily enough, this cell is still not generic enough for native Xilinx FPGA latches, which have a gate and a gate enable signal.  It would be if we had no-op tokens for async triggers though.  Eh.

Note: we may avoid having a separate encoding for a "no-op token" by simply connecting the data value to the register's own output.

## Memory

A memory is described together with all its write ports as a single cell.  However, read ports are stored as separate cells.

A memory has:

- element width
- address width
- valid address range
- initialization data
- list of sync write ports
  - clock value and polarity
  - write enable value (and polarity?) — single bit
  - mask value
  - width factor (how many elements does this port access at once — has to be POT)
  - address value
  - data value
  - a list of sync write port indices that this port has priority over
- list of async write ports (if anyone ever cares to implement that)
  - write enable / strobe value and polarity - single bit
  - mask value
  - width factor (how many elements does this port access at once — has to be POT)
  - address value (log2(width factor) shorter than the address width mentioned above)
  - data value (of width (width factor) * (element width))
  - a list of async write port indices that this port has priority over
- keep flag
- (optional) name
- (optional) data interpretation to be applied for each element
- (optional) bit indexing information for each element
  - start offset
  - downto or upto


When multiple write ports write the same bit at the same time, the result is x unless there is a defined priority relation between them. Ports should be stored topologically sorted wrt priority.

Width factors have the following restrictions:

- cannot be larger than `2 ** address_width`
- valid address range start and end must be a multiple of every width factor used

Note: width factor is actually easier to store as log2.

Note: some of the feature combinations (particularly transparency / priority between ports that are not obviously in the same clock domain) are not actually expressible in synthesizable Verilog.  At the same time, some of these feature combinations can actually be correctly synthesized on some targets.  This seems to be an unavoidable fact of life.

TODO: we may want something more complex than "valid address range" — consider the following funny memory in SystemVerilog:

    logic [7:0] array2d[0:8][0:8];

Clearly, the memory has 81 valid addresses, but they don't form a contiguous range unless we want to have multiplication in the address generation path.  This may not come up in practice often enough to worry about it, though.

### Memory read port — async

- element width
- memory cell reference
- width factor
- address value

The output width is `element_width * width_factor`.


### Memory read port — sync

An unholy union of the register cell with the async memory read port, with an extra addition on top.

- element width
- memory cell reference
- width factor
- address value
- init value
- list of async triggers (see register cell)
- sync trigger (not optional this time)
  - clock value and edge (see register cell)
  - list of rules (see register cell)
  - *no* default rule
- a list of sync write ports this port has a well-defined relation with
  - port index
  - relation kind (read old, read new)

This cell behaves like the register cell, except that when the default sync rule would normally be reached in a register, a memory read is performed instead.  If a sync write is happening at the same time to the same bits, x is read unless otherwise specified.  If multiple sync writes are happening and the relations specified seem to be contradictory or underdefined, gods help you.

## Buses

### Bus cell

The bus cell represents a tristate bus, roughly equivalent to Verilog `wire`.  It has:

- width
- subkind (no pull, pulldown, pullup)

The subkind defines the value that the bus takes when there are no active drivers.  If no pull is specified, it takes the value of x.

The bus port cell is identical, except it's also accessible when instantiating the module.

The bus is valid to use as a value (it will simply read as the current state of the bus), and additionally can be referenced in a handful of special cells that accept only bus (or bus swizzle) references.

Note that buses can be connected together, possibly swizzled, by bus joiners and module instantiations.  It may very well be the case that bus bit 0 is, in fact, the same as bus bit 1.  Generally very little can be done with a bus at all until the relevant part of hierarchy is flattened enough and all bus joiners are resolved.

TODO: also wire-and and wire-or buses? eh.

### Bus driver

- bus reference (or bus swizzle reference)
- data value (same width as the bus)
- enable value (single bit)
- enable polarity

Drives a value onto a bus, or possibly a swizzle of busses.  If multiple drivers are enabled, x and possibly a burnt device happens.

### Bus joiner

- a list of bus (or bus swizzle) references

Joins (connects) the buses together in a bidirectional aliasing connection.  Analogous to SystemVerilog `alias` construct.

This may join together buses of differing pull subkinds.  If a no-pull bus is joined to a bus with with pull, the pull kind wins.  If a pulldown bus is joined with a pullup bus... an error I guess? idk.

## Clock gate

A funny cell type roughly equivalent to BUFGCE.  It may be useful for some transformations.

- clock value and polarity
- enable value (and polarity?)

Always has output (and input) widths of one.

Gates the input clock with a given enable.  Assuming positive clock polarity: output starts at 0. At rising edge of the clock, the enable signal is sampled.  The output is set to 1 if enable is active, remains at 0 otherwise.  At falling edge of the clock, output is set to 0.

Alternatively: a register clocked with the output of this cell is equivalent to the same register clocked with the original clock input, with an added sync rule to do nothing if the enable is inactive.

The behavior of this cell is equivalent to some simple latch circuit, the details of which escape me.  However, it has important additional semantics:

- the resulting output is considered to be a reasonable clock signal
- the output is delay-matched with buffers on the original (input) clock network, so registers see matching active edges on both arrive synchronously
- likewise, in simulation, the output transitions in the same delta cycle as the input, not in the next one as would be expected of a latch

Note: this was in my old drafts, but I'm not sure how useful it is.

## Instances

### Module instance

An instantiation of a module defined in the design (whether a user module or a blackbox). Is definitely not a value.

- module reference (index in the global module list)
- keep flag (ORed with the corresponding module flag)
- keep hierarchy flag (likewise)
- (optional) name
- list of param bindings (inline constants of proper types)
- list of input port bindings (value references)

The parameter and input port bindings must match the number of types of parameters and input ports defined on the module.

Output and bus ports are not bound here — instead, they are refered to by "instance output" and "instance bus" cells.

### Instance output

This cell kind simply brings out an instance output port value to the instantiating module.

- width
- instance cell reference
- output port index

### Instance bus

Likewise, but for bus ports.  This cell can be refered to wherever a bus reference is valid.

- width
- instance cell reference
- bus port index

### Unresolved instance

An instantiation of a module known only by name.  Since no prototype is available, all parameters and ports are bound by name as well.

- module name (a string)
- (optional) library name (a string)
- keep flag
- keep hierarchy flag
- (optional) instance name
- list of parameter bindings
  - parameter name
  - parameter value (inline constant)
- list of input port bindings
  - port name
  - port value (value reference)
- list of output ports
  - port name
  - port width
- list of bus ports
  - port name
  - port width

The same instance output/bus cells are used to refer to an unresolved instance's ports — the port indices simply become indices into the inline output/bus port lists.

Note that all unresolvewd instances must be assumed keep until proven otherwise.

## Annotation-like cells

### Wire

- value reference
- name
- keep flag
- (optional) data interpretation
- mask of bits optimized away
- bit indexing information
  - start offset
  - downto or upto

This cell associates a value with a name and an interpretation.  It can also be used to slap a keep flag on any value-like cell.

If the keep flag is set, the value must be kept available throughout the flow (though the underlying cell computing it may be transformed and replaced as appropriate).  If not set, the wire is preserved on best-effort basis: any bits of the value that are deemed unused can be replaced with x const bits, and the corresponding bit set in the "optimized away" mask.  If all bits of the wire get optimized away, the wire itself is removed.

### Blackbox port annotations

This set of cells is used to describe usage and dependencies of module ports.  The main use case is constraining blackbox module behavior, but it'd be perfectly reasonable to also have these on normal modules (presumably computed by some analysis pass).

The description of any given port by these cells may be complete (as in, describe all potential interactions the port may have) or not.  This is marked by the "description complete" flag on the port.

TODO: these seem like an obvious place to stuff timing information some time in the future; we probably want a layer of indirection here (the cell library names a timing parameter associated with a setup time on given port, the actual time is contained in a speed-grade-dependent timing file that is a giant key-value dict).  I've listed the obvious associated timing parameters where applicable.

TODO: these cells contain a "condition" field that describes a const-computable (aka parameter-dependent) condition whether the annotation is considered applicable to a given cell.  Actually doing this requires properly specifying what we consider const-computable.

Note: it is possible for a port to belong to multiple domains, and thus have multiple (kinds of) annotations (several sequential ones, or both sequential and combinatorial).  Consider the following:

    always @(posedge clka)
        if (rst)
            ...
        else
            ...

    always @(posedge clkb)
        if (rst)
            ...
        else
            ...

The `rst` port is associated with two clocks.  Likewise, for the following:

    input a;
    output o;

    always @(posedge clka)
        qa <= ...;

    always @(posedge clkb)
        qb <= ...;

    assign o = a ^ qa ^ qb;

The output `o` is simultanously associated with clock `clka`, clock `clkb`, and also has a combinatorial path.

It is also possible for an output to not have any annotations, yet have the "description complete" flag.  This corresponds to a const driver output, or an unused input.

As for bus ports, they are cursed anyway and we do not speak of them.


#### Blackbox clock

- value reference (must resolve to an input port or a swizzle thereof, single bit)
- condition (must resolve to a const-computable value depending only on cell parameters)
- (future) min clock period, pulse widths

Marks the port as a clock.


### Blackbox combinatorial path

- input value reference (must resolve to an input port or a swizzle thereof)
- output value reference (must resolve to an output port or a swizzle thereof)
- condition (const-computable)
- (future) path delay

Describes a combinatorial path existing from (all bits of) the input value to (all bits of) the output value

TODO: Verilog specify also has a per-bit version here, how useful is it in practice?


### Blackbox sequential input

- input value reference (must resolve to an input port or a swizzle thereof)
- clock value reference (must resolve to an input port or a swizzle thereof; single-bit)
- clock polarity
- condition (const-computable)
- runtime condition value (aka clock enable)
- (future) setup, hold

### Blackbox sequential output

- output value reference (must resolve to an output port or a swizzle thereof)
- clock value reference (must resolve to an input port or a swizzle thereof; single-bit)
- clock polarity
- condition (const-computable)
- (future) clock-to-out delay

### Blackbox sequential async reset

- reset value reference (must resolve to an input port or a swizzle thereof; single-bit)
- clock value reference (must resolve to an input port or a swizzle thereof; single-bit)
- reset polarity
- clock polarity
- condition (const-computable)
- (future) recovery, removal

Marks a port as an async reset associated with a clock.  Essentially can be used to mark the port as needing a reset synchronizer.

## On vendor-specific cells and mapped cells

I propose minimizing the number of vendor-speficic cells used for fine techmapping.  Whenever possible, instead of introducing a target-specific cell, a target-independent cell type will be used, with a "mapped primitive type" annotation type designating the actual hardware implementation.  The set of values used in this annotation will be target-specific, and having such an annotation will impose target-specific validity rules on the cell.  A pass may only operate on a mapped cell if it understands these rules.

Specifically:

- LUTs should be represented as the target-independent fine cell throughout the flow
- FFs and latches should be represented as the target-independent register cell (unless the vendor register truly is too fancy to be contained by this cursed cell type)
- hard muxes (like `MUXF7`, `MUXCY`) should be represented as the target-independent mux cell
- simple hard gates (`XORCY`, `MULT_AND`, `ORCY`) should be represented as the corresponding bitop cell
- CPLD product terms should be represented as the fine `eq` cell
- more complex ASIC-like gates (AND-OR-NOT etc.) and combinatorial FPGA hard cells (like carry chains) should be represented as a fine LUT cell with a fixed LUT value

For vendor primitives that are impossible or impractical to represent as above, one of two options will be used:

1. The primitive will be represented as an instantiation of a blackbox or whitebox module

   - the module will be marked by a special annotation type as corresponding to a vendor cell
   - the target-specific code may rely on exact parameter and port ordering (ie. port indices may be hardcoded as Rust-level consts)
   - the module should only be created by target-specific code, to ensure the above requirements are met
   - the module can be created in an early "linking" pass, where existing unresolved instances are resolved to use it, and existing frontend-provided blackbox modules (if any) are replaced with the tightly-defined target cell
   - the module can also be created on-demand whenever a target-specific pass needs to emit a cell (eg. DSP inference)

2. The primitive will be represented as a first-class target-specific cell type

   - the cell will have to be supported *everywhere* (including every text format parser) and require an IR version bump, so this option should be reserved for very well-justified circumstances
     - cell needs to be common and actively manipulated (eg. used in fine techmapping)
     - not easily representable as a mapped cell
     - not easily representable as a blackbox/whitebox cell (presumably because of having variable port widths)
   - it is not currently clear that such cells should even exist

All targets should provide a way to convert all mapped (and first-class target-specific, if any) cells to (possibly unresolved) instances, for use with Verilog emitters and similar backends.

# Various notes, validity checks, etc.

## The planes of existence

It may be useful to split the set of cells belonging to the module into three "planes":

- the parameter plane
  - contains parameters and combinatorial cells performing compile-time computations on parameters
  - may even be allowed to perform operations on non-bitvecs (comparing string parameters for equality is something we'd definitely want)
  - does not get synthesized into vendor cells (if kept that long, results in a parametrized netlist instead)
  - can be used for things like passing parameters to submodules, init values, domain annotation conditions, etc.
  - cannot refer to higher planes
- the Prime Material Plane
  - synthesizable cells live here
  - can use outputs of the parameter plane (they will become const drivers), cannot see the annotation plane
- the annotation aka debug plane
  - does not get synthesized
  - actually, logic no longer used by the material plane kind of ascends here
  - contains stuff like wire names, plus whatever combinatorial cells are necessary to compute the values of wires that don't simply directly exist on the material plane
  - probably also useful for stuff like assertions, etc
  - no storage, only combinatorial cells — registers still need to materially exist

This is inspired by CXXRTL debug-only cells, but can be more widely applicable (the same mechanism can be used to compute wire values from eg. FF values captured by JTAG directly from the FPGA).

If we go forward with this, the planes can be implemented with separate cell lists, or by just slapping plane tag on every cell (I prefer the latter).

Technically, a module instantiation could actually be considered to live across all three planes at once.  Ports could be material or annotation.  This could get messy.

## Names

The wire, memory, instance (and possibly some I missed) names in this document are HDL hierarchical names — when flattening a cell, such names will be prefixed with inner instance name in the outer module.  A HDL name is a non-trivial data type, as it's essentially
a list of chunks that can be of two types:

- component name
- component index

I propose flattening this to a string, like yosys already does for `hdlname`.  The yosys encoding unfortunately doesn't support the "component index" part.  This can be worked around by eg. prefixing index components with two spaces instead of one.

Module and library names are non-hierarchical.  Port names are not hierarchical, but they become hierarchical wire names upon flattening.

## Indexes

The above essentially describes the schema of a database, but doesn't mention the database indexes.

The schema doesn't require many indexes.  I propose the following:

- the list of cell/value uses: for each cell, list all (cell, input index) pairs that refer to it
  - this applies to all kinds of inter-cell references, not just value references — a memory cell will have memory read ports listed as its uses
  - requires defining exactly what an "input index" means for each cell kind
  - note that this is less often used than it may be expected — instead of having RAUW, we can simply swap the relevant cell in-place
- a list of module instantiations: for each module, list all (other module, cell index) pairs that instantiate it
  - significantly less useful, but beneficial for eg. the "fix up these bus ports to be proper input or output ports" pass
- a lookup of (module name, param value set) to module index
  - for resolving unresolved instances
  - for checking whether a blackbox/whitebox module for a vendor cell is already present, or needs to be added
- a lookup of port name to (port kind, port index)
  - for resolving unresolved instances
- a lookup of hierarchical name to wire/memory/instance/...
  - not used by synthesis passes, but may be useful for user access?
  - since it's unknown how flattened a hierarchical name may be, will actually need to check for every prefix of the name, and return a hierarchical instance path itself?

## Combinatorial loops

It is very tempting to add "no combinatorial loops" as a netlist validity rule.  It is also a horrible mistake.  This is effectively impossible to keep proper track of:

- the analysis needs to go through hierarchy boundaries, making it quite complex
- analysis is not always possible for blackboxes, which means linking two perfectly valid netlists together can easily produce an invalid natlist
- it's not always clear what constitutes a combinatorial loop — is the D→Q path of a latch combinatorial? If it is, we reject perfectly reasonable netlists (two opposite latches make a flip-flop). If it isn't, optimizing an always-transparent latch can invalidate a netlist

I propose the following solution instead:

- combinatorial loops are considered perfectly valid, although somewhat cursed; all passes must deal with them somehow (possibly by rejecting the netlist, as a last resort when no other solution is applicable)
- a helper pass will be provided that sorts cells in topological order
  - optionally, dummy always-on latches will be inserted as cycle-breakers when necessary
  - optionally, false cycles involving plain combinatorial cells (eg. an and cell traversed twice) will force-split the cell
  - there are several variants of how to deal with false cycles involving module instances here

## Keep flags

Keep flags coming from the source code must be propagated — any module recursively containing anything marked "keep" is effectively "keep" itself.  Since "keep" items must, by definition, never be removed, I propose a simple rule: a module containing anything with a keep flag set must itself have its keep flag set.  This can be ensured by an early pass.

However, note that we cannot rely on it when any unresolved instances are present in the netlist — we don't know if the module it eventually resolves to will have the flag set or not.

## Assorted TODOs

- combinatorial cell for "insert these bits at this (non-const) position into this value"?
- some kind of annotation for "I pinky promise this signal is one-hot"
- assertions
- other FV features
- list validity checks we want to have, both general and context-dependent ones (eg. what is a fine netlist)
- describe what the netlist looks like post-P&R
- source location annotations
- in general, list the annotation types we want to have; make it clear what is an annotation and what is a field
