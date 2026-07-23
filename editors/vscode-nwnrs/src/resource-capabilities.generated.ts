// Generated from ../resource-capabilities.json by sync-resource-capabilities.ts.
// Do not edit by hand.

const CUSTOM_EDITOR_SUFFIXES = [".2da",".tlk",".dds",".tga",".plt",".gff",".utc",".utd",".ute",".uti",".utm",".utp",".uts",".utt",".utw",".git",".are",".gic",".ifo",".fac",".dlg",".dlg.json",".itp",".bic",".jrl",".gui",".erf",".hak",".mod",".nwm",".key",".ncs",".ndb",".mdl",".wok",".dwk",".pwk"] as const;
const VIEWER_SUFFIXES = [".utc",".utd",".uti",".utp",".git",".are",".ifo",".mdl",".wok",".dwk",".pwk"] as const;
const TEXT_SUFFIXES = [".mtr",".txi",".shd",".set",".nss",".lua",".txt",".ini",".css"] as const;

function hasSuffix(value: string, suffixes: readonly string[]): boolean {
  const normalized = value.toLowerCase();
  return suffixes.some((suffix) => normalized.endsWith(suffix));
}

export function isCustomEditorResource(value: string): boolean {
  return hasSuffix(value, CUSTOM_EDITOR_SUFFIXES);
}

export function isViewerResource(value: string): boolean {
  return hasSuffix(value, VIEWER_SUFFIXES);
}

export function isTextResource(value: string): boolean {
  return hasSuffix(value, TEXT_SUFFIXES);
}
