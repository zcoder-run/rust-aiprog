type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

use super::*;
use crate::modules::{DirContext, RefinrModule};
use crate::{AipRegistryBuilder, RunningContext, ScriptEngine};
use tempfile::TempDir;

fn setup_engine() -> crate::Result<ScriptEngine> {
	let registry = AipRegistryBuilder::default().add_module(RefinrModule)?.build();
	Ok(ScriptEngine::builder().with_registry(registry).build()?)
}

fn setup_context(tmp: &TempDir) -> Result<RunningContext> {
	let root = simple_fs::SPath::from_std_path(tmp.path())?;
	let dir_context = DirContext::from_base_dir(root)?;
	let mut context = RunningContext::default();
	context.insert(dir_context);
	Ok(context)
}

async fn eval_script_error(engine: &ScriptEngine, script: &str, context: RunningContext) -> Result<String> {
	let outcome = engine.exec(script, context).await?;
	let error = outcome.result.err().ok_or("Expected script execution to fail")?;
	Ok(error.to_string())
}

#[tokio::test]
async fn test_aip_refiner_process_local_fetch_ok() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	let docs = tmp.path().join("docs");
	std::fs::create_dir_all(&docs)?;
	std::fs::write(docs.join("a.md"), "# A")?;
	std::fs::write(docs.join("b.md"), "# B")?;
	let engine = setup_engine()?;
	let context = setup_context(&tmp)?;
	let script = r#"
		return aip.refiner.process({
			source = "docs",
			destination = "docs-out"
		})
	"#;

	// -- Exec
	let outcome = engine.exec(script, context).await?;
	let output = outcome.result?;

	// -- Check
	assert!(output["content_root"].is_string());
	assert!(output["journal_errors"].is_array());
	assert!(
		output["stats"]["fetch"]["completed"]
			.as_u64()
			.is_some_and(|completed| completed >= 2)
	);
	assert!(output["stats"]["sanitize"].is_null());
	assert!(output["stats"]["map"].is_null());
	let items = output["items"].as_array().expect("items should be an array");
	assert_eq!(items.len(), 2);
	for item in items {
		assert_eq!(item["fetch"]["status"].as_str(), Some("completed"));
		assert!(
			item["relative_path"]
				.as_str()
				.is_some_and(|relative_path| !relative_path.is_empty())
		);
	}
	let started_epoch_us = output["stats"]["started_epoch_us"]
		.as_i64()
		.expect("stats.started_epoch_us should be an integer");
	let ended_epoch_us = output["stats"]["ended_epoch_us"]
		.as_i64()
		.expect("stats.ended_epoch_us should be an integer");
	assert!(ended_epoch_us >= started_epoch_us);
	Ok(())
}

#[tokio::test]
async fn test_aip_refiner_process_empty_lists_are_arrays() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	std::fs::create_dir_all(tmp.path().join("docs"))?;
	let engine = setup_engine()?;
	let context = setup_context(&tmp)?;

	// -- Exec
	let outcome = engine
		.exec(
			r#"
				return aip.refiner.process({
					source = "docs",
					destination = "docs-out"
				})
			"#,
			context,
		)
		.await?;
	let output = outcome.result?;

	// -- Check
	assert_eq!(output["items"].as_array().map(Vec::len), Some(0));
	assert_eq!(output["journal_errors"].as_array().map(Vec::len), Some(0));
	Ok(())
}

#[tokio::test]
async fn test_aip_refiner_process_fetch_disabled_rerun_ok() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	let docs = tmp.path().join("docs");
	std::fs::create_dir_all(&docs)?;
	std::fs::write(docs.join("a.md"), "# A")?;
	let engine = setup_engine()?;
	let first_context = setup_context(&tmp)?;
	let first_outcome = engine
		.exec(
			r#"
				return aip.refiner.process({
					source = "docs",
					destination = "docs-out"
				})
			"#,
			first_context,
		)
		.await?;
	let _first_output = first_outcome.result?;
	let second_context = setup_context(&tmp)?;

	// -- Exec
	let error = eval_script_error(
		&engine,
		r#"
			return aip.refiner.process({
				source = "docs",
				destination = "docs-out",
				fetch = false,
				sanitize = false,
				map = false
			})
		"#,
		second_context,
	)
	.await?;

	// -- Check
	assert!(error.contains("[REFINER_INVALID_CONFIG]"), "{error}");
	Ok(())
}

