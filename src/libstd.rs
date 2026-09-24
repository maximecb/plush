//! Standard library modules baked into the binary.
//!
//! These are imported without a leading `./`, e.g. `from random import *;`,
//! which sets them apart from relative imports of files on disk.

/// Key under which a module is registered in the program. Unit keys are
/// canonical file paths, so this is shaped to not collide with one, and
/// to read clearly in error messages
pub fn unit_key(module_path: &str) -> String
{
    format!("<std>/{}.psh", module_path)
}

/// Get the source of a module given its unit key, if the key names one
pub fn get_source(unit_key: &str) -> Option<&'static str>
{
    let module_path = unit_key
        .strip_prefix("<std>/")?
        .strip_suffix(".psh")?;

    let src = match module_path {
        "csv" => include_str!("../lib/std/csv.psh"),
        "datetime" => include_str!("../lib/std/datetime.psh"),
        "image" => include_str!("../lib/std/image.psh"),
        "json" => include_str!("../lib/std/json.psh"),
        "random" => include_str!("../lib/std/random.psh"),
        "sha256" => include_str!("../lib/std/sha256.psh"),
        _ => return None
    };

    Some(src)
}
