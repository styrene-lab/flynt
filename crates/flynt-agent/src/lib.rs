// json!() macro expansion in extension.rs grew past the default 128-deep
// recursion limit when the canvas tool schemas landed.
#![recursion_limit = "512"]

pub mod extension;
pub mod forge_tools;