#[tokio::test]
async fn test_aip_refiner_process_missing_source_err() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	let engine = setup_engine()?;
	let context = setup_context(&tmp)?;

	// -- Exec
	let error = eval_script_error(
		&engine,
		r#"return aip.refiner.process({ destination = "docs-out" })"#,
		context,
	)
	.await?;

	// -- Check
	assert!(
		error.contains("Missing required property 'source' of type 'string'"),
		"{error}"
	);
	Ok(())
}

#[tokio::test]
async fn test_aip_refiner_process_missing_destination_err() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	let engine = setup_engine()?;
	let context = setup_context(&tmp)?;

	// -- Exec
	let error = eval_script_error(&engine, r#"return aip.refiner.process({ source = "docs" })"#, context).await?;

	// -- Check
	assert!(
		error.contains("Missing required property 'destination' of type 'string'"),
		"{error}"
	);
	Ok(())
}

#[tokio::test]
async fn test_aip_refiner_process_invalid_format_err() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	let engine = setup_engine()?;
	let context = setup_context(&tmp)?;

	// -- Exec
	let error = eval_script_error(
		&engine,
		r#"return aip.refiner.process({ source = "docs", destination = "out", format = "json" })"#,
		context,
	)
	.await?;

	// -- Check
	assert!(
		error.contains("Property 'format' expected to be one of 'raw', 'slim', 'md', but was 'json'"),
		"{error}"
	);
	Ok(())
}

#[tokio::test]
async fn test_aip_refiner_process_zero_concurrency_err() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	let engine = setup_engine()?;
	let context = setup_context(&tmp)?;

	// -- Exec
	let error = eval_script_error(
		&engine,
		r#"return aip.refiner.process({ source = "docs", destination = "out", concurrency = 0 })"#,
		context,
	)
	.await?;

	// -- Check
	assert!(
		error.contains("Property 'concurrency' must be greater than or equal to 1"),
		"{error}"
	);
	Ok(())
}

#[tokio::test]
async fn test_aip_refiner_process_sanitize_prompt_both_keys_err() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	let engine = setup_engine()?;
	let context = setup_context(&tmp)?;

	// -- Exec
	let error = eval_script_error(
		&engine,
		r#"
			return aip.refiner.process({
				source = "docs",
				destination = "out",
				sanitize_prompt = { file = "prompt.md", content = "instructions" }
			})
		"#,
		context,
	)
	.await?;

	// -- Check
	assert!(error.contains("cannot contain both 'file' and 'content'"), "{error}");
	Ok(())
}

#[tokio::test]
async fn test_aip_refiner_process_destination_outside_policy_err() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	std::fs::create_dir_all(tmp.path().join("docs"))?;
	let engine = setup_engine()?;
	let context = setup_context(&tmp)?;

	// -- Exec
	let error = eval_script_error(
		&engine,
		r#"return aip.refiner.process({ source = "docs", destination = "../outside" })"#,
		context,
	)
	.await?;

	// -- Check
	assert!(error.contains("[PATH_POLICY_DENIED]"), "{error}");
	Ok(())
}

#[tokio::test]
async fn test_aip_refiner_process_sanitize_without_model_err() -> Result<()> {
	// -- Setup & Fixtures
	let tmp = TempDir::new()?;
	std::fs::create_dir_all(tmp.path().join("docs"))?;
	let engine = setup_engine()?;
	let context = setup_context(&tmp)?;

	// -- Exec
	let error = eval_script_error(
		&engine,
		r#"
			return aip.refiner.process({
				source = "docs",
				destination = "out",
				sanitize = true
			})
		"#,
		context,
	)
	.await?;

	// -- Check
	assert!(error.contains("[REFINER_INVALID_CONFIG]"), "{error}");
	Ok(())
}

