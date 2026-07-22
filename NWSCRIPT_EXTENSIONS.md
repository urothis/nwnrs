# NWScript extensions in nwnrs

This guide is for script authors who already know the NWScript shipped with
Neverwinter Nights: Enhanced Edition. It covers the language features nwnrs
adds on top of vanilla NSS and shows when each one is useful.

The important compatibility rule is simple:

> Extended NSS must be compiled or packaged with nwnrs.

The nwnrs compiler validates and lowers the extensions before producing
ordinary NCS bytecode. The game does not need to understand enums, `match`,
macros, or compiler attributes at runtime.

## At a glance

| Feature | What it adds |
| --- | --- |
| Strong enums | Named `int`- or `string`-backed values with type checking |
| Type aliases | Another source-level name for an existing type |
| `match` | Exhaustive branching over a strong enum |
| `static_assert` | A compile-time check with an optional error message |
| Bang macros | Balanced `name!(...)`, `name![...]`, and `name!{...}` calls |
| `macro_rules!` | Declarative source-to-source transformations |
| `proc_macro!` | Transformations written in NWScript and run by the compiler |
| `quote!` | Convenient construction of generated NSS token streams |
| Compiler attributes | Metadata such as `#[nwnrs::events(...)]` consumed during packaging |
| Compiler-only types | Token streams, token lists, cursors, and quote bindings for procedural macros |

Most module authors only need the enum, alias, `match`, assertion, and event
sections. Read the declarative macro section when repeated boilerplate becomes
a problem. The procedural macro, quoting, and compiler-only type sections are
the advanced layer for people building reusable source generators.

## Strong enums

Vanilla NWScript usually represents a closed set of choices as unrelated
integer constants. A strong enum gives the set a name and prevents unrelated
values from being mixed accidentally.

### Integer enums

An enum uses `int` storage unless another backing type is written. Values begin
at zero and increment from the previous value.

```nss
enum EncounterState {
    Idle,       // 0
    Starting,   // 1
    Active = 5,
    Finished,   // 6
}
```

Use a variant through its enum name:

```nss
EncounterState state = EncounterState::Starting;
```

Enum values remain strongly typed through variables, function parameters,
returns, structure fields, assignments, equality checks, and `switch`.

```nss
int IsFinished(EncounterState state)
{
    return state == EncounterState::Finished;
}
```

Raw integers do not implicitly become enum values. A compile-time constant may
be converted with one argument when it exactly matches a declared variant:

```nss
EncounterState state = EncounterState(5);
int nStoredValue = int(state);
```

Dynamic values require an explicit fallback. The expression is evaluated once,
and the fallback is used when it does not equal any declared variant:

```nss
EncounterState state = EncounterState(nStoredValue, EncounterState::Idle);
```

The compiler rejects invalid constant conversions and dynamic conversions that
omit the fallback. This prevents undeclared enum values from entering strongly
typed code.

### String enums

Use `: string` for a string-backed enum. Every variant needs an explicit,
compile-time string value.

```nss
enum EventPhase : string {
    Before = "before",
    After = "after",
}

EventPhase phase = EventPhase::Before;
string sPhase = string(phase);
```

Only `int` and `string` are supported as enum backing types.

### Default variants

The first variant is the default value unless one variant has `#[default]`.
This affects uninitialized variables and defaults nested inside structures; it
does not change the variant's numeric or string value.

```nss
enum ConnectionState {
    Connecting,

    #[default]
    Disconnected,

    Connected,
}
```

An enum can have at most one explicit default.

### Compatibility aliases

`#[alias(NAME)]` creates a typed global compatibility constant for a variant.
It is useful when migrating existing constant-based scripts without changing
every call site at once.

```nss
enum LogLevel {
    Trace,

    #[alias(MY_LOG_LEVEL_INFO)]
    Info,
}

LogLevel level = MY_LOG_LEVEL_INFO;
```

The alias retains the enum's strong type. It is not an untyped preprocessor
replacement.

## Type aliases

A type alias gives an existing type another source-level name:

```nss
type CurrentState = EncounterState;

CurrentState state = EncounterState::Active;
```

Aliases are transparent. They add no runtime wrapper, storage, conversion, or
metadata. An alias can refer to a native NWScript type, strong enum, structure,
or another valid alias, but aliases cannot form a cycle. Declaration modifiers
such as `const` belong on variables, not alias targets, so
`type Value = const int;` is rejected.

## Exhaustive `match`

`match` is an expression for branching on a strong enum. Unlike a normal
`switch`, the compiler checks that every possible variant is handled.

```nss
string DescribeState(EncounterState state)
{
    return match state {
        EncounterState::Idle => "idle",
        EncounterState::Starting => "starting",
        EncounterState::Active => "active",
        EncounterState::Finished => "finished",
    };
}
```

Multiple variants can share an arm:

