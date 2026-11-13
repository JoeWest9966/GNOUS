//! Page-level plugin renderers.
//!
//! A plugin's manifest (see `registry`) decides which views it contributes to
//! the top bar; the matching module here decides how they look. Adding a view
//! means adding an entry to the manifest and a match arm in the module.

pub mod gnome_fixes;
pub mod openshell;
pub mod processes;
pub mod root_align;
pub mod startup_layouts;
