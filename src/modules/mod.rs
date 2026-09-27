use crate::{AipRegistry, AipRegistryBuilder, NativeFunctionSet};

// region:    --- Modules

mod aip_file;
mod aip_html;
mod aip_json;
mod aip_zmapr;
pub mod aip_md;
pub mod aip_text;
pub mod aip_time;
mod aip_web;

pub use aip_file::FileModule;
pub use aip_file::file_types::{AbsolutePathPolicy, DirContext, DirPolicyError, PathPolicy, ResolvedDirPath};

pub use aip_html::HtmlModule;
pub use aip_json::JsonModule;
pub use aip_zmapr::ZmaprModule;
pub use aip_md::MdModule;
pub use aip_text::TextModule;
pub use aip_time::TimeModule;
pub use aip_web::WebModule;

// endregion: --- Modules

// region:    --- Combined Registry

/// Build and return a combined `AipRegistry` containing all built-in modules
/// (`aip.json`, `aip.web`, `aip.file`).
///
/// The `aip.file` module uses a default `FileContext` (current directory).
pub fn init_registry() -> crate::Result<AipRegistry> {
	Ok(AipRegistryBuilder::default()
		.add_module(JsonModule)?
		.add_module(ZmaprModule)?
		.add_module(WebModule)?
		.add_module(FileModule)?
		.add_module(HtmlModule)?
		.add_module(MdModule)?
		.add_module(TextModule)?
		.add_module(TimeModule)?
		.build())
}

// endregion: --- Combined Registry

#[allow(dead_code)]
pub fn native_functions() -> NativeFunctionSet {
	WebModule.native_functions()
}

// region:    --- Tests

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

// endregion: --- Tests
