use std::{cell::RefCell, collections::HashMap};

use nwnrs_io::prelude::*;
use tracing::instrument;

use crate::prelude::*;

#[derive(Debug, Clone)]
struct Registry {
    types:   HashMap<ResType, String>,
    reverse: HashMap<String, ResType>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(make_registry());
}

macro_rules! builtin_res_types {
    ($callback:expr) => {{
        let callback = &mut $callback;
        callback(0, "res");
        callback(1, "bmp");
        callback(2, "mve");
        callback(3, "tga");
        callback(4, "wav");
        callback(5, "wfx");
        callback(6, "plt");
        callback(7, "ini");
        callback(8, "bmu");
        callback(9, "mpg");
        callback(10, "txt");
        callback(2000, "plh");
        callback(2001, "tex");
        callback(2002, "mdl");
        callback(2003, "thg");
        callback(2005, "fnt");
        callback(2007, "lua");
        callback(2008, "slt");
        callback(2009, "nss");
        callback(2010, "ncs");
        callback(2011, "mod");
        callback(2012, "are");
        callback(2013, "set");
        callback(2014, "ifo");
        callback(2015, "bic");
        callback(2016, "wok");
        callback(2017, "2da");
        callback(2018, "tlk");
        callback(2022, "txi");
        callback(2023, "git");
        callback(2024, "bti");
        callback(2025, "uti");
        callback(2026, "btc");
        callback(2027, "utc");
        callback(2029, "dlg");
        callback(2030, "itp");
        callback(2031, "btt");
        callback(2032, "utt");
        callback(2033, "dds");
        callback(2034, "bts");
        callback(2035, "uts");
        callback(2036, "ltr");
        callback(2037, "gff");
        callback(2038, "fac");
        callback(2039, "bte");
        callback(2040, "ute");
        callback(2041, "btd");
        callback(2042, "utd");
        callback(2043, "btp");
        callback(2044, "utp");
        callback(2045, "dft");
        callback(2046, "gic");
        callback(2047, "gui");
        callback(2048, "css");
        callback(2049, "ccs");
        callback(2050, "btm");
        callback(2051, "utm");
        callback(2052, "dwk");
        callback(2053, "pwk");
        callback(2054, "btg");
        callback(2055, "utg");
        callback(2056, "jrl");
        callback(2057, "sav");
        callback(2058, "utw");
        callback(2059, "4pc");
        callback(2060, "ssf");
        callback(2061, "hak");
        callback(2062, "nwm");
        callback(2063, "bik");
        callback(2064, "ndb");
        callback(2065, "ptm");
        callback(2066, "ptt");
        callback(2067, "bak");
        callback(2068, "dat");
        callback(2069, "shd");
        callback(2070, "xbc");
        callback(2071, "wbm");
        callback(2072, "mtr");
        callback(2073, "ktx");
        callback(2074, "ttf");
        callback(2075, "sql");
        callback(2076, "tml");
        callback(2077, "sq3");
        callback(2078, "lod");
        callback(2079, "gif");
        callback(2080, "png");
        callback(2081, "jpg");
        callback(2082, "caf");
        callback(2083, "jui");
        callback(9996, "ids");
        callback(9997, "erf");
        callback(9998, "bif");
        callback(9999, "key");
    }};
}

/// Registers a custom resource type and extension mapping.
///
/// # Errors
///
/// Returns [`RegisterResTypeError`] if the extension is invalid or already
/// registered.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(res_type = res_type.0, extension = %extension)
)]
pub fn register_custom_res_type(
    res_type: ResType,
    extension: &str,
) -> Result<(), RegisterResTypeError> {
    let normalized = normalize_extension(extension)?;
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.types.insert(res_type, normalized.clone());
        registry.reverse.insert(normalized, res_type);
    });
    Ok(())
}

/// Returns `true` if `res_type` is registered.
#[instrument(level = "debug", fields(res_type = res_type.0))]
pub fn res_type_registered(res_type: ResType) -> bool {
    REGISTRY.with(|registry| registry.borrow().types.contains_key(&res_type))
}

/// Returns `true` if `extension` is registered.
#[instrument(level = "debug", skip_all, fields(extension = %extension))]
pub fn res_ext_registered(extension: &str) -> bool {
    let normalized = extension.to_ascii_lowercase();
    REGISTRY.with(|registry| registry.borrow().reverse.contains_key(&normalized))
}

/// Looks up the numeric resource type for `extension`.
#[instrument(level = "debug", skip_all, fields(extension = %extension))]
pub fn lookup_res_type(extension: &str) -> Option<ResType> {
    let normalized = extension.to_ascii_lowercase();
    REGISTRY.with(|registry| registry.borrow().reverse.get(&normalized).copied())
}

/// Looks up the extension for `res_type`.
#[instrument(level = "debug", fields(res_type = res_type.0))]
pub fn lookup_res_ext(res_type: ResType) -> Option<String> {
    REGISTRY.with(|registry| registry.borrow().types.get(&res_type).cloned())
}

/// Returns the registered resource type for `extension`, panicking if it is
/// unknown.
#[instrument(level = "debug", skip_all, fields(extension = %extension))]
pub fn get_res_type(extension: &str) -> ResType {
    match lookup_res_type(extension) {
        Some(value) => value,
        None => unreachable!("resource extension is not registered"),
    }
}

/// Returns the registered extension for `res_type`, panicking if it is unknown.
#[instrument(level = "debug", fields(res_type = res_type.0))]
pub fn get_res_ext(res_type: ResType) -> String {
    match lookup_res_ext(res_type) {
        Some(value) => value,
        None => unreachable!("resource type is not registered"),
    }
}

/// Resets the registry to the built-in NWN resource set.
#[cfg(test)]
pub fn reset_registry_for_tests() {
    REGISTRY.with(|registry| *registry.borrow_mut() = make_registry());
}

fn make_registry() -> Registry {
    let mut registry = Registry {
        types:   HashMap::new(),
        reverse: HashMap::new(),
    };

    builtin_res_types!(|ty: u16, ext: &str| {
        registry.types.insert(ResType(ty), ext.to_string());
        registry
            .reverse
            .insert(ext.to_ascii_lowercase(), ResType(ty));
    });

    registry
}

fn normalize_extension(extension: &str) -> Result<String, RegisterResTypeError> {
    expect(
        !extension.is_empty() && extension.len() <= 3,
        "Expectation failed: extension.len() > 0 and extension.len() <= 3",
    )?;

    let normalized = extension.to_ascii_lowercase();
    if normalized.len() != 3
        || !normalized
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
    {
        return Err(RegisterResTypeError::InvalidCharacters(
            extension.to_string(),
        ));
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use crate::{
        ResType, get_res_ext, lookup_res_type, register_custom_res_type, reset_registry_for_tests,
    };

    #[test]
    fn builtins_roundtrip_through_registry() {
        reset_registry_for_tests();
        assert_eq!(lookup_res_type("2da"), Some(ResType(2017)));
        assert_eq!(get_res_ext(ResType(2018)), "tlk");
    }

    #[test]
    fn custom_types_are_normalized_and_validated() {
        reset_registry_for_tests();
        if let Err(error) = register_custom_res_type(ResType(65000), "ABC") {
            panic!("register custom: {error}");
        }
        assert_eq!(lookup_res_type("abc"), Some(ResType(65000)));
        assert!(register_custom_res_type(ResType(65001), "bad!").is_err());
    }
}