```nss
int IsRunning(EncounterState state)
{
    return match state {
        EncounterState::Starting | EncounterState::Active => TRUE,
        EncounterState::Idle | EncounterState::Finished => FALSE,
    };
}
```

An arm can have an `int` guard:

```nss
string DescribeState(EncounterState state, int bDetailed)
{
    return match state {
        EncounterState::Active if bDetailed => "active and processing",
        EncounterState::Active => "active",
        _ => "not active",
    };
}
```

Use `_` as the fallback pattern. An unguarded wildcard satisfies the
exhaustiveness check.

An arm may also be a block. The final expression has no semicolon and becomes
the value returned by that arm:

```nss
string DescribeState(EncounterState state)
{
    return match state {
        EncounterState::Idle => "idle",
        EncounterState::Starting | EncounterState::Active => {
            string sPrefix = "encounter: ";
            sPrefix + "running"
        },
        EncounterState::Finished => "finished",
    };
}
```

All value-producing arms must have compatible types. Duplicate, unreachable,
and non-exhaustive arms are compiler errors.

## Compile-time assertions

`static_assert` verifies a constant `int` expression during compilation. A
zero value fails compilation; a nonzero value succeeds. The assertion is
removed before bytecode generation.

```nss
static_assert(EncounterState::Idle != EncounterState::Active);
static_assert(MAX_PLAYERS > 0, "MAX_PLAYERS must be positive");
```

Assertions may appear at the top level or inside a function. The optional
string becomes the diagnostic message when the assertion fails.

Use this for assumptions that should never be deferred until the server is
running.

## Bang macros

nwnrs recognizes three balanced macro invocation forms:

```nss
name!(...)
name![...]
name!{...}
```

The delimiters are part of the input token tree, and nested parentheses,
brackets, and braces remain balanced. Macro invocations are recursively
expanded before ordinary parsing and code generation.

## Declarative macros with `macro_rules!`

Use `macro_rules!` for transformations that can be expressed as a pattern and
a replacement:

```nss
macro_rules! make_logger {
    ($name:ident, $level:expr) => {
        void $name(string sMessage)
        {
            NWNRS_Log(sMessage, $level);
        }
    };
}

make_logger!(LogInfo, NwnrsLogLevel::Info)
```

This generates a normal function named `LogInfo`. Neither the macro definition
nor its invocation reaches the resulting NCS.

Matcher fragments describe what a capture accepts:

| Fragment | Accepts |
| --- | --- |
| `ident` | One identifier |
| `literal` | One literal value |
| `tt` | One token or balanced token group |
| `expr` | One expression |
| `tokens` | A token sequence |

Rust-style repetition is supported:

```nss
$($value:expr),*
$($value:ident)+
$($value:tt)?
```

`*` means zero or more, `+` means one or more, and `?` means zero or one. A
single-token separator such as the comma above can be placed before the
quantifier. Repetitions can be nested, and captures repeated together are
zipped by position.

## Procedural macros written in NWScript

Use `proc_macro!` when pattern replacement is not enough. The macro's
implementation is ordinary NWScript compiled to NCS and executed inside a
bounded compiler VM.

```nss
proc_macro! project::wrap {
    tokenstream wrap(tokenstream input)
    {
        return quote! {
            void GeneratedFunction()
            {
                $input
            }
        };
    }
}

project::wrap!(DoWork();)
```

The final path segment is the required implementation function name. In this
example, `project::wrap!` calls:

```nss
tokenstream wrap(tokenstream input)
```

The function receives its invocation as a lossless token stream and returns
the replacement token stream.

Procedural macros are deterministic and isolated from the game runtime. The
compiler VM limits instructions, recursion, stack use, expansion depth, and
generated output size.

## Quoting generated source

Inside a procedural macro, `quote!` constructs NSS tokens without assembling a
large source string manually:

```nss
tokenstream generated = quote! {
    void Generated()
    {
        $input
    }
};
```

Use `$name` to interpolate a token-stream value and `$$` when the generated
source needs a literal dollar sign. Quoted repetition uses the same forms as a
declarative macro:

```nss
quote! { $($items),* }
```

## Compiler-only macro types

Procedural macro implementations have several types that do not exist in
vanilla NWScript and cannot be used by runtime game scripts:

| Type | Purpose |
| --- | --- |
| `tokenstream` | A lossless sequence of tokens and balanced groups |
| `tokenstream_list` | A collection used for generated repetition and project-wide aggregation |
| `token_cursor` | A position-aware reader for parsing macro input |
| `quote_bindings` | Captured values used while expanding quoted repetition |

Compiler-provided `__NWNRS_Token*`, `__NWNRS_TokenCursor*`,
`__NWNRS_Quote*`, and `__NWNRS_MacroError*` functions let a procedural macro:

- inspect token kinds, text, and delimiter groups;
- parse identifiers, literals, paths, expressions, types, statements,
  functions, and structures;
