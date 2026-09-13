//! Defines the `text` module, used in the lua engine.
//!
//! ---
//!
//! ## Lua documentation
//!
//! The `aip.text` module exposes text formatting and manipulation utilities.
//!
//! ### Functions
//!
//! - `aip.text.format_size(params: { size?: integer, lowest_unit?: string, trim?: boolean }) -> string | nil`
//! - `aip.text.trim(params: { text?: string, mode?: "all" | "start" | "end" }) -> string | nil`
//!
//! ---

#![allow(non_camel_case_types)]

use crate::base::text::format_pretty_size;
use crate::registry::HandlerResult;
use crate::{AipFromLua, AipIntoLua, AipParams, HandlerCallContext};
use crate::{AipModule, LuaExt};
use crate::{AipOutput, AipRegistryBuilder};
use aiprog_macros::aip_handler;
use aiprog_macros::register_handler;
use mlua::Lua;
use simple_fs::PrettySizeOptions;

#[derive(Debug, Clone, Copy, Default)]
pub struct TextModule;

impl AipModule for TextModule {
	fn register(mut builder: AipRegistryBuilder) -> crate::Result<AipRegistryBuilder> {
		register_handler!(builder, "aip.text.format_size", aip_text_format_size_handler)?;
		register_handler!(builder, "aip.text.trim", aip_text_trim_handler)?;
		Ok(builder)
	}
}

// region:    --- aip.text.format_size

/// Parameters for `aip.text.format_size`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde_with::skip_serializing_none]
pub struct AipTextFormatSizeParams {
	/// in bytes. If nil/absent, returns nil
	pub size: Option<u64>,

	/// Default: "B".  "B" | "KB" | "MB" | "GB" | "TB"
	pub lowest_unit: Option<String>,

	/// Default: false. (Do NOT set to 'true' when show in list or table, only when in a sentence)
	pub unpad: Option<bool>,
}

impl AipFromLua for AipTextFormatSizeParams {
	fn from_lua(_lua: &Lua, value: mlua::Value) -> crate::Result<Self> {
		let table = value.as_table().ok_or_else(|| {
			crate::Error::custom(format!(
				"Params expected to be a table, but was of type '{}'",
				value.type_name()
			))
		})?;

		let size = match table.x_try_get_value("size")? {
			Some(mlua::Value::Integer(i)) => Some(i.max(0) as u64),
			Some(mlua::Value::Number(n)) => Some(n.round().max(0.0) as u64),
			Some(mlua::Value::Nil) | None => None,
			Some(other) => {
				return Err(crate::Error::custom(format!(
					"Property 'size' expected to be a number, but was '{}'",
					other.type_name()
				)));
			}
		};

		let lowest_unit = table.x_try_get_string("lowest_unit")?;
		let unpad = table.x_try_get_bool("unpad")?;

		Ok(AipTextFormatSizeParams {
			size,
			lowest_unit,
			unpad,
		})
	}
}

impl AipParams for AipTextFormatSizeParams {}

/// Output type for `aip.text.format_size`.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct AipTextFormatSizeOutput(pub Option<String>);

impl AipIntoLua for AipTextFormatSizeOutput {
	fn into_lua(self, lua: &Lua) -> crate::Result<mlua::Value> {
		match self.0 {
			Some(s) => s.into_lua(lua),
			None => Ok(mlua::Value::Nil),
		}
	}
}

impl AipOutput for AipTextFormatSizeOutput {}

/// Formats a byte size into a human-readable 9-character aligned string.
/// Keep the padding for tables, lists, and aligned output.
#[aip_handler]
fn aip_text_format_size_handler(
	_call_ctx: HandlerCallContext,
	params: AipTextFormatSizeParams,
) -> HandlerResult<AipTextFormatSizeOutput> {
	let Some(size) = params.size else {
		return Ok(AipTextFormatSizeOutput(None));
	};

	let pretty_options = params.lowest_unit.as_deref().map(PrettySizeOptions::from);
	let pretty = format_pretty_size(size, pretty_options);
	let result = if params.unpad.unwrap_or_default() {
		pretty.trim().to_string()
	} else {
		pretty
	};

	Ok(AipTextFormatSizeOutput(Some(result)))
}

// endregion: --- aip.text.format_size

// region:    --- aip.text.trim

/// Trim mode for `aip.text.trim`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrimMode {
	#[default]
	All,
	Start,
	End,
}

/// Parameters for `aip.text.trim`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde_with::skip_serializing_none]
pub struct AipTextTrimParams {
	/// Text to trim. If nil or absent, returns nil.
	pub text: Option<String>,

	/// Trim mode: "all" (default), "start", or "end".
	pub mode: Option<TrimMode>,
}

impl AipFromLua for AipTextTrimParams {
	fn from_lua(_lua: &Lua, value: mlua::Value) -> crate::Result<Self> {
		let table = value.as_table().ok_or_else(|| {
			crate::Error::custom(format!(
				"Params expected to be a table, but was of type '{}'",
				value.type_name()
			))
		})?;

		let text = table.x_try_get_string("text")?;
		let mode = match table.x_try_get_string("mode")?.as_deref() {
			Some("start" | "left") => Some(TrimMode::Start),
			Some("end" | "right") => Some(TrimMode::End),
			Some("all" | "both") => Some(TrimMode::All),
			Some(other) => {
				return Err(crate::Error::custom(format!(
					"Invalid mode '{other}', expected 'all', 'start', or 'end'"
				)));
			}
			None => None,
		};

		Ok(AipTextTrimParams { text, mode })
	}
}

impl AipParams for AipTextTrimParams {}

/// Output type for `aip.text.trim`.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct AipTextTrimOutput(pub Option<String>);

impl AipIntoLua for AipTextTrimOutput {
	fn into_lua(self, lua: &Lua) -> crate::Result<mlua::Value> {
		match self.0 {
			Some(s) => s.into_lua(lua),
			None => Ok(mlua::Value::Nil),
		}
	}
}

impl AipOutput for AipTextTrimOutput {}

/// Trims whitespace from text according to the specified mode ("all", "start", or "end").
#[aip_handler]
fn aip_text_trim_handler(_call_ctx: HandlerCallContext, params: AipTextTrimParams) -> HandlerResult<AipTextTrimOutput> {
	let Some(text) = params.text else {
		return Ok(AipTextTrimOutput(None));
	};

	let mode = params.mode.unwrap_or_default();
	let trimmed = match mode {
		TrimMode::Start => text.trim_start().to_string(),
		TrimMode::End => text.trim_end().to_string(),
		TrimMode::All => text.trim().to_string(),
	};

	Ok(AipTextTrimOutput(Some(trimmed)))
}

// endregion: --- aip.text.trim

// region:    --- Tests

#[cfg(test)]
#[path = "aip_text_tests.rs"]
mod tests;

// endregion: --- Tests
