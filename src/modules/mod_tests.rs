type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

use super::*;
use crate::{RegistrySelectionOptions, RunningContext, ScriptEngine, UnmatchedPatternPolicy};

#[test]
fn test_modules_add_module_independent_composition() -> Result<()> {
	// -- Setup & Fixtures
	let json_registry = AipRegistryBuilder::default().add_module(JsonModule)?.build();
	let web_registry = AipRegistryBuilder::default().add_module(WebModule)?.build();

	// -- Exec
	let combined = json_registry.to_builder().merge(web_registry)?.build();
	let paths = combined
		.list_registered_fns()
		.into_iter()
		.map(|registered| registered.path)
		.collect::<Vec<_>>();

	// -- Check
	assert!(paths.iter().any(|path| path == "aip.json.parse"));
	assert!(paths.iter().any(|path| path == "aip.web.get"));
	assert_eq!(json_registry.list_registered_fns().len(), 3);

	Ok(())
}

#[test]
fn test_modules_select_changes_exposed_surface() -> Result<()> {
	// -- Setup & Fixtures
	let registry = init_registry()?;

	// -- Exec
	let selected = registry.select(
		["aip.json.*"],
		RegistrySelectionOptions {
			unmatched_patterns: UnmatchedPatternPolicy::Error,
		},
	)?;
	let paths = selected
		.list_registered_fns()
		.into_iter()
		.map(|registered| registered.path)
		.collect::<Vec<_>>();

	// -- Check
	assert!(!paths.is_empty());
	assert!(paths.iter().all(|path| path.starts_with("aip.json.")));

	Ok(())
}

#[test]
fn test_modules_init_registry_contains_md_module() -> Result<()> {
	// -- Exec
	let registry = init_registry()?;
	let paths = registry
		.list_registered_fns()
		.into_iter()
		.map(|registered| registered.path)
		.collect::<Vec<_>>();

	// -- Check
	assert!(paths.iter().any(|path| path == "aip.md.make_table"));

	Ok(())
}

#[test]
fn test_modules_init_registry_contains_time_module() -> Result<()> {
	// -- Exec
	let registry = init_registry()?;
	let paths = registry
		.list_registered_fns()
		.into_iter()
		.map(|registered| registered.path)
		.collect::<Vec<_>>();

	// -- Check
	assert!(paths.iter().any(|path| path == "aip.time.now"));
	assert!(paths.iter().any(|path| path == "aip.time.parse"));

	Ok(())
}

#[test]
fn test_modules_init_registry_contains_refiner_module() -> Result<()> {
	// -- Exec
	let registry = init_registry()?;
	let paths = registry
		.list_registered_fns()
		.into_iter()
		.map(|registered| registered.path)
		.collect::<Vec<_>>();

	// -- Check
	assert!(paths.iter().any(|path| path == "aip.refiner.process"));

	Ok(())
}

#[tokio::test]
async fn test_modules_web_native_functions_install_constants() -> Result<()> {
	// -- Setup & Fixtures
	let registry = AipRegistryBuilder::default().add_module(WebModule)?.build();
	let template = ScriptEngine::builder()
		.with_registry(registry)
		.with_native_functions(WebModule.native_functions())
		.build()?;

	// -- Exec
	let outcome = template
		.exec(
			"return { aip.web.UA_AIPROG, aip.web.UA_BROWSER }",
			RunningContext::default(),
		)
		.await?;
	let value = outcome.result?;

	// -- Check
	assert_eq!(value[0], "aiprog");
	assert!(value[1].as_str().is_some_and(|value| value.contains("Mozilla/5.0")));

	Ok(())
}