- build, concatenate, sort, and repeat token streams;
- report an error at the relevant input tokens.

Most module authors will never need these APIs. They are the advanced layer
for authors building reusable source transformations. The complete function
inventory is in the
[compiler README](./crates/nwscript/README.md#strong-enums-aliases-matches-and-assertions).

## Compiler attributes and events

Compiler attributes attach build-time metadata to otherwise normal NWScript.
The currently public project attribute registers event handlers:

```nss
#[nwnrs::events(module_load)]
void OnModuleLoad(json jEvent)
{
    NWNRS_Log("module loaded", NwnrsLogLevel::Info);
}
```

An event handler must have exactly this shape:

```nss
void HandlerName(json jEvent)
{
    // Handle the immutable event snapshot.
}
```

When a module is packaged, nwnrs scans every project source, validates the
event identity and handler signature, generates the dispatcher, subscribes to
the required native events during module load, and includes the handler source
once. Attribute tokens are erased. The dispatcher exists as virtual build-time
source: its behavior is compiled into NCS, but no generated `.nss` file is
written into the project.

Handlers receive one immutable JSON snapshot containing the event name, phase,
target, controls, and event-specific data. Handlers are dispatched in a
deterministic order.

Inside an event handler:

```nss
json jEvent = NWNRS_GetCurrentEvent();

// Only has an effect when the event is defined as skippable.
NWNRS_SkipCurrentEvent();

// Only valid when the event defines a replaceable result schema.
NWNRS_SetCurrentEventResult(jReplacement);
```

The supported event identities and their porting status are tracked in
[EVENTS.md](./EVENTS.md). The compiler rejects unknown identities, duplicate
handler names, and invalid signatures at their original source locations.

## What the compiler does with extended NSS

At a high level, nwnrs processes a script in this order:

1. Resolve `#include` files and vanilla object-like `#define` values.
2. Collect source-defined `macro_rules!` and `proc_macro!` definitions.
3. Recursively expand bang macros.
4. Run project-wide macros such as event dispatcher generation when packaging.
5. Parse and type-check enums, aliases, matches, and assertions.
6. Lower extended constructs to native storage and ordinary control flow.
7. Generate normal NCS bytecode and optional NDB debug information.

To inspect the expanded NSS without compiling it:

```bash
nwnrs expand path/to/script.nss
```

To see each macro invocation and its immediate output:

```bash
nwnrs expand --trace-macros path/to/script.nss
```

To compile normally:

```bash
nwnrs compile path/to/script.nss
```

Module packaging runs the same compiler and additionally performs project-wide
event generation.

## Editor support

The [nwnrs VS Code extension](./editors/vscode-nwnrs/README.md) uses the real
compiler in-process. It currently provides:

- diagnostics and source squiggles;
- unsaved-buffer compilation and multiple independent errors per file;
- hover documentation;
- Go to Definition for functions, macros, enum types, variants, compatibility
  aliases, and type aliases;
- project and include-aware source lookup.
- dependency-aware workspace checks with cancellation and progress.

The extension does not require a separately installed nwnrs CLI.

## Current boundaries

- Extended syntax must go through the nwnrs compiler; the stock NWN compiler
  does not recognize it.
- Enum backing types are limited to `int` and `string`.
- String enum variants always require explicit values.
- `match` operates on strong enums and is exhaustiveness-checked.
- Enum flags are not implemented.
- Conditional compilation is not implemented.
- Event handler priority and ordering attributes are not implemented.
- Compiler-only token types are available only inside procedural macros.
- The generated NCS remains ordinary game bytecode; these features do not add
  runtime reflection or a language-version layer.

## Complete example

This small script combines the features most module authors are likely to use:

```nss
#include "nwnrs"

enum EncounterState {
    #[default]
    Idle,
    Starting,
    Active,
    Finished,
}

type CurrentEncounterState = EncounterState;

static_assert(
    EncounterState::Idle != EncounterState::Finished,
    "encounter states must remain distinct"
);

macro_rules! make_logger {
    ($name:ident, $level:expr) => {
        void $name(string sMessage)
        {
            NWNRS_Log(sMessage, $level);
        }
    };
}

make_logger!(LogEncounter, NwnrsLogLevel::Info)

string DescribeEncounter(CurrentEncounterState state)
{
    return match state {
        EncounterState::Idle => "idle",
        EncounterState::Starting | EncounterState::Active => "running",
        EncounterState::Finished => "finished",
    };
}

#[nwnrs::events(module_load)]
void OnModuleLoad(json jEvent)
{
    CurrentEncounterState state = EncounterState::Idle;
    LogEncounter("Encounter is " + DescribeEncounter(state));
}
```

For compiler implementation details and the full procedural-macro helper API,
see the [nwnrs-nwscript README](./crates/nwscript/README.md). For the event
catalog and implementation status, see [EVENTS.md](./EVENTS.md).
