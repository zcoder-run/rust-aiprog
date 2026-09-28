use crate::EngineError;
use crate::LuaErrorDetails;
use derive_more::{Display, From};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Display, From)]
#[display("{self:?}")]
pub enum Error {
	#[from(String, &String, &str)]
	Custom(String),

	#[display("Error: {_0}\n\tCause: {_1}")]
	CustomAndCause(String, String),

	#[display("{_0}")]
	#[from]
	LuaScript(LuaErrorDetails),

	// -- Engine
	#[from]
	Engine(EngineError),

	// -- Externals
	#[from]
	Io(std::io::Error),

	#[from]
	Json(serde_json::Error),

	#[from]
	Lua(mlua::Error),

	#[from]
	SimpleFs(simple_fs::Error),
}

impl From<crate::HandlerError> for Error {
	fn from(err: crate::HandlerError) -> Self {
		Error::Custom(err.to_string())
	}
}

impl From<crate::AipRegistryError> for Error {
	fn from(err: crate::AipRegistryError) -> Self {
		Error::Custom(err.to_string())
	}
}

// region:    --- Custom

impl Error {
	pub fn custom(val: impl Into<String>) -> Self {
		Self::Custom(val.into())
	}

	pub fn custom_from_err(err: impl std::error::Error) -> Self {
		Self::Custom(err.to_string())
	}

	/// Same as custom_and_cause (just a "cute" shorcut)
	pub fn cc(context: impl Into<String>, cause: impl std::fmt::Display) -> Self {
		Self::CustomAndCause(context.into(), cause.to_string())
	}
}

// endregion: --- Custom

// region:    --- Error Boilerplate

impl std::error::Error for Error {}

// endregion: --- Error Boilerplate

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>; // For tests.

	use super::*;

	/// Compile-time guard: anything wrapped into `mlua::Error::external` must be `Send + Sync + 'static`
	/// (required by the `mlua` `send` feature).
	fn assert_send_sync<T: Send + Sync + 'static>() {}

	#[test]
	fn test_error_types_are_send_sync() -> Result<()> {
		// -- Check
		assert_send_sync::<Error>();
		assert_send_sync::<crate::HandlerError>();
		assert_send_sync::<EngineError>();
		assert_send_sync::<LuaErrorDetails>();

		Ok(())
	}
}

// endregion: --- Tests
