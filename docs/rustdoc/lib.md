# AIProg

AIProg is a Rust runtime for executing constrained Lua programs against explicitly registered, Rust-backed APIs.

The crate is designed for applications that want an AI system, or another program author, to orchestrate approved capabilities as a small Lua program instead of making individual tool calls.

## Primary entry points

- [`ScriptEngine`](crate::ScriptEngine) creates isolated script execution environments from an [`AipRegistry`](crate::AipRegistry). It is the preferred API for handlers that need execution-scoped state.

- [`AipRegistryBuilder`](crate::AipRegistryBuilder) registers synchronous and asynchronous handlers, combines modules, and builds an immutable [`AipRegistry`](crate::AipRegistry).

- [`AipModule`](crate::AipModule) provides composable registration for a group of handlers. Built-in modules include [`JsonModule`](crate::modules::JsonModule), [`WebModule`](crate::modules::WebModule), [`FileModule`](crate::modules::FileModule), and [`HtmlModule`](crate::modules::HtmlModule).

## Registry defaults

`AipRegistry::from_empty()` builds a registry with no handlers. It is equivalent to `AipRegistryBuilder::default().build()`. `AipRegistryBuilder::default()` creates an empty builder that you can register handlers or modules with before calling `build()`. The current API does not implement `Default` for `AipRegistry`. Use `AipRegistry::from_aip_modules()` to create a registry containing the built-in modules.

`RunningContext::default()` is different: it creates an empty store for execution-scoped values. It does not create a registry or add capabilities.

## Execution with context

`RunningContext::default()` creates an empty typed-value store. Use [`ScriptEngine`] when handlers need caller-provided capabilities or state. Insert values before execution, then recover the returned context from [`RunOutcome`]. If no [`DirContext`] is supplied, the engine adds a default one rooted at the current directory. It does not add an [`AiContext`], so insert one explicitly when handlers need it.

```rust
use aiprog::{AipRegistry, RunningContext, ScriptEngine};

let engine = ScriptEngine::builder()
	.with_registry(AipRegistry::from_empty())
	.build()?;

let outcome = engine
	.exec("return { message = 'hello' }", RunningContext::default())
	.await?;

let value = outcome.result?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Setting `DirContext` and `AiContext`

Insert these values into the context before calling `exec`. `DirContext::from_base_dir` is a convenient option when an existing directory should be the read and write root. Use `DirContext::new` with separate `PathPolicy` values when read and write access need different roots or permissions.

```rust
use aiprog::{AiContext, AipRegistry, DirContext, RunningContext, ScriptEngine};
use std::collections::HashMap;

let engine = ScriptEngine::builder()
	.with_registry(AipRegistry::from_empty())
	.build()?;

let mut context = RunningContext::default();
context.insert(DirContext::from_base_dir("./workspace")?);
context.insert(
	AiContext::default()
		.with_model_map(HashMap::from([(
			"fast".to_string(),
			"provider/fast".to_string(),
		)]))
		.with_default_model("fast"),
);

let outcome = engine
	.exec("return { message = 'hello' }", context)
	.await?;
let (result, _returned_context) = outcome.into_parts();
let value = result?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The directory passed to `DirContext::from_base_dir` must already exist. Add a GenAI client with `.with_genai_client(client)` when AI-backed handlers need one. The model map resolves aliases to provider model names, and the default model is used when a request does not select one.

Handlers receive a [`HandlerCallContext`] and can access typed values in the current [`RunningContext`]. Applications commonly insert capability policies, service clients, or request-specific state into the context before starting an engine.

## Registering handlers

A handler accepts a [`HandlerCallContext`], a strongly typed parameter value implementing [`AipParams`], and returns a [`HandlerResult`](crate::HandlerResult) containing an output type implementing [`AipOutput`].

Use the [`aip_handler`](crate::aip_handler) attribute and [`register_handler`](crate::register_handler) macro for generated handler metadata and registration support. For lower-level registration, use [`AipRegistryBuilder::register_sync`] or [`AipRegistryBuilder::register_async`].

## Filesystem capabilities

The built-in file module requires a [`DirContext`](crate::DirContext) in the running context. Construct it with separate read and write [`PathPolicy`](crate::PathPolicy) values. Each policy defines canonical allowed roots and whether absolute paths are permitted through [`AbsolutePathPolicy`](crate::AbsolutePathPolicy).

This explicit capability model prevents a script from obtaining filesystem access outside roots supplied by the host application.

## Error handling

Most public APIs return [`Result`](crate::Result), whose error type is [`Error`](crate::Error). Script engine startup and execution preserve ownership of the caller's context when possible through [`EngineError`](crate::EngineError).

Use [`RunOutcome::into_parts`](crate::RunOutcome::into_parts) when both the script result and recovered context need to be handled together.

## Schema inspection

[`SchemaRef`](crate::schema_ref::SchemaRef) and [`SchemaPropRef`](crate::schema_ref::SchemaPropRef) provide borrowed convenience views over `schemars` schemas. They are useful for consumers that generate documentation or UI from registered handler schemas.

## Feature organization

- [`registry`](crate::registry) contains handler registration, schemas, handler errors, and registry selection.
- [`schema_ref`](crate::schema_ref) contains read-only schema inspection helpers.
- [`modules`](crate::modules) exposes the built-in module marker types and filesystem policy types.

The Lua runtime's built-in functions and registered handlers are implementation details of the selected registry and modules. Rustdoc documents the Rust API used to configure and host that runtime.
