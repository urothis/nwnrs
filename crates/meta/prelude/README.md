# nwnrs

`nwnrs` is the umbrella crate for the public workspace surface. It is a guided
map and re-export surface, not a second abstraction layer over the workspace.

## Guided Map

The project is large enough that filesystem order is not a useful way to learn
it. The most useful reading order is the dependency order:

1. foundation primitives
2. resource identity and resolution
3. file and asset formats
4. language tooling
5. public interfaces

That order mirrors how the system is actually built.

`nwnrs` is also a reverse-engineering project. Many of the crates exist to
capture recovered semantics from Neverwinter Nights without smearing those
semantics across unrelated layers. The documentation is written with that in
mind: what each crate knows, what it does not know, and which types matter
first.

If you only care about one problem area:

- format parsing and writing: start with [`gff`], [`twoda`], [`tlk`], [`ssf`],
  [`tga`], [`dds`], [`plt`], [`txi`], [`mtr`], [`mdl`], [`erf`], [`key`], and
  [`nwsync`]
- install-backed resource loading: start with [`restype`], [`resref`],
  [`resman`], and [`install`]
- NWScript compilation: jump to [`nwscript`]
- the operational entry points: see the
  [`nwnrs-cli` README](https://github.com/urothis/nwnrs/blob/main/cli/README.md)
  and the
  [`nwnrs-wasm` README](https://github.com/urothis/nwnrs/blob/main/wasm/README.md)

One rule matters throughout this codebase: different layers promise different
fidelity. Some layers preserve binary or textual structure. Some normalize.
Some compose. Some export. If two types look similar but live in different
crates, assume the difference is intentional.

## Scope

- re-export the public crates through root modules such as [`gff`], [`twoda`],
  [`resman`], and [`install`]
- provide a convenience [`prelude`] module for callers that prefer one import
  boundary
- keep the top-level API aligned with the workspace crate boundaries rather than
  introducing a second abstraction layer
- provide the workspace-level guided documentation that replaced the old
  standalone book

## Example

```rust
use nwnrs::{
    gff::{GffRoot, GffValue},
    twoda::TwoDa,
};

let mut root = GffRoot::new("UTC ");
root.put_value("Tag", GffValue::CExoString("nw_chicken".to_string()))?;

let mut table = TwoDa::new();
table.set_columns(vec!["Label".to_string()])?;
table.replace_rows(
    vec![vec![Some("Chicken".to_string())]],
    vec!["0".to_string()],
)?;

assert_eq!(root.file_type, "UTC ");
assert_eq!(table.cell_or(0, "Label", ""), "Chicken");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Foundation

The foundation crates are the lowest-level reusable pieces in the workspace.
They are intentionally small. They should not know about NWN domain objects
beyond the minimum needed to support higher layers.

The order to learn them is:

1. [`io`] for exact reads, endian conversion, and small invariant helpers
2. [`encoding`] for NWN-side and host-side text encoding policy
3. [`localization`] for `Language`, `Gender`, and `StrRef`
4. [`checksums`] for typed digest handling
5. [`lru`] for weighted bounded caching
6. [`streamext`] for size-prefixed and framed stream helpers

If you are trying to understand why a format crate is written the way it is,
start here before diving into the format itself.

## Resource Identity and Resolution

Once you move above the foundation layer, the next thing to understand is how
`nwnrs` names and resolves game resources.

This layer matters because most higher-level workflows are not driven by files
in isolation. They are driven by resource identities and lookup order across
installs, user directories, archives, manifests, and overrides.

The key progression is:

1. [`restype`] for resource-type identity
2. the shipped built-in resource catalog below
3. [`resref`] for resource reference identity
4. [`resman`] for lookup algebra
5. [`install`] for conventional install-backed assembly
6. the concrete resource backends listed below

### Built-In Resource Catalog

The source of truth is the registry in `nwnrs-restype`.

Two cautions matter:

1. a registered resource type is only an identity mapping
2. several resource kinds share one physical substrate such as `GFF`, while
   differing primarily by top-level tag and schema semantics

#### Core media and miscellaneous

- `0 => res`
- `1 => bmp`
- `2 => mve`
- `3 => tga`
- `4 => wav`
- `5 => wfx`
- `6 => plt`
- `7 => ini`
- `8 => bmu`
- `9 => mpg`
- `10 => txt`

#### Model, texture, and material families

- `2001 => tex`
- `2002 => mdl`
- `2003 => thg`
- `2016 => wok`
- `2022 => txi`
- `2024 => bti`
- `2033 => dds`
- `2052 => dwk`
- `2053 => pwk`
- `2065 => ptm`
- `2066 => ptt`
- `2069 => shd`
- `2072 => mtr`
- `2073 => ktx`
- `2078 => lod`

Current first-class typed crates in this family:

- [`mdl`]
- [`tga`]
- [`dds`]
- [`plt`]
- [`txi`]
- [`mtr`]

#### Script and code artifacts

- `2007 => lua`
- `2009 => nss`
- `2010 => ncs`
- `2048 => css`
- `2049 => ccs`
- `2064 => ndb`

Current first-class typed crate in this family:

- [`nwscript`]

#### Tables, localization, and small data formats

- `2005 => fnt`
- `2017 => 2da`
- `2018 => tlk`
- `2036 => ltr`
- `2060 => ssf`
- `9996 => ids`

Current first-class typed crates in this family:

- [`twoda`]
- [`tlk`]
- [`ssf`]

#### GFF-backed gameplay and tooling resources

- `2012 => are`
- `2014 => ifo`
- `2015 => bic`
- `2023 => git`
- `2025 => uti`
- `2027 => utc`
- `2029 => dlg`
- `2030 => itp`
- `2032 => utt`
- `2035 => uts`
- `2037 => gff`
- `2038 => fac`
- `2040 => ute`
- `2042 => utd`
- `2044 => utp`
- `2046 => gic`
- `2047 => gui`
- `2050 => btm`
- `2051 => utm`
- `2054 => btg`
- `2055 => utg`
- `2056 => jrl`
- `2058 => utw`

Current first-class typed crates in this family:

- [`gff`]
- [`git`]

Important implication: today, most of these schemas are represented in the repo
as `GffRoot` plus domain knowledge, not as one dedicated crate per file tag.

#### Area, module, and package containers

- `2011 => mod`
- `2057 => sav`
- `2061 => hak`
- `2062 => nwm`
- `9997 => erf`
- `9998 => bif`
- `9999 => key`

Current first-class typed crates in this family:

- [`erf`]
- [`key`]

#### Database, UI, and other engine artifacts

- `2039 => bte`
- `2045 => dft`
- `2059 => 4pc`
- `2067 => bak`
- `2068 => dat`
- `2070 => xbc`
- `2071 => wbm`
- `2074 => ttf`
- `2075 => sql`
- `2076 => tml`
- `2077 => sq3`
- `2079 => gif`
- `2080 => png`
- `2081 => jpg`
- `2082 => caf`
- `2083 => jui`

If you are reverse-engineering the ecosystem, the first question is often not
"how do I parse this file?" but "what class of thing is this supposed to be?"
The registry answers that identity question and makes the current implementation
boundary visible.

### Concrete Resource Backends

These crates are the concrete storage forms that implement the abstract
`ResContainer` model.

- [`resdir`]
  Maps a directory tree into typed resource references. It does not define
  precedence between multiple directories. That is a [`resman`] concern.
- [`resfile`]
  Wraps one file as one resource entry. It is the minimal bridge from "I have
  this one file" into the `ResContainer` world.
- [`resmemfile`]
  Exists for synthetic, downloaded, or otherwise non-filesystem-backed payloads.
  It lets higher layers treat in-memory content like any other resource
  container.
- [`resnwsync`]
  Is the repository-layout side of `NWSync`, not the manifest file-format side.
  It maps manifests and shard storage into resource-container semantics.

The point of the backend layer is to let the rest of the workspace care about
lookup semantics instead of storage-specific mechanics. Once a backend can
behave like a `ResContainer`, higher layers can compose it with the rest of the
system.

## Formats

The format layer is where reverse-engineered wire layouts become typed Rust
representations.

Read this part in two passes:

1. use the family sections to orient yourself around the major problem domains
2. use the dedicated crate docs for concrete layouts, exported types, fidelity
   boundaries, and awkward edge cases

Most of the subtlety in this workspace is not just "how do I parse the bytes?"
but "what invariants are actually stable enough to model?"

### Core Data Formats

This family covers the low-level data formats that a large portion of the rest
of the workspace depends on.

- [`gff`]
- [`twoda`]
- [`tlk`]
- [`ssf`]
- [`exo`]

Why this family matters:

- `GFF` is the structural substrate for a large fraction of gameplay-facing
  resources
- many engine resource kinds are schema variants over `GFF`, not separate
  container formats
- `2DA` and `TLK` are foundational because they anchor table-driven behavior
  and localized text
- `SSF` is small but semantically positional
- `EXO` defines shared compression markers that show up in container formats

### Textures and Materials

This family covers raw texture payloads, palette-oriented texture encodings, and
sidecar material metadata.

- [`tga`]
- [`dds`]
- [`plt`]
- [`txi`]
- [`mtr`]

The important split here is between canonical stored payloads such as `TGA`,
`DDS`, and `PLT`; sidecar or descriptor layers such as `TXI` and `MTR`; and
derived views such as "decode to RGBA8" or "render PLT with a palette". Those
are not interchangeable representations.

### Models and World Data

This family contains the richest object models in the workspace.

- [`mdl`]
- [`git`]
- [`set`]

The common theme is that these formats are not just containers of fields. They
encode graph structure, transforms, placement, composition rules, and
editor-authored catalogs.

### Archives, Compression, and Sync

This family covers framed payloads, archive containers, and distribution
metadata.

- [`compressedbuf`]
- [`erf`]
- [`key`]
- [`nwsync`]

This is the family where resource identity and physical storage diverge most
sharply:

- compressed-buffer framing is not the same thing as the payload format
- archive membership is not the same thing as resource precedence
- KEY indexing is not the same thing as BIF storage
- an `NWSync` manifest is not the same thing as an `NWSync` repository

### GFF Schema Families

This section sits one layer above [`gff`]. The `GFF` crate explains the
container mechanics. The sections below explain the major schema families that
ride on top of that container.

These are not all implemented today as dedicated Rust crates, so the goal here
is architectural accuracy rather than pretending the repo already has
field-by-field typed coverage for every file tag.

#### The Core Distinction

At this layer, a resource is no longer just "some GFF document." Its top-level
file tag determines what kind of object graph the engine expects.

```text
GFF container
|
+-- file tag "UTC "  -> creature blueprint schema
+-- file tag "UTI "  -> item blueprint schema
+-- file tag "ARE "  -> area static data schema
+-- file tag "GIT "  -> placed instance schema
+-- file tag "DLG "  -> dialogue graph schema
+-- file tag "JRL "  -> journal schema
+-- ...
```

That means reverse engineering has two separate obligations:

1. parse `GFF` correctly
2. know what the top-level schema means for a given file tag

#### Current Coverage Boundary

Today the repo has:

- first-class generic `GFF` support in [`gff`]
- first-class lifted `GIT` support in [`git`]
- broad registry coverage for many GFF-backed resource kinds via [`restype`]

For most other GFF-backed resource families, the codebase currently stops at the
container layer. The material below documents those schema classes without
misrepresenting current code coverage.

#### Blueprint Resources

These are the canonical template-style `GFF` resources that describe authored
game objects before placement or runtime instantiation.

Common members of this family:

- `UTC` creature blueprint
- `UTI` item blueprint
- `UTP` placeable blueprint
- `UTD` door blueprint
- `UTM` merchant/store blueprint
- `UTE` encounter blueprint
- `UTS` sound blueprint
- `UTT` trigger blueprint
- `UTW` waypoint blueprint

Shared structural pattern:

```text
GFF root tagged as one blueprint kind
|
+-- identity fields        tag, resref links, localized name
+-- appearance/model refs  appearance ids, model/material hooks, portraits
+-- gameplay data          stats, flags, faction/classification, scripts
+-- inventory/equipment    optional embedded lists or references
+-- locals/vars            optional per-object authored state
+-- script hooks           event-entry script names
```

The main semantics:

- a blueprint is a template, not a placed instance
- a blueprint usually owns object-level default state
- a placed instance layer such as [`git`] may refer back to a blueprint by
  `template_resref`
- embedded substructures are still usually ordinary `GFF` structs/lists rather
  than a different container type

##### `UTC` creature blueprints

- extension: `utc`
- resource type: `2027`
- top-level GFF tag: `"UTC "`

Role:

- describes one authored creature template before placement
- is the object later instantiated into an area, often through a `GIT`
  reference or other spawning mechanism

Conceptual shape:

```text
UTC root
|
+-- identity            tag, name, template identity
+-- classification      race, class, faction, challenge-style metadata
+-- appearance          appearance rows, portrait/model hooks, animation-facing ids
+-- stats               attributes, saves, combat-facing defaults
+-- scripts             event hooks
+-- inventory/equipment nested lists and owned items
+-- locals/state        authored default object state
```

Logical edges:

- `UTC` is the template; `GIT` is the placed instance
- a creature blueprint is not character-save state; that is closer to `BIC`
- appearance-related fields usually reference other layers such as `2DA`,
  `MDL`, `PLT`, `TXI`, `MTR`, and `TLK`; the `UTC` itself does not subsume
  those formats

##### `UTI` item blueprints

- extension: `uti`
- resource type: `2025`
- top-level GFF tag: `"UTI "`

Role:

- defines one authored item template
- describes default classification, appearance-facing configuration, localized
  naming and description, and gameplay defaults

Conceptual shape:

```text
UTI root
|
+-- identity            tag, localized names, blueprint identity
+-- classification      base item type, category, rarity-like flags
+-- appearance          icon/model/texture-facing references
+-- mechanics           cost, stack/charge/use properties, item-specific flags
+-- properties          nested item-property lists
+-- scripts/metadata    optional behavior hooks and authored notes
```

Logical edges:

- `UTI` is an item template, not one runtime inventory-slot instance
- the item-property list is usually the key nested semantic payload
- visual appearance may fan out into many other formats, but `UTI` remains the
  coordinating template

##### `UTP` placeable blueprints

- extension: `utp`
- resource type: `2044`
- top-level GFF tag: `"UTP "`

Conceptual shape:

```text
UTP root
|
+-- identity            tag, localized name, blueprint identity
+-- appearance          appearance/model references
+-- interaction flags   usability, lock/open/trap-like behavior
+-- scripts             event hooks
+-- inventory/contents  optional nested owned resources
+-- state defaults      authored object defaults
```

Logical edges:

- a placeable blueprint is not a placed placeable instance; placement belongs
  in `GIT`
- many placeables are containers, so the schema often mixes world-object
  identity with nested inventory semantics
- a placeable can participate in pathing, trap, lock, and script systems
  simultaneously

##### `UTD` door blueprints

- extension: `utd`
- resource type: `2042`
- top-level GFF tag: `"UTD "`

Conceptual shape:

```text
UTD root
|
+-- identity            tag, localized name, blueprint identity
+-- appearance          appearance/model state
+-- connection          transition/link metadata
+-- interaction         lock/open/trap state defaults
+-- scripts             event hooks
+-- miscellaneous       door-specific authored defaults
```

Logical edges:

- door connection and transition semantics make `UTD` more than "a placeable
  with a different appearance"
- placed door state in `GIT` adds transform and instance-local information
- door-related navigation also interacts with non-GFF world resources such as
  walkmesh and pathing data

##### `UTM` store blueprints

- extension: `utm`
- resource type: `2051`
- top-level GFF tag: `"UTM "`

Conceptual shape:

```text
UTM root
|
+-- identity            tag, localized name, blueprint identity
+-- economics           price modifiers and store policy
+-- inventory/catalog   nested saleable item list
+-- scripts             event hooks
+-- metadata            authored flags and store defaults
```

Logical edges:

- a store blueprint is not itself a placed merchant actor
- nested inventory in `UTM` behaves more like a curated resource catalog than a
  loose runtime container
- economic behavior is part of the schema, not an external spreadsheet layered
  on later

##### `UTE` encounter blueprints

- extension: `ute`
- resource type: `2040`
- top-level GFF tag: `"UTE "`

Conceptual shape:

```text
UTE root
|
+-- identity            tag, name, blueprint identity
+-- spawn policy        counts, limits, reset-like behavior
+-- roster              nested spawnable creature references
+-- scripts             encounter hooks
+-- metadata            authored encounter defaults
```

Logical edges:

- encounter geometry belongs in `GIT`, not `UTE`
- the roster structure is usually the key semantic payload
- encounter blueprints are a clear example of template-vs-instance separation

##### `UTS` sound blueprints

- extension: `uts`
- resource type: `2035`
- top-level GFF tag: `"UTS "`

Conceptual shape:

```text
UTS root
|
+-- identity            tag, localized/display metadata
+-- playback policy     radius/loop/randomization-style defaults
+-- sound references    nested or repeated sound identifiers
+-- scripts             event hooks when applicable
+-- authored defaults   object-level sound behavior
```

Logical edges:

- `UTS` is not the raw audio payload
- a placed sound emitter in `GIT` adds concrete transform and instance state
- sound blueprints often mix content selection with playback policy

##### `UTT` trigger blueprints

- extension: `utt`
- resource type: `2032`
- top-level GFF tag: `"UTT "`

Conceptual shape:

```text
UTT root
|
+-- identity            tag, localized/display metadata
+-- trigger policy      cursor/interaction/trap/transition-like defaults
+-- scripts             event hooks
+-- authored state      default trigger properties
```

Logical edges:

- trigger polygon geometry belongs in `GIT`, not `UTT`
- trigger schemas are behavior-heavy rather than model-heavy
- transition logic, script hooks, and local state tend to matter more than
  visual appearance

##### `UTW` waypoint blueprints

- extension: `utw`
- resource type: `2058`
- top-level GFF tag: `"UTW "`

Conceptual shape:

```text
UTW root
|
+-- identity            tag, localized name, blueprint identity
+-- map/display         waypoint-facing presentation metadata
+-- scripts/state       hooks and local defaults
```

Logical edges:

- a waypoint is semantically light compared to many other blueprint classes,
  but it still benefits from a dedicated schema identity
- concrete placement still belongs in `GIT`
- waypoint identity is often more important than large amounts of nested state

#### Area and Module Resources

This family covers the GFF-backed resources that describe module-level and
area-level structure rather than one standalone gameplay object.

Common members of this family:

- `ARE` area static data
- `GIT` placed instance data
- `IFO` module metadata and top-level module settings
- `GIC` ancillary area companion metadata

The important split is:

```text
module / area identity and static configuration
!=
placed runtime-facing instances
```

Another useful diagram:

```text
module
|
+-- IFO   module identity and defaults
|
+-- area A
|   +-- ARE  static area data
|   +-- GIT  placed objects and geometry
|   +-- GIC  companion metadata
|
+-- area B
    +-- ARE
    +-- GIT
    +-- GIC
```

Current typed coverage:

- [`git`] has a dedicated lifted schema crate
- `ARE`, `IFO`, and `GIC` currently remain primarily documented and tagged
  `GFF` families rather than dedicated lifted schema crates

##### `ARE` area static data

- extension: `are`
- resource type: `2012`
- top-level GFF tag: `"ARE "`

Conceptual shape:

```text
ARE root
|
+-- identity            area name and area-level metadata
+-- environment         lighting/ambient/music-like defaults
+-- area policy         flags and authored configuration
+-- references          links to related area resources
```

Logical edges:

- area-level ambient and environment settings do not belong inside each placed
  object
- static area metadata and instance placement evolve on different axes
- nearby non-GFF resources such as tilesets and walkmeshes participate in area
  realization, but are not encoded inside the `ARE` schema itself

##### `IFO` module metadata

- extension: `ifo`
- resource type: `2014`
- top-level GFF tag: `"IFO "`

Conceptual shape:

```text
IFO root
|
+-- module identity     name, description, top-level metadata
+-- configuration       module-wide defaults and policy
+-- entry references    start locations / starting-area style links
+-- scripts             module-scope hooks
+-- metadata            authored module information
```

Logical edges:

- module-scope configuration does not substitute for area-scope `ARE` data
- entry and start references create relationships across other resource kinds
- `IFO` is orchestration metadata, not a physical scene description

##### `GIC` area companion data

- extension: `gic`
- resource type: `2046`
- top-level GFF tag: `"GIC "`

Conceptual shape:

```text
GIC root
|
+-- area companion metadata
+-- auxiliary authored configuration
+-- references into neighboring area resources
```

Logical edges:

- the existence of a companion area document is itself important architectural
  information
- reverse engineering should resist flattening every area-related field into
  `ARE`
- `GIC` is best understood relative to the rest of the area family rather than
  in isolation

#### Dialogue, Journal, and Meta Resources

This family covers GFF-backed resources whose main semantics are graph,
campaign-state, UI, or tooling metadata rather than one physical object in the
world.

Common members:

- `DLG` dialogue resources
- `JRL` journal resources
- `FAC` faction resources
- `GUI` GUI layout and configuration resources
- `ITP` tool palette resources
- `BIC` character resources

Structural pattern:

```text
GFF root
|
+-- graph or catalog metadata
+-- ordered and/or keyed lists
+-- localized strings
+-- references into scripts, portraits, models, or resource ids
+-- state classification fields
```

Current coverage boundary:

- container mechanics are implemented
- type identity is implemented
- dedicated schema lifting is selective rather than universal

##### `DLG` dialogue graphs

- extension: `dlg`
- resource type: `2029`
- top-level GFF tag: `"DLG "`

Conceptual shape:

```text
DLG root
|
+-- dialogue metadata
+-- node list           entries, replies, or comparable graph nodes
+-- link structure      outgoing transitions
+-- conditions/actions  script-facing logic hooks
+-- text references     localized content bindings
```

Logical edges:

- ordering and linkage are core semantics, not incidental serialization detail
- a dialogue graph is not well represented as one flat object record
- text payloads often live in `TLK`, so the dialogue schema is partially a
  graph over external localized references

##### `JRL` journal data

- extension: `jrl`
- resource type: `2056`
- top-level GFF tag: `"JRL "`

Conceptual shape:

```text
JRL root
|
+-- journal metadata
+-- category list
+-- entry lists within categories
+-- localized text references
+-- progression/classification metadata
```

Logical edges:

- journal semantics are catalog and progression oriented rather than spatial
- ordering often matters because journal displays and progression logic depend
  on explicit structure
- localized content is typically externalized even when the journal owns the
  progression graph

##### `FAC` faction data

- extension: `fac`
- resource type: `2038`
- top-level GFF tag: `"FAC "`

Conceptual shape:

```text
FAC root
|
+-- faction list
+-- faction metadata
+-- inter-faction relationship data
+-- module-facing defaults or classification
```

Logical edges:

- faction relationships are relational data, not one isolated object record
- this schema is closer to a policy and configuration graph than to a world
  object
- consumer code should not confuse faction-membership references with the
  faction-definition catalog itself

##### `GUI` UI resources

- extension: `gui`
- resource type: `2047`
- top-level GFF tag: `"GUI "`

Conceptual shape:

```text
GUI root
|
+-- screen metadata
+-- control/widget list
+-- layout and presentation configuration
+-- resource references for UI assets
+-- behavior/configuration metadata
```

Logical edges:

- UI structure is graph and catalog data, not a world-space scene
- ordering and nesting are often semantically important
- the schema may reference non-GFF assets such as textures or fonts, but is not
  itself those payload formats

##### `ITP` tool palettes

- extension: `itp`
- resource type: `2030`
- top-level GFF tag: `"ITP "`

Conceptual shape:

```text
ITP root
|
+-- palette metadata
+-- category/group hierarchy
+-- entry list
+-- references to blueprint/resource kinds
```

Logical edges:

- `ITP` is tooling and catalog data, not one gameplay object
- it is closely related to blueprint resource families because palette entries
  usually point at those templates
- hierarchy and ordering are usually part of the meaning

##### `BIC` character resources

- extension: `bic`
- resource type: `2015`
- top-level GFF tag: `"BIC "`

Conceptual shape:

```text
BIC root
|
+-- identity            player/character identity
+-- progression         levels, classes, feats, skills, progression state
+-- inventory/equipment owned item state
+-- appearance          character-facing presentation state
+-- locals/state        persisted character data
```

Difference from `UTC`:

- `UTC` is the blueprint template for a creature archetype
- `BIC` is persisted character state

Logical edges:

- persisted character state is not just "a creature template with more fields"
- inventory and progression information are central, not incidental
- character state links outward to many other resource classes without
  replacing them

## Language and Compiler

There is currently one language-oriented crate in the workspace: [`nwscript`].

This crate is different from the rest of the format layer because it is not
just a codec. It is a compiler subsystem with multiple internal
representations and artifact types.

## Interfaces

These are the layers most consumers touch directly:

- `nwnrs` is the umbrella crate
- [`nwnrs-cli`](https://github.com/urothis/nwnrs/blob/main/cli/README.md) is
  the operational command-line interface
- [`nwnrs-wasm`](https://github.com/urothis/nwnrs/blob/main/wasm/README.md) is
  the browser and JavaScript boundary

Each of these should be read after the lower layers, because they are meant to
sit on top of the domain logic rather than replace it.

## Public Surface

### Root modules

- [`checksums`]
- [`compressedbuf`]
- [`dds`]
- [`encoding`]
- [`erf`]
- [`exo`]
- [`gff`]
- [`git`]
- [`io`]
- [`key`]
- [`localization`]
- [`lru`]
- [`masterlist`]
- [`mdl`]
- [`mtr`]
- [`nwscript`]
- [`nwsync`]
- [`plt`]
- [`resdir`]
- [`resfile`]
- [`resman`]
- [`resmemfile`]
- [`resnwsync`]
- [`resref`]
- [`restype`]
- [`set`]
- [`ssf`]
- [`streamext`]
- [`tga`]
- [`tlk`]
- [`twoda`]
- [`txi`]
- [`install`] on non-wasm targets

### Convenience namespace

- [`prelude`]

## Logical Edges

- the root modules mirror workspace crate boundaries intentionally
- the umbrella crate is about import ergonomics and navigation, not about
  hiding the underlying architecture
- if a consumer wants explicit imports, they should prefer root modules over
  the wildcard [`prelude`]

## Why This Crate Exists

Most downstream users need one stable import boundary over the workspace. This
crate provides that without flattening away the actual subsystem structure.
