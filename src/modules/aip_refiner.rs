//! Defines the `refiner` module, used in the Lua engine.
//!
//! ## Lua documentation
//!
//! The `aip.refiner` module exposes the refiner content-processing workflow.
//!
//! - `aip.refiner.process(params: AipRefinrProcessParams) -> AipRefinrProcessOutput`
//!
//! The call waits for processing and publication to finish. Local source,
//! destination, and Sanitize prompt paths are resolved through `DirContext`.
//! Workflow configuration and execution failures are returned as Lua errors.
//! Successful results include final workflow and per-stage statistics.
//! Per-item summaries include each selected stage's status and artifact metadata.

use crate::modules::{DirContext, DirPolicyError};
use crate::registry::{HandlerError, HandlerResult};
use crate::{
	AiContext, AipFromLua, AipIntoLua, AipModule, AipOutput, AipParams, AipRegistryBuilder, ContextAccessError,
	HandlerCallContext, LuaExt, LuaJsonExt,
};
use mlua::{Lua, Table, Value};

// region:    --- Module

#[derive(Debug, Clone, Copy, Default)]
pub struct RefinrModule;

impl AipModule for RefinrModule {
	fn register(builder: AipRegistryBuilder) -> crate::Result<AipRegistryBuilder> {
		register(builder)
	}
}

fn register(builder: AipRegistryBuilder) -> crate::Result<AipRegistryBuilder> {
	Ok(builder.register_async("aip.refiner.process", aip_refiner_process_handler)?)
}

// endregion: --- Module

// region:    --- Types

/// Fetch representation selected by `aip.refiner.process`.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AipRefinrFetchFormat {
	/// Preserve fetched content in its raw representation.
	Raw,
	/// Select a compact representation.
	Slim,
	/// Select Markdown.
	Md,
}

/// Custom instructions for the Sanitize stage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AipRefinrSanitizePrompt {
	/// Read custom instructions from a policy-authorized file.
	File(String),
	/// Use inline custom instructions.
	Content(String),
}

#[derive(schemars::JsonSchema)]
#[schemars(untagged, inline)]
#[allow(dead_code)]
enum AipRefinrSanitizePromptSchema {
	Inline(String),
	File { file: String },
	Content { content: String },
}

/// A single include or exclude pattern, or a list of patterns.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum AipRefinrStringList {
	/// One pattern.
	Single(String),
	/// Multiple patterns.
	Multiple(Vec<String>),
}

impl AipRefinrStringList {
	fn into_vec(self) -> Vec<String> {
		match self {
			Self::Single(value) => vec![value],
			Self::Multiple(values) => values,
		}
	}
}

/// Parameters for `aip.refiner.process`.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AipRefinrProcessParams {
	/// Local path or HTTP(S) URL to fetch.
	pub source: String,
	/// Required output directory, resolved through the write policy.
	pub destination: String,
	/// Optional base directory for relative paths.
	pub base_dir: Option<String>,
	/// Whether to run Fetch. Defaults to `true`.
	pub fetch: Option<bool>,
	/// Patterns selecting content to include.
	pub include: Option<AipRefinrStringList>,
	/// Patterns selecting content to exclude.
	pub exclude: Option<AipRefinrStringList>,
	/// Representation used for fetched HTML content. Defaults to `md`.
	pub format: Option<AipRefinrFetchFormat>,
	/// Maximum web crawl depth. Defaults to `10`.
	pub max_depth: Option<usize>,
	/// Whether to discover `llms.txt` entries. Defaults to `true`.
	pub llms: Option<bool>,
	/// Whether to run the Sanitize stage. Defaults to `false`.
	pub sanitize: Option<bool>,
	/// Whether to run the Map stage. Defaults to `false`.
	pub map: Option<bool>,
	/// Fallback model for enabled AI stages.
	pub model: Option<String>,
	/// Model override for the Sanitize stage.
	pub sanitize_model: Option<String>,
	/// Model override for the Map stage.
	pub map_model: Option<String>,
	/// Custom instructions replacing the built-in Sanitize instructions.
	#[schemars(with = "Option<AipRefinrSanitizePromptSchema>")]
	pub sanitize_prompt: Option<AipRefinrSanitizePrompt>,
	/// Whether to reuse successful unchanged work. Defaults to `false`.
	pub resume: Option<bool>,
	/// Maximum parallel item processing. Defaults to `8`.
	pub concurrency: Option<usize>,
}