#[test]
fn test_build_process_options_preserves_defaults_and_applies_overrides() {
	// -- Setup & Fixtures
	let default_params = AipRefinrProcessParams {
		source: "docs".to_string(),
		destination: "out".to_string(),
		base_dir: None,
		fetch: None,
		include: None,
		exclude: None,
		format: None,
		max_depth: None,
		llms: None,
		sanitize: None,
		map: None,
		model: None,
		sanitize_model: None,
		map_model: None,
		sanitize_prompt: None,
		resume: None,
		concurrency: None,
	};
	let default_paths = ResolvedRefinerPaths {
		source: "docs".to_string(),
		destination: "out".to_string(),
		sanitize_prompt_file: None,
	};

	// -- Exec
	let defaults = build_process_options(default_params, default_paths, None);

	// -- Check
	assert_eq!(defaults.source, "docs");
	assert_eq!(defaults.destination.as_ref().map(|path| path.as_str()), Some("out"));
	assert!(defaults.fetch);
	assert!(defaults.include.is_empty());
	assert!(defaults.exclude.is_empty());
	assert!(matches!(defaults.format, refinr::FetchFormat::Md));
	assert_eq!(defaults.max_depth, 10);
	assert!(defaults.llms);
	assert!(!defaults.sanitize);
	assert!(!defaults.map);
	assert!(defaults.model.is_none());
	assert!(defaults.sanitize_model.is_none());
	assert!(defaults.map_model.is_none());
	assert!(defaults.sanitize_prompt.is_none());
	assert!(!defaults.resume);
	assert_eq!(defaults.concurrency, 8);

	let override_params = AipRefinrProcessParams {
		source: "unused".to_string(),
		destination: "unused".to_string(),
		base_dir: None,
		fetch: Some(false),
		include: Some(AipRefinrStringList::Multiple(vec!["**/*.md".to_string()])),
		exclude: Some(AipRefinrStringList::Single("**/draft/**".to_string())),
		format: Some(AipRefinrFetchFormat::Slim),
		max_depth: Some(2),
		llms: Some(false),
		sanitize: Some(true),
		map: Some(true),
		model: Some("gpt-6-luna".to_string()),
		sanitize_model: Some("sanitize-model".to_string()),
		map_model: Some("map-model".to_string()),
		sanitize_prompt: Some(AipRefinrSanitizePrompt::Content("instructions".to_string())),
		resume: Some(true),
		concurrency: Some(3),
	};
	let override_paths = ResolvedRefinerPaths {
		source: "resolved-source".to_string(),
		destination: "resolved-output".to_string(),
		sanitize_prompt_file: None,
	};

	// -- Exec
	let overrides = build_process_options(override_params, override_paths, None);

	// -- Check
	assert_eq!(overrides.source, "resolved-source");
	assert_eq!(
		overrides.destination.as_ref().map(|path| path.as_str()),
		Some("resolved-output")
	);
	assert!(!overrides.fetch);
	assert_eq!(overrides.include, vec!["**/*.md"]);
	assert_eq!(overrides.exclude, vec!["**/draft/**"]);
	assert!(matches!(overrides.format, refinr::FetchFormat::Slim));
	assert_eq!(overrides.max_depth, 2);
	assert!(!overrides.llms);
	assert!(overrides.sanitize);
	assert!(overrides.map);
	assert_eq!(overrides.model.as_deref(), Some("gpt-6-luna"));
	assert_eq!(overrides.sanitize_model.as_deref(), Some("sanitize-model"));
	assert_eq!(overrides.map_model.as_deref(), Some("map-model"));
	assert!(matches!(
		overrides.sanitize_prompt,
		Some(refinr::SanitizePrompt::Content(ref content)) if content == "instructions"
	));
	assert!(overrides.resume);
	assert_eq!(overrides.concurrency, 3);
}

#[test]
fn test_aip_refiner_sanitize_prompt_schema_matches_lua_forms() -> Result<()> {
	let params_schema = serde_json::to_value(schemars::schema_for!(AipRefinrProcessParams))?;
	let sanitize_prompt_schema = params_schema["properties"]["sanitize_prompt"].to_string();

	assert!(sanitize_prompt_schema.contains("\"type\":\"string\""));
	assert!(sanitize_prompt_schema.contains("\"file\""));
	assert!(sanitize_prompt_schema.contains("\"content\""));
	Ok(())
}

