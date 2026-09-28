use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mlua::{Lua, Value};
use schemars::Schema;

use super::registry_types::AipFnKind;
use crate::HandlerCallContext;

pub type LuaSyncClosure = Box<dyn Fn(&Lua, Value) -> mlua::Result<Value> + Send + Sync>;

pub type LuaAsyncClosure =
	Box<dyn Fn(Lua, Value) -> Pin<Box<dyn Future<Output = mlua::Result<Value>> + Send>> + Send + Sync>;

pub type HandlerFactory = Box<dyn Fn(HandlerCallContext) -> AipHandlerClosure + Send + Sync>;

#[doc(hidden)]
pub struct HandlerDefinition {
	pub path: String,
	pub kind: AipFnKind,
	pub params_schema: Schema,
	pub output_schema: Schema,
	pub error_schema: Schema,
	pub description: Option<String>,
	pub title: Option<String>,
	pub factory: HandlerFactory,
}

pub struct BoundRegistryEntry {
	pub definition: Arc<HandlerDefinition>,
	pub path: String,
	pub kind: AipFnKind,
	pub handler: AipHandlerClosure,
}

pub struct BoundRegistry {
	entries: Vec<BoundRegistryEntry>,
}

pub enum AipHandlerClosure {
	Sync(LuaSyncClosure),
	Async(LuaAsyncClosure),
}

impl HandlerDefinition {
	pub fn bind(self: &Arc<Self>, call_context: HandlerCallContext) -> BoundRegistryEntry {
		BoundRegistryEntry {
			definition: Arc::clone(self),
			path: self.path.clone(),
			kind: self.kind,
			handler: (self.factory)(call_context),
		}
	}
}

impl BoundRegistry {
	pub fn from_definitions(definitions: &[Arc<HandlerDefinition>], call_context: HandlerCallContext) -> Self {
		let entries = definitions
			.iter()
			.map(|definition| definition.bind(call_context.clone()))
			.collect();
		Self { entries }
	}
}

// region:    --- Iterator Implementations

impl IntoIterator for BoundRegistry {
	type Item = BoundRegistryEntry;
	type IntoIter = std::vec::IntoIter<BoundRegistryEntry>;

	fn into_iter(self) -> Self::IntoIter {
		self.entries.into_iter()
	}
}

// endregion: --- Iterator Implementations