impl AipFromLua for AipRefinrProcessParams {
	fn from_lua(_lua: &Lua, value: Value) -> crate::Result<Self> {
		let table = params_table(&value)?;

		Ok(Self {
			source: required_string(table, "source")?,
			destination: required_string(table, "destination")?,
			base_dir: optional_string(table, "base_dir")?,
			fetch: optional_bool(table, "fetch")?,
			include: optional_string_list(table, "include")?,
			exclude: optional_string_list(table, "exclude")?,
			format: optional_fetch_format(table, "format")?,
			max_depth: optional_usize(table, "max_depth", 0)?,
			llms: optional_bool(table, "llms")?,
			sanitize: optional_bool(table, "sanitize")?,
			map: optional_bool(table, "map")?,
			model: optional_string(table, "model")?,
			sanitize_model: optional_string(table, "sanitize_model")?,
			map_model: optional_string(table, "map_model")?,
			sanitize_prompt: optional_sanitize_prompt(table)?,
			resume: optional_bool(table, "resume")?,
			concurrency: optional_usize(table, "concurrency", 1)?,
		})
	}
}

impl AipParams for AipRefinrProcessParams {}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct AipRefinrStats {
	/// Final Fetch statistics, or `None` if Fetch was not selected.
	pub fetch: Option<AipRefinrStageStats>,
	/// Final Sanitize statistics, or `None` if Sanitize was not selected.
	pub sanitize: Option<AipRefinrStageStats>,
	/// Final Map statistics, or `None` if Map was not selected.
	pub map: Option<AipRefinrStageStats>,
	/// Aggregated token usage across the workflow, when available.
	pub total_usage: Option<AipRefinrUsage>,
	/// Workflow start time in epoch microseconds.
	pub started_epoch_us: i64,
	/// Workflow end time in epoch microseconds.
	pub ended_epoch_us: i64,
	/// Elapsed workflow duration in milliseconds.
	pub duration_ms: u64,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct AipRefinrStageStats {
	/// Number of items accounted for by the stage outcomes.
	pub total_items: usize,
	/// Items processed during this workflow.
	pub completed: usize,
	/// Items whose results were reused.
	pub reused: usize,
	/// Items skipped without processing.
	pub skipped: usize,
	/// Items that failed processing.
	pub failed: usize,
	/// Items excluded from processing.
	pub excluded: usize,
	/// Token usage reported for this stage, when available.
	pub usage: Option<AipRefinrUsage>,
	/// Stage start time in epoch microseconds.
	pub started_epoch_us: i64,
	/// Stage end time in epoch microseconds.
	pub ended_epoch_us: i64,
	/// Elapsed stage duration in milliseconds.
	pub duration_ms: u64,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct AipRefinrUsage {
	/// Number of prompt tokens, when reported.
	pub prompt_tokens: Option<i64>,
	/// Number of completion tokens, when reported.
	pub completion_tokens: Option<i64>,
	/// Total number of tokens, when reported.
	pub total_tokens: Option<i64>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct AipRefinrItem {
	/// Source string associated with this item.
	pub source: String,
	/// Original path associated with this item.
	pub origin_path: String,
	/// Path used to identify this item relative to its source root.
	pub relative_path: String,
	/// Final Fetch-stage state, when recorded.
	pub fetch: Option<AipRefinrItemStage>,
	/// Final Sanitize-stage state, when recorded.
	pub sanitize: Option<AipRefinrItemStage>,
	/// Final Map-stage state, when recorded.
	pub map: Option<AipRefinrItemStage>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct AipRefinrItemStage {
	/// Lowercase snake_case lifecycle status.
	pub status: String,
	/// Path to the stage artifact. Fetch and Sanitize artifacts live under `.tmp-refinr/`.
	pub path: Option<String>,
	/// Error details when the stage failed.
	pub error: Option<String>,
	/// Token usage reported for the stage, when available.
	pub usage: Option<AipRefinrUsage>,
}

/// Result of a completed `aip.refiner.process` workflow.
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct AipRefinrProcessOutput {
	/// Root directory containing generated workflow artifacts.
	pub destination: String,
	/// Destination root containing the published final content.
	pub content_root: String,
	/// Durable workflow manifest, when one was written.
	pub manifest_path: Option<String>,
	/// Published content map, when mapping was selected.
	pub content_map_path: Option<String>,
	/// Final workflow and per-stage statistics.
	pub stats: AipRefinrStats,
	/// Final state of all registered workflow items.
	pub items: Vec<AipRefinrItem>,
	/// Non-fatal journal append errors.
	pub journal_errors: Vec<String>,
}

impl AipIntoLua for AipRefinrProcessOutput {
	fn into_lua(self, lua: &Lua) -> crate::Result<Value> {
		let value = serde_json::to_value(self).map_err(|error| crate::Error::custom(error.to_string()))?;
		Value::x_from_json_value(lua, value)
	}
}

impl AipOutput for AipRefinrProcessOutput {}

// endregion: --- Types

// region:    --- Handler

/// Runs Fetch, Sanitize, and Map in order and returns the completed workflow output.
async fn aip_refiner_process_handler(
	call_ctx: HandlerCallContext,
	params: AipRefinrProcessParams,
) -> HandlerResult<AipRefinrProcessOutput> {
	let paths = call_ctx
		.with::<DirContext, _>(|dir| resolve_refiner_paths(dir, &params))?
		.map_err(|error| HandlerError::custom(format!("[PATH_POLICY_DENIED] {error}")))?;

	let ai_options = match call_ctx.with::<AiContext, _>(|ai| resolve_ai_options(ai, &params)) {
		Ok(options) => Some(options),
		Err(ContextAccessError::MissingValue { .. } | ContextAccessError::ContextUnavailable) => None,
		Err(error) => return Err(error.into()),
	};
	let options = build_process_options(params, paths, ai_options);
	let handle = refinr::process_content(options).await.map_err(|error| {
		HandlerError::custom(format!("[REFINER_INVALID_CONFIG] aip.refiner.process failed. {error}"))
	})?;
	let output = handle.wait_output().await.map_err(|error| {
		HandlerError::custom(format!("[REFINER_PROCESS_FAILED] aip.refiner.process failed. {error}"))
	})?;

	Ok(project_process_output(output))
}

// endregion: --- Handler

// region:    --- Support

struct ResolvedRefinerPaths {
	source: String,
	destination: String,
	sanitize_prompt_file: Option<String>,
}

fn resolve_refiner_paths(
	dir: &DirContext,
	params: &AipRefinrProcessParams,
) -> Result<ResolvedRefinerPaths, DirPolicyError> {
	let source = if is_web_source(&params.source) {
		params.source.clone()
	} else {
		let resolved = if params.fetch.unwrap_or(true) {
			dir.resolve_read(&params.source, params.base_dir.as_deref())?
		} else {
			dir.resolve_read_target(&params.source, params.base_dir.as_deref())?
		};
		resolved.path().as_str().to_string()
	};

	let destination = dir
		.resolve_write(&params.destination, params.base_dir.as_deref())?
		.path()
		.as_str()
		.to_string();

	let sanitize_prompt_file = match params.sanitize_prompt.as_ref() {
		Some(AipRefinrSanitizePrompt::File(path)) => {
			Some(dir.resolve_read(path, params.base_dir.as_deref())?.path().as_str().to_string())
		}
		_ => None,
	};

	Ok(ResolvedRefinerPaths {
		source,
		destination,
		sanitize_prompt_file,
	})
}

fn is_web_source(source: &str) -> bool {
	source.get(..7).is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
		|| source.get(..8).is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

struct ResolvedAiOptions {
	genai_client: Option<genai::Client>,
	model: Option<String>,
	sanitize_model: Option<String>,
	map_model: Option<String>,
}

fn resolve_ai_options(ai: &AiContext, params: &AipRefinrProcessParams) -> ResolvedAiOptions {
	let sanitize_model = if params.sanitize.unwrap_or(false) {
		ai.resolve_model(params.sanitize_model.as_deref().or(params.model.as_deref()))
	} else {
		None
	};
	let map_model = if params.map.unwrap_or(false) {
		ai.resolve_model(params.map_model.as_deref().or(params.model.as_deref()))
	} else {
		None
	};

	ResolvedAiOptions {
		genai_client: ai.genai_client().cloned(),
		model: ai.resolve_model(params.model.as_deref()),
		sanitize_model,
		map_model,
	}
}

fn build_process_options(
	params: AipRefinrProcessParams,
	paths: ResolvedRefinerPaths,
	ai_options: Option<ResolvedAiOptions>,
) -> refinr::ProcessContentOptions {
	let ResolvedRefinerPaths {
		source,
		destination,
		sanitize_prompt_file,
	} = paths;
	let mut options = refinr::ProcessContentOptions::new(source).with_dest(destination);

	if let Some(fetch) = params.fetch {
		options = options.with_fetch(fetch);
	}
	if let Some(include) = params.include {
		options = options.with_include(include.into_vec());
	}
	if let Some(exclude) = params.exclude {
		options = options.with_exclude(exclude.into_vec());
	}
	if let Some(format) = params.format {
		options = options.with_format(match format {
			AipRefinrFetchFormat::Raw => refinr::FetchFormat::Raw,
			AipRefinrFetchFormat::Slim => refinr::FetchFormat::Slim,
			AipRefinrFetchFormat::Md => refinr::FetchFormat::Md,
		});
	}
	options = options.with_max_depth(params.max_depth.unwrap_or(10));
	if let Some(llms) = params.llms {
		options = options.with_llms(llms);
	}
	if let Some(sanitize) = params.sanitize {
		options = options.with_sanitize(sanitize);
	}
	if let Some(map) = params.map {
		options = options.with_map(map);
	}
	if let Some(model) = params.model {
		options = options.with_model(model);
	}
	if let Some(model) = params.sanitize_model {
		options = options.with_sanitize_model(model);
	}
	if let Some(model) = params.map_model {
		options = options.with_map_model(model);
	}
	match params.sanitize_prompt {
		Some(AipRefinrSanitizePrompt::File(_)) => {
			if let Some(path) = sanitize_prompt_file {
				options = options.with_sanitize_prompt(refinr::SanitizePrompt::file(path));
			}
		}
		Some(AipRefinrSanitizePrompt::Content(content)) => {
			options = options.with_sanitize_prompt(refinr::SanitizePrompt::content(content));
		}
		None => {}
	}
	if let Some(resume) = params.resume {
		options = options.with_resume(resume);
	}
	if let Some(concurrency) = params.concurrency {
		options = options.with_concurrency(concurrency);
	}
	if let Some(ai_options) = ai_options {
		if let Some(client) = ai_options.genai_client {
			options = options.with_genai_client(client);
		}
		if let Some(model) = ai_options.model {
			options = options.with_model(model);
		}
		if let Some(model) = ai_options.sanitize_model {
			options = options.with_sanitize_model(model);
		}
		if let Some(model) = ai_options.map_model {
			options = options.with_map_model(model);
		}
	}

	options
}

fn project_stats(stats: refinr::FinalStats) -> AipRefinrStats {
	let duration_ms = stats.duration().as_millis() as u64;
	AipRefinrStats {
		fetch: stats.fetch.map(project_stage_stats),
		sanitize: stats.sanitize.map(project_stage_stats),
		map: stats.map.map(project_stage_stats),
		total_usage: stats.total_usage.map(project_usage),
		started_epoch_us: stats.started_epoch_us,
		ended_epoch_us: stats.ended_epoch_us,
		duration_ms,
	}
}

fn project_stage_stats(stats: refinr::StageFinal) -> AipRefinrStageStats {
	let duration_ms = stats.duration().as_millis() as u64;
	AipRefinrStageStats {
		total_items: stats.total_items,
		completed: stats.completed,
		reused: stats.reused,
		skipped: stats.skipped,
		failed: stats.failed,
		excluded: stats.excluded,
		usage: stats.usage.map(project_usage),
		started_epoch_us: stats.started_epoch_us,
		ended_epoch_us: stats.ended_epoch_us,
		duration_ms,
	}
}

fn project_usage(usage: genai::chat::Usage) -> AipRefinrUsage {
	AipRefinrUsage {
		prompt_tokens: usage.prompt_tokens.map(i64::from),
		completion_tokens: usage.completion_tokens.map(i64::from),
		total_tokens: usage.total_tokens.map(i64::from),
	}
}

fn project_item(item: refinr::ItemState) -> AipRefinrItem {
	AipRefinrItem {
		source: item.source,
		origin_path: item.origin_path,
		relative_path: item.relative_path,
		fetch: item.fetch.map(project_item_stage),
		sanitize: item.sanitize.map(project_item_stage),
		map: item.map.map(project_item_stage),
	}
}

fn project_item_stage(stage: refinr::ItemStageState) -> AipRefinrItemStage {
	AipRefinrItemStage {
		status: item_status_name(&stage.status),
		path: stage.path.map(|path| path.as_str().to_string()),
		error: stage.error,
		usage: stage.usage.map(project_usage),
	}
}

fn item_status_name(status: &refinr::ItemStatus) -> String {
	let status = match status {
		refinr::ItemStatus::Pending => "pending",
		refinr::ItemStatus::Running => "running",
		refinr::ItemStatus::Completed => "completed",
		refinr::ItemStatus::Reused => "reused",
		refinr::ItemStatus::Skipped => "skipped",
		refinr::ItemStatus::Failed => "failed",
	};
	status.to_string()
}

fn project_process_output(output: refinr::ProcessContentOutput) -> AipRefinrProcessOutput {
	AipRefinrProcessOutput {
		destination: output.destination.as_str().to_string(),
		content_root: output.content_root.as_str().to_string(),
		manifest_path: output.manifest_path.map(|path| path.as_str().to_string()),
		content_map_path: output.content_map_path.map(|path| path.as_str().to_string()),
		stats: project_stats(output.stats),
		items: output.items.into_iter().map(project_item).collect(),
		journal_errors: output.journal_errors,
	}
}

fn params_table(value: &Value) -> crate::Result<&Table> {
	value.as_table().ok_or_else(|| {
		crate::Error::custom(format!(
			"Params expected to be a table, but was of type '{}'",
			value.type_name()
		))
	})
}

fn required_string(table: &Table, key: &str) -> crate::Result<String> {
	optional_string(table, key)?
		.ok_or_else(|| crate::Error::custom(format!("Missing required property '{key}' of type 'string'")))
}

fn optional_property(table: &Table, key: &str) -> crate::Result<Option<Value>> {
	Ok(table.x_try_get_value(key)?.filter(|value| !value.x_is_null()))
}

fn optional_string(table: &Table, key: &str) -> crate::Result<Option<String>> {
	let Some(value) = optional_property(table, key)? else {
		return Ok(None);
	};
	value
		.x_as_lua_str()
		.map(|value| Some(value.to_string()))
		.ok_or_else(|| type_mismatch_error(key, "string", &value))
}

fn optional_bool(table: &Table, key: &str) -> crate::Result<Option<bool>> {
	let Some(value) = optional_property(table, key)? else {
		return Ok(None);
	};
	value
		.x_as_bool()
		.map(Some)
		.ok_or_else(|| type_mismatch_error(key, "boolean", &value))
}

fn optional_string_list(table: &Table, key: &str) -> crate::Result<Option<AipRefinrStringList>> {
	let Some(value) = optional_property(table, key)? else {
		return Ok(None);
	};

	if let Some(value) = value.x_as_lua_str() {
		return Ok(Some(AipRefinrStringList::Single(value.to_string())));
	}

	let Some(values) = value.x_as_list() else {
		return Err(type_mismatch_error(key, "string or string[]", &value));
	};
	if values.is_empty() {
		return Err(crate::Error::custom(format!(
			"Property '{key}' must not be an empty list"
		)));
	}

	let values = values
		.into_iter()
		.map(|value| {
			value.x_as_lua_str().map(|value| value.to_string()).ok_or_else(|| {
				crate::Error::custom(format!(
					"Property '{key}' entries expected to be of type 'string', but got type '{}'",
					value.type_name()
				))
			})
		})
		.collect::<crate::Result<Vec<_>>>()?;

	Ok(Some(AipRefinrStringList::Multiple(values)))
}

fn optional_fetch_format(table: &Table, key: &str) -> crate::Result<Option<AipRefinrFetchFormat>> {
	let Some(value) = optional_property(table, key)? else {
		return Ok(None);
	};
	let Some(format) = value.x_as_lua_str() else {
		return Err(type_mismatch_error(key, "string", &value));
	};

	match format.as_ref() {
		"raw" => Ok(Some(AipRefinrFetchFormat::Raw)),
		"slim" => Ok(Some(AipRefinrFetchFormat::Slim)),
		"md" => Ok(Some(AipRefinrFetchFormat::Md)),
		_ => Err(crate::Error::custom(format!(
			"Property 'format' expected to be one of 'raw', 'slim', 'md', but was '{format}'"
		))),
	}
}

fn optional_usize(table: &Table, key: &str, minimum: usize) -> crate::Result<Option<usize>> {
	let Some(value) = optional_property(table, key)? else {
		return Ok(None);
	};
	let Value::Integer(value_integer) = value else {
		return Err(type_mismatch_error(key, "integer", &value));
	};
	if value_integer < minimum as i64 {
		return Err(crate::Error::custom(format!(
			"Property '{key}' must be greater than or equal to {minimum}"
		)));
	}
	usize::try_from(value_integer)
		.map(Some)
		.map_err(|_| crate::Error::custom(format!("Property '{key}' is too large")))
}

fn optional_sanitize_prompt(table: &Table) -> crate::Result<Option<AipRefinrSanitizePrompt>> {
	let Some(value) = optional_property(table, "sanitize_prompt")? else {
		return Ok(None);
	};

	match value {
		Value::String(content) => Ok(Some(AipRefinrSanitizePrompt::Content(content.to_string_lossy()))),
		Value::Table(prompt_table) => {
			let file = optional_prompt_string(&prompt_table, "file")?;
			let content = optional_prompt_string(&prompt_table, "content")?;
			match (file, content) {
				(Some(_), Some(_)) => Err(crate::Error::custom(
					"Property 'sanitize_prompt' table cannot contain both 'file' and 'content'",
				)),
				(Some(file), None) => Ok(Some(AipRefinrSanitizePrompt::File(file))),
				(None, Some(content)) => Ok(Some(AipRefinrSanitizePrompt::Content(content))),
				(None, None) => Err(crate::Error::custom(
					"Property 'sanitize_prompt' table must contain either 'file' or 'content'",
				)),
			}
		}
		other => Err(type_mismatch_error("sanitize_prompt", "string or table", &other)),
	}
}

fn optional_prompt_string(table: &Table, key: &str) -> crate::Result<Option<String>> {
	let Some(value) = optional_property(table, key)? else {
		return Ok(None);
	};
	value
		.x_as_lua_str()
		.map(|value| Some(value.to_string()))
		.ok_or_else(|| type_mismatch_error(&format!("sanitize_prompt.{key}"), "string", &value))
}

fn type_mismatch_error(key: &str, expected: &str, value: &Value) -> crate::Error {
	crate::Error::custom(format!(
		"Property '{key}' expected to be of type '{expected}', but was of type '{}'",
		value.type_name()
	))
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "aip_refiner_tests.rs"]
mod aip_refiner_tests;

// endregion: --- Tests