#[test]
fn test_aip_refiner_sanitize_prompt_accepts_lua_forms() -> Result<()> {
	let lua = Lua::new();

	let table = lua.load(r#"return { sanitize_prompt = "instructions" }"#).eval::<Table>()?;
	assert!(matches!(
		optional_sanitize_prompt(&table)?,
		Some(AipRefinrSanitizePrompt::Content(content)) if content == "instructions"
	));

	let table = lua
		.load(r#"return { sanitize_prompt = { file = "prompt.md" } }"#)
		.eval::<Table>()?;
	assert!(matches!(
		optional_sanitize_prompt(&table)?,
		Some(AipRefinrSanitizePrompt::File(path)) if path == "prompt.md"
	));

	let table = lua
		.load(r#"return { sanitize_prompt = { content = "instructions" } }"#)
		.eval::<Table>()?;
	assert!(matches!(
		optional_sanitize_prompt(&table)?,
		Some(AipRefinrSanitizePrompt::Content(content)) if content == "instructions"
	));

	Ok(())
}

#[test]
fn test_aip_refiner_sanitize_prompt_rejects_invalid_table_combinations() -> Result<()> {
	let lua = Lua::new();

	for script in [
		r#"return { sanitize_prompt = { file = "prompt.md", content = "instructions" } }"#,
		r#"return { sanitize_prompt = {} }"#,
	] {
		let table = lua.load(script).eval::<Table>()?;
		assert!(optional_sanitize_prompt(&table).is_err());
	}

	Ok(())
}

#[test]
fn test_build_process_options_resolves_ai_context_stage_models() {
	// -- Setup & Fixtures
	let ai_context = AiContext::default()
		.with_genai_client(genai::Client::new().expect("GenAI client construction should succeed"))
		.with_model_map(std::collections::HashMap::from([
			("sanitize-alias".to_string(), "provider/sanitize".to_string()),
			("request-alias".to_string(), "provider/request".to_string()),
			("default-alias".to_string(), "provider/default".to_string()),
		]))
		.with_default_model("default-alias");
	let params = AipRefinrProcessParams {
		source: "docs".to_string(),
		destination: "out".to_string(),
		base_dir: None,
		fetch: None,
		include: None,
		exclude: None,
		format: None,
		max_depth: None,
		llms: None,
		sanitize: Some(true),
		map: Some(true),
		model: Some("request-alias".to_string()),
		sanitize_model: Some("sanitize-alias".to_string()),
		map_model: None,
		sanitize_prompt: None,
		resume: None,
		concurrency: None,
	};
	let paths = ResolvedRefinerPaths {
		source: "docs".to_string(),
		destination: "out".to_string(),
		sanitize_prompt_file: None,
	};

	// -- Exec
	let ai_options = resolve_ai_options(&ai_context, &params);
	let options = build_process_options(params, paths, Some(ai_options));

	// -- Check
	assert!(options.genai_client.is_some());
	assert_eq!(options.model.as_deref(), Some("provider/request"));
	assert_eq!(options.sanitize_model.as_deref(), Some("provider/sanitize"));
	assert_eq!(options.map_model.as_deref(), Some("provider/request"));
}

#[test]
fn test_build_process_options_uses_ai_context_default_model() {
	// -- Setup & Fixtures
	let ai_context = AiContext::default()
		.with_model_map(std::collections::HashMap::from([(
			"default-alias".to_string(),
			"provider/default".to_string(),
		)]))
		.with_default_model("default-alias");
	let params = AipRefinrProcessParams {
		source: "docs".to_string(),
		destination: "out".to_string(),
		base_dir: None,
		fetch: None,
		include: None,
		exclude: None,
		format: None,
		max_depth: None,
		llms: None,
		sanitize: Some(true),
		map: Some(true),
		model: None,
		sanitize_model: None,
		map_model: None,
		sanitize_prompt: None,
		resume: None,
		concurrency: None,
	};
	let paths = ResolvedRefinerPaths {
		source: "docs".to_string(),
		destination: "out".to_string(),
		sanitize_prompt_file: None,
	};

	// -- Exec
	let ai_options = resolve_ai_options(&ai_context, &params);
	let options = build_process_options(params, paths, Some(ai_options));

	// -- Check
	assert_eq!(options.model.as_deref(), Some("provider/default"));
	assert!(options.genai_client.is_none());
	assert_eq!(options.sanitize_model.as_deref(), Some("provider/default"));
	assert_eq!(options.map_model.as_deref(), Some("provider/default"));
}
