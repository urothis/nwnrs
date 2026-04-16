# Built-In Resource Catalog

Docs:

- crate: `nwnrs-restype`
- [registry source](https://github.com/urothis/nwnrs/blob/main/crates/resources/restype/src/registry.rs)

This chapter is the built-in resource-type catalog as the workspace currently
ships it. The source of truth is the registry in `nwnrs-restype`.

Two cautions matter here:

1. A registered resource type is only an identity mapping. It does not imply
   there is already a dedicated typed parser crate for that payload.
2. Several resource kinds share one physical substrate such as `GFF`, while
   differing primarily by top-level file tag and schema semantics.

## Core Media and Miscellaneous

- `0` => `res`
- `1` => `bmp`
- `2` => `mve`
- `3` => `tga`
- `4` => `wav`
- `5` => `wfx`
- `6` => `plt`
- `7` => `ini`
- `8` => `bmu`
- `9` => `mpg`
- `10` => `txt`

## Model, Texture, and Material Families

- `2001` => `tex`
- `2002` => `mdl`
- `2003` => `thg`
- `2016` => `wok`
- `2022` => `txi`
- `2024` => `bti`
- `2033` => `dds`
- `2052` => `dwk`
- `2053` => `pwk`
- `2065` => `ptm`
- `2066` => `ptt`
- `2069` => `shd`
- `2072` => `mtr`
- `2073` => `ktx`
- `2078` => `lod`

Current first-class typed crates in this family:

- `nwnrs-mdl`
- `nwnrs-tga`
- `nwnrs-dds`
- `nwnrs-plt`
- `nwnrs-txi`
- `nwnrs-mtr`

## Script and Code Artifacts

- `2007` => `lua`
- `2009` => `nss`
- `2010` => `ncs`
- `2048` => `css`
- `2049` => `ccs`
- `2064` => `ndb`

Current first-class typed crate in this family:

- `nwnrs-nwscript`

## Tables, Localization, and Small Data Formats

- `2005` => `fnt`
- `2017` => `2da`
- `2018` => `tlk`
- `2036` => `ltr`
- `2060` => `ssf`
- `9996` => `ids`

Current first-class typed crates in this family:

- `nwnrs-twoda`
- `nwnrs-tlk`
- `nwnrs-ssf`

## GFF-Backed Gameplay and Tooling Resources

- `2012` => `are`
- `2014` => `ifo`
- `2015` => `bic`
- `2023` => `git`
- `2025` => `uti`
- `2027` => `utc`
- `2029` => `dlg`
- `2030` => `itp`
- `2032` => `utt`
- `2035` => `uts`
- `2037` => `gff`
- `2038` => `fac`
- `2040` => `ute`
- `2042` => `utd`
- `2044` => `utp`
- `2046` => `gic`
- `2047` => `gui`
- `2050` => `btm`
- `2051` => `utm`
- `2054` => `btg`
- `2055` => `utg`
- `2056` => `jrl`
- `2058` => `utw`

Current first-class typed crates in this family:

- `nwnrs-gff`
- `nwnrs-git`

Important implication:

- today, most of these schemas are represented in the repo as `GffRoot` plus
  domain knowledge, not as one dedicated crate per file tag

## Area, Module, and Package Containers

- `2011` => `mod`
- `2057` => `sav`
- `2061` => `hak`
- `2062` => `nwm`
- `9997` => `erf`
- `9998` => `bif`
- `9999` => `key`

Current first-class typed crates in this family:

- `nwnrs-erf`
- `nwnrs-key`

## Database, UI, and Other Engine Artifacts

- `2039` => `bte`
- `2045` => `dft`
- `2059` => `4pc`
- `2067` => `bak`
- `2068` => `dat`
- `2070` => `xbc`
- `2071` => `wbm`
- `2074` => `ttf`
- `2075` => `sql`
- `2076` => `tml`
- `2077` => `sq3`
- `2079` => `gif`
- `2080` => `png`
- `2081` => `jpg`
- `2082` => `caf`
- `2083` => `jui`

## Why This Chapter Exists

If you are reverse engineering the ecosystem, the first question is often not
"how do I parse this file?" but "what class of thing is this supposed to be?"
The registry answers that identity question and, just as importantly, makes the
current implementation boundary visible:

- some resource kinds already have dedicated typed crates
- some are intentionally documented but not yet fully lifted
- several are schema variants over `GFF` rather than wholly separate container
  formats
