#![allow(missing_docs)]

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Supported `nwproject` kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectKind {
    Include,
    TwoDa,
    Are,
    Bic,
    Dds,
    Dlg,
    Erf,
    Git,
    Hak,
    Ifo,
    Itp,
    Jrl,
    Key,
    Mdl,
    Mod,
    Ncs,
    Nwm,
    Plt,
    Ssf,
    Tga,
    Tlk,
    Utc,
    Utd,
    Ute,
    Uti,
    Utm,
    Utp,
    Uts,
    Utt,
    Utw,
}

/// High-level source or packaging layout implied by one project kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectLayout {
    Include,
    Resource,
    Erf,
    Key,
}

const ALL_PROJECT_KINDS: [ProjectKind; 30] = [
    ProjectKind::TwoDa,
    ProjectKind::Are,
    ProjectKind::Bic,
    ProjectKind::Dds,
    ProjectKind::Dlg,
    ProjectKind::Erf,
    ProjectKind::Git,
    ProjectKind::Hak,
    ProjectKind::Ifo,
    ProjectKind::Itp,
    ProjectKind::Jrl,
    ProjectKind::Key,
    ProjectKind::Mdl,
    ProjectKind::Mod,
    ProjectKind::Ncs,
    ProjectKind::Nwm,
    ProjectKind::Plt,
    ProjectKind::Ssf,
    ProjectKind::Tga,
    ProjectKind::Tlk,
    ProjectKind::Utc,
    ProjectKind::Utd,
    ProjectKind::Ute,
    ProjectKind::Uti,
    ProjectKind::Utm,
    ProjectKind::Utp,
    ProjectKind::Uts,
    ProjectKind::Utt,
    ProjectKind::Utw,
    ProjectKind::Include,
];

impl ProjectKind {
    /// Default kind used when callers do not specify one.
    pub const DEFAULT: Self = Self::Erf;

    /// Returns the supported kind list in presentation order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &ALL_PROJECT_KINDS
    }

    /// Returns the serialized lowercase kind name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Include => "include",
            Self::TwoDa => "2da",
            Self::Are => "are",
            Self::Bic => "bic",
            Self::Dds => "dds",
            Self::Dlg => "dlg",
            Self::Erf => "erf",
            Self::Git => "git",
            Self::Hak => "hak",
            Self::Ifo => "ifo",
            Self::Itp => "itp",
            Self::Jrl => "jrl",
            Self::Key => "key",
            Self::Mdl => "mdl",
            Self::Mod => "mod",
            Self::Ncs => "ncs",
            Self::Nwm => "nwm",
            Self::Plt => "plt",
            Self::Ssf => "ssf",
            Self::Tga => "tga",
            Self::Tlk => "tlk",
            Self::Utc => "utc",
            Self::Utd => "utd",
            Self::Ute => "ute",
            Self::Uti => "uti",
            Self::Utm => "utm",
            Self::Utp => "utp",
            Self::Uts => "uts",
            Self::Utt => "utt",
            Self::Utw => "utw",
        }
    }

    /// Returns the package layout implied by the kind.
    #[must_use]
    pub const fn layout(self) -> ProjectLayout {
        match self {
            Self::Include => ProjectLayout::Include,
            Self::Erf | Self::Hak | Self::Mod | Self::Nwm => ProjectLayout::Erf,
            Self::Key => ProjectLayout::Key,
            _ => ProjectLayout::Resource,
        }
    }
}

impl fmt::Display for ProjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProjectKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "include" => Ok(Self::Include),
            "2da" => Ok(Self::TwoDa),
            "are" => Ok(Self::Are),
            "bic" => Ok(Self::Bic),
            "dds" => Ok(Self::Dds),
            "dlg" => Ok(Self::Dlg),
            "erf" => Ok(Self::Erf),
            "git" => Ok(Self::Git),
            "hak" => Ok(Self::Hak),
            "ifo" => Ok(Self::Ifo),
            "itp" => Ok(Self::Itp),
            "jrl" => Ok(Self::Jrl),
            "key" => Ok(Self::Key),
            "mdl" => Ok(Self::Mdl),
            "mod" => Ok(Self::Mod),
            "ncs" => Ok(Self::Ncs),
            "nwm" => Ok(Self::Nwm),
            "plt" => Ok(Self::Plt),
            "ssf" => Ok(Self::Ssf),
            "tga" => Ok(Self::Tga),
            "tlk" => Ok(Self::Tlk),
            "utc" => Ok(Self::Utc),
            "utd" => Ok(Self::Utd),
            "ute" => Ok(Self::Ute),
            "uti" => Ok(Self::Uti),
            "utm" => Ok(Self::Utm),
            "utp" => Ok(Self::Utp),
            "uts" => Ok(Self::Uts),
            "utt" => Ok(Self::Utt),
            "utw" => Ok(Self::Utw),
            _ => Err(format!("unsupported project kind: {value}")),
        }
    }
}

impl Serialize for ProjectKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProjectKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{ProjectKind, ProjectLayout};

    #[test]
    fn project_kind_round_trips_through_strings() {
        assert_eq!(
            "utc".parse::<ProjectKind>().expect("parse utc kind"),
            ProjectKind::Utc
        );
        assert_eq!(ProjectKind::Utc.to_string(), "utc");
    }

    #[test]
    fn project_kind_layouts_match_archive_families() {
        assert_eq!(ProjectKind::Mod.layout(), ProjectLayout::Erf);
        assert_eq!(ProjectKind::Key.layout(), ProjectLayout::Key);
        assert_eq!(ProjectKind::Tlk.layout(), ProjectLayout::Resource);
    }
}
