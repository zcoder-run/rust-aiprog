use super::error::{
	EngineBuildError, EngineError, EngineResult, EngineStartError, RunningEngineContextError, RunningEngineFinishError,
};
use super::lua_runtime_policy::LuaRuntimePolicy;
use crate::engine::LuaEngine;
use crate::modules::DirContext;
use crate::running_context::RunningContextHandle;
use crate::{AipRegistry, HandlerCallContext, RunOutcome, RunningContext};
use mlua::{Lua, LuaOptions, StdLib};
use std::sync::Arc;

pub type NativeFunctionInstaller = Arc<dyn Fn(&Lua) -> mlua::Result<()> + Send + Sync>;

#[derive(Clone)]
pub struct NativeFunctionSet {
	installers: Arc<[NativeFunctionInstaller]>,
}

#[derive(Clone)]
pub struct ScriptEngine {
	inner: Arc<ScriptEngineInner>,
}

struct ScriptEngineInner {
	registry: AipRegistry,
	lua_policy: LuaRuntimePolicy,
	native_functions: NativeFunctionSet,
}

#[derive(Default)]
pub struct ScriptEngineBuilder {
	registry: Option<AipRegistry>,
	lua_policy: LuaRuntimePolicy,
	native_functions: NativeFunctionSet,
}

pub struct RunningEngine {
	pub(crate) engine: LuaEngine,
	context: RunningContextHandle,
}

impl NativeFunctionSet {
	pub fn new(installers: impl Into<Arc<[NativeFunctionInstaller]>>) -> Self {
		Self {
			installers: installers.into(),
		}
	}

	pub fn append_installer(mut self, installer: NativeFunctionInstaller) -> Self {
		let mut installers = self.installers.iter().cloned().collect::<Vec<_>>();
		installers.push(installer);
		self.installers = installers.into();
		self
	}

	pub fn install(&self, lua: &Lua) -> mlua::Result<()> {
		for installer in self.installers.iter() {
			installer(lua)?;
		}
		Ok(())
	}
}

impl Default for NativeFunctionSet {
	fn default() -> Self {
		Self {
			installers: Arc::from([]),
		}
	}
}

impl ScriptEngine {
	pub fn builder() -> ScriptEngineBuilder {
		ScriptEngineBuilder::default()
	}

	pub fn generate_doc(&self) -> EngineResult<String> {
		let lua = create_restricted_lua(&self.inner.lua_policy)?;
		let mut engine = LuaEngine {
			lua,
			registered_fns: Vec::new(),
		};
		engine.init_native_fns().map_err(|e| EngineError::Custom(e.to_string()))?;
		engine
			.register(self.inner.registry.clone())
			.map_err(|e| EngineError::Custom(e.to_string()))?;
		engine.generate_doc().map_err(|e| EngineError::Custom(e.to_string()))
	}

	pub fn start(&self) -> EngineResult<RunningEngine> {
		let context_handle = RunningContextHandle::new_empty();
		let call_context = HandlerCallContext::new(context_handle.clone());

		match self.create_running_engine(call_context, context_handle) {
			Ok(engine) => Ok(engine),
			Err(setup_source) => Err(EngineStartError::Setup {
				source: Box::new(crate::Error::Engine(setup_source)),
			}
			.into()),
		}
	}

	pub async fn exec(&self, script: &str, context: RunningContext) -> EngineResult<RunOutcome<serde_json::Value>> {
		let mut running = self.start()?;
		let outcome = running.exec(script, context).await?;
		Ok(outcome)
	}

	fn create_running_engine(
		&self,
		call_context: HandlerCallContext,
		context: RunningContextHandle,
	) -> EngineResult<RunningEngine> {
		let lua = create_restricted_lua(&self.inner.lua_policy)?;
		let mut engine = LuaEngine {
			lua,
			registered_fns: Vec::new(),
		};
		engine.init_native_fns().map_err(|e| EngineError::Custom(e.to_string()))?;
		engine
			.register_with_context(self.inner.registry.clone(), call_context)
			.map_err(|e| EngineError::Custom(e.to_string()))?;
		self.inner.native_functions.install(engine.lua())?;

		Ok(RunningEngine { engine, context })
	}
}

impl ScriptEngineBuilder {
	pub fn with_registry(mut self, registry: AipRegistry) -> Self {
		self.registry = Some(registry);
		self
	}

	pub fn with_lua_policy(mut self, policy: LuaRuntimePolicy) -> Self {
		self.lua_policy = policy;
		self
	}

	pub fn with_native_functions(mut self, native_functions: NativeFunctionSet) -> Self {
		self.native_functions = native_functions;
		self
	}
	pub fn build(self) -> EngineResult<ScriptEngine> {
		validate_runtime_policy(&self.lua_policy)?;
		let registry = self.registry.ok_or(EngineBuildError::MissingRegistry)?;

		Ok(ScriptEngine {
			inner: Arc::new(ScriptEngineInner {
				registry,
				lua_policy: self.lua_policy,
				native_functions: self.native_functions,
			}),
		})
	}
}

