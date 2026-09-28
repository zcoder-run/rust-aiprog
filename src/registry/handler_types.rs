#![allow(dead_code)]

use super::HandlerResult;
use mlua::{Lua, Value};
use std::future::Future;
use std::pin::Pin;

/// The pinned `Send` future returned by a handler call, resolving to a normalized
/// response (`mlua::Value`) or a normalized `HandlerError`.
pub type PinFutureValue = Pin<Box<dyn Future<Output = HandlerResult<Value>> + Send>>;

/// The generic, Lua-agnostic handler trait, modeled on `rpc-router::Handler`.
///
/// Key points:
/// - A handler is a plain Rust function or closure taking a single typed `P`
///   (params) argument and returning a typed `Result<R, E>`.
/// - The trait operates on `mlua::Value` at its public boundary. Typed conversion
///   happens inside the handler implementation (params satisfy `AipParams`,
///   response satisfy `AipResponse`, error via `IntoHandlerError`).
/// - Both sync and async handler kinds are supported through the
///   `impl_handler!` macro implementations.
/// - Async handler futures must be `Send + 'static`, as required by the
///   `mlua` `send` feature.
/// - The handler layer now depends on `mlua` for the Lua value types.
///
/// Type parameters:
/// - `P` is the typed params (satisfies `AipParams`).
/// - `R` is the typed response (satisfies `AipResponse`).
/// - `M` is a marker type used to distinguish the sync and async
///   implementations during type resolution.
pub trait Handler<P, R, M>: Clone
where
	P: Send + Sync + 'static,
	R: crate::AipIntoLua + Send + Sync + 'static,
{
	/// The future type returned by calling this handler.
	type Future: Future<Output = HandlerResult<Value>> + 'static;

	/// Call the handler with a Lua state and a pre-converted params value, and
	/// return a future resolving to a Lua value response or a normalized error.
	fn call(self, lua: Lua, call_context: crate::HandlerCallContext, params: P) -> Self::Future;
}

// region:    --- Markers

/// Marker type for synchronous handler implementations.
pub struct SyncMarker;

/// Marker type for asynchronous handler implementations.
pub struct AsyncMarker;

// endregion: --- Markers