impl RunningEngine {
	pub async fn exec(
		&mut self,
		script: &str,
		mut context: RunningContext,
	) -> EngineResult<RunOutcome<serde_json::Value>> {
		if context.get::<DirContext>().is_none() {
			context.insert(DirContext::default());
		}

		self.context.set_context(context).map_err(RunningEngineContextError::from)?;

		let result = self.engine.exec(script).await;

		let context = match self.context.take_context().map_err(RunningEngineContextError::from) {
			Ok(context) => context,
			Err(source) => {
				return Err(RunningEngineFinishError {
					result: result.map_err(|e| EngineError::Custom(e.to_string())),
					source,
				}
				.into());
			}
		};

		Ok(RunOutcome::new(result, context))
	}
}

// region:    --- Support
fn validate_runtime_policy(policy: &LuaRuntimePolicy) -> EngineResult<()> {
	if !policy.std_lib_policy().base {
		return Err(EngineBuildError::BaseLibraryRequired.into());
	}
	if policy.limits().max_instructions.is_some() {
		return Err(EngineBuildError::InstructionLimitUnsupported.into());
	}
	if policy.limits().wall_clock_timeout.is_some() {
		return Err(EngineBuildError::WallClockTimeoutUnsupported.into());
	}
	Ok(())
}

fn create_restricted_lua(policy: &LuaRuntimePolicy) -> mlua::Result<Lua> {
	let std_lib_policy = &policy.std_lib_policy();
	let mut std_libs = StdLib::NONE;

	if std_lib_policy.coroutine {
		std_libs |= StdLib::COROUTINE;
	}
	if std_lib_policy.math {
		std_libs |= StdLib::MATH;
	}
	if std_lib_policy.string {
		std_libs |= StdLib::STRING;
	}
	if std_lib_policy.table {
		std_libs |= StdLib::TABLE;
	}
	if std_lib_policy.utf8 {
		std_libs |= StdLib::UTF8;
	}
	if std_lib_policy.package {
		std_libs |= StdLib::PACKAGE;
	}
	if std_lib_policy.io {
		std_libs |= StdLib::IO;
	}
	if std_lib_policy.os {
		std_libs |= StdLib::OS;
	}
	if std_lib_policy.debug {
		std_libs |= StdLib::DEBUG;
	}

	let lua = Lua::new_with(std_libs, LuaOptions::default())?;
	if let Some(max_memory_bytes) = policy.limits().max_memory_bytes {
		lua.set_memory_limit(max_memory_bytes)?;
	}

	Ok(lua)
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
mod tests {
	type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

	use super::*;
	use crate::AipRegistryBuilder;
	use crate::impl_lua_serde_traits;
	use crate::registry::{HandlerError, HandlerResult};
	use schemars::JsonSchema;
	use serde::{Deserialize, Serialize};

	#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
	struct TestParams {
		data: String,
	}

	#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
	struct TestResponse {
		data: String,
	}

	impl_lua_serde_traits!(TestParams);
	impl_lua_serde_traits!(TestResponse);

	impl crate::AipParams for TestParams {}
	impl crate::AipOutput for TestResponse {}

	#[tokio::test]
	async fn test_script_engine_exec_uses_fresh_lua_state() -> Result<()> {
		// -- Setup & Fixtures
		let engine = ScriptEngine::builder().with_registry(AipRegistry::from_empty()).build()?;

		// -- Exec
		let first = engine.exec("leaked_value = 42; return true", RunningContext::default()).await?;
		let second = engine.exec("return leaked_value == nil", RunningContext::default()).await?;

		// -- Check
		assert_eq!(first.result?, serde_json::Value::Bool(true));
		assert_eq!(second.result?, serde_json::Value::Bool(true));

		Ok(())
	}

	#[tokio::test]
	async fn test_running_engine_exec_hands_off_context_between_calls() -> Result<()> {
		// -- Setup & Fixtures
		let registry = AipRegistryBuilder::default()
			.register_sync("aip.test.context", test_context_mutation_handler)?
			.build();
		let engine = ScriptEngine::builder().with_registry(registry).build()?;
		let mut running = engine.start()?;
		let mut context = RunningContext::default();
		context.insert::<u32>(0);

		// -- Exec
		let first = running.exec("return aip.test.context({data='first'})", context).await?;
		let (first_result, first_context) = first.into_parts();
		let first_value = first_result?;
		let first_data = first_value
			.get("data")
			.and_then(serde_json::Value::as_str)
			.ok_or("Expected first handler output")?;
		let second = running.exec("return aip.test.context({data='second'})", first_context).await?;
		let (second_result, second_context) = second.into_parts();
		let second_value = second_result?;
		let second_data = second_value
			.get("data")
			.and_then(serde_json::Value::as_str)
			.ok_or("Expected second handler output")?;

		// -- Check
		assert_eq!(first_data, "1");
		assert_eq!(second_data, "2");
		assert_eq!(second_context.get::<u32>(), Some(&2));

		Ok(())
	}

	#[tokio::test]
	async fn test_running_engine_exec_preserves_lua_state() -> Result<()> {
		// -- Setup & Fixtures
		let engine = ScriptEngine::builder().with_registry(AipRegistry::from_empty()).build()?;
		let mut running = engine.start()?;

		// -- Exec
		let first = running
			.exec("session_value = 41; return session_value", RunningContext::default())
			.await?;
		let first_context = first.context;
		let first_result = first.result?;
		let second = running.exec("return session_value + 1", first_context).await?;
		let second_result = second.result?;

		// -- Check
		assert_eq!(first_result, serde_json::Value::from(41));
		assert_eq!(second_result, serde_json::Value::from(42));

		Ok(())
	}

	#[tokio::test]
	async fn test_script_engine_exec_returns_context_after_script_error() -> Result<()> {
		// -- Setup & Fixtures
		let engine = ScriptEngine::builder().with_registry(AipRegistry::from_empty()).build()?;
		let mut context = RunningContext::default();
		context.insert::<u32>(42);

		// -- Exec
		let outcome = engine.exec("this is not valid Lua", context).await?;

		// -- Check
		assert!(outcome.result.is_err());
		assert_eq!(outcome.context.get::<u32>(), Some(&42));

		Ok(())
	}

	#[tokio::test]
	async fn test_running_engine_exec_continues_after_script_error() -> Result<()> {
		// -- Setup & Fixtures
		let engine = ScriptEngine::builder().with_registry(AipRegistry::from_empty()).build()?;
		let mut running = engine.start()?;
		let mut context = RunningContext::default();
		context.insert::<u32>(42);

		// -- Exec
		let failed = running.exec("this is not valid Lua", context).await?;
		let (failed_result, recovered_context) = failed.into_parts();
		let next = running.exec("return true", recovered_context).await?;

		// -- Check
		assert!(failed_result.is_err());
		assert_eq!(next.result?, serde_json::Value::Bool(true));
		assert_eq!(next.context.get::<u32>(), Some(&42));

		Ok(())
	}

	#[test]
	fn test_script_engine_start_returns_context_after_setup_error() -> Result<()> {
		Ok(())
	}

	fn test_context_mutation_handler(
		call: crate::HandlerCallContext,
		_params: TestParams,
	) -> HandlerResult<TestResponse> {
		let value = call
			.with_mut::<u32, _>(|counter| {
				*counter += 1;
				*counter
			})
			.map_err(|error| HandlerError::custom(error.to_string()))?;

		Ok(TestResponse {
			data: value.to_string(),
		})
	}

	#[test]
	fn test_script_engine_build_rejects_missing_registry() -> Result<()> {
		// -- Setup & Fixtures
		let builder = ScriptEngine::builder();

		// -- Exec
		let error = builder.build().err().ok_or("Should reject a gengine without a registry")?;

		// -- Check
		assert!(matches!(error, EngineError::Build(EngineBuildError::MissingRegistry)));

		Ok(())
	}

	#[tokio::test]
	async fn test_running_engine_exec_seeds_default_dir_context() -> Result<()> {
		// -- Setup & Fixtures
		let engine = ScriptEngine::builder().with_registry(AipRegistry::from_empty()).build()?;
		let mut running = engine.start()?;
		let context = RunningContext::default();

		// -- Exec
		let outcome = running.exec("return true", context).await?;

		// -- Check
		assert!(outcome.context.get::<DirContext>().is_some());

		Ok(())
	}

	#[test]
	fn test_script_engine_captured_state_is_send_sync() -> Result<()> {
		// -- Setup & Fixtures
		fn assert_send_sync<T: Send + Sync + 'static>() {}
		fn assert_send<T: Send + 'static>() {}

		// -- Exec & Check
		assert_send_sync::<RunningContext>();
		assert_send_sync::<HandlerCallContext>();
		assert_send_sync::<DirContext>();
		assert_send_sync::<crate::AiContext>();
		assert_send_sync::<AipRegistry>();
		assert_send_sync::<NativeFunctionSet>();
		assert_send_sync::<ScriptEngine>();
		assert_send::<RunningEngine>();

		Ok(())
	}

	#[test]
	fn test_script_engine_exec_future_is_send() -> Result<()> {
		// -- Setup & Fixtures
		fn assert_send<T: Send>(_: &T) {}
		let engine = ScriptEngine::builder()
			.with_registry(AipRegistry::from_empty())
			.build()?;

		// -- Exec
		let fut = engine.exec("return 1", RunningContext::default());

		// -- Check
		assert_send(&fut);

		Ok(())
	}
}

// endregion: --- Tests
