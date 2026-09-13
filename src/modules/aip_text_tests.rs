type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

use crate::_test_support::{eval_script, setup_lua_engine};
use crate::AipRegistryBuilder;
use crate::modules::TextModule;
use serde_json::json;

fn setup_text_engine() -> crate::Result<crate::ScriptEngine> {
	setup_lua_engine(|| Ok(AipRegistryBuilder::default().add_module(TextModule)?.build()))
}

#[tokio::test]
async fn test_lua_text_trim_default() -> Result<()> {
	// -- Setup & Fixtures
	let engine = setup_text_engine()?;

	// -- Exec & Check
	let res = eval_script(&engine, r#"return aip.text.trim({ text = "  hello world  " })"#).await?;
	assert_eq!(res, json!("hello world"));

	let res = eval_script(&engine, "return aip.text.trim({ text = \"\\t\\n  hello \\n\\t \" })").await?;
	assert_eq!(res, json!("hello"));

	let res = eval_script(&engine, r#"return aip.text.trim({ text = "hello" })"#).await?;
	assert_eq!(res, json!("hello"));

	Ok(())
}

#[tokio::test]
async fn test_lua_text_trim_modes() -> Result<()> {
	// -- Setup & Fixtures
	let engine = setup_text_engine()?;

	// -- Exec & Check: Start / Left
	let res = eval_script(
		&engine,
		r#"return aip.text.trim({ text = "  hello world  ", mode = "start" })"#,
	)
	.await?;
	assert_eq!(res, json!("hello world  "));

	let res = eval_script(
		&engine,
		r#"return aip.text.trim({ text = "  hello world  ", mode = "left" })"#,
	)
	.await?;
	assert_eq!(res, json!("hello world  "));

	// -- Exec & Check: End / Right
	let res = eval_script(
		&engine,
		r#"return aip.text.trim({ text = "  hello world  ", mode = "end" })"#,
	)
	.await?;
	assert_eq!(res, json!("  hello world"));

	let res = eval_script(
		&engine,
		r#"return aip.text.trim({ text = "  hello world  ", mode = "right" })"#,
	)
	.await?;
	assert_eq!(res, json!("  hello world"));

	// -- Exec & Check: All / Both
	let res = eval_script(
		&engine,
		r#"return aip.text.trim({ text = "  hello world  ", mode = "all" })"#,
	)
	.await?;
	assert_eq!(res, json!("hello world"));

	let res = eval_script(
		&engine,
		r#"return aip.text.trim({ text = "  hello world  ", mode = "both" })"#,
	)
	.await?;
	assert_eq!(res, json!("hello world"));

	Ok(())
}

#[tokio::test]
async fn test_lua_text_trim_nil_and_empty() -> Result<()> {
	// -- Setup & Fixtures
	let engine = setup_text_engine()?;

	// -- Exec & Check: Nil text
	let res = eval_script(&engine, r#"return aip.text.trim({ text = nil })"#).await?;
	assert_eq!(res, serde_json::Value::Null);

	// -- Exec & Check: Empty table
	let res = eval_script(&engine, r#"return aip.text.trim({})"#).await?;
	assert_eq!(res, serde_json::Value::Null);

	Ok(())
}

#[tokio::test]
async fn test_lua_text_trim_invalid_mode() -> Result<()> {
	// -- Setup & Fixtures
	let engine = setup_text_engine()?;

	// -- Exec & Check
	let res = eval_script(
		&engine,
		r#"return aip.text.trim({ text = "hello", mode = "invalid_mode" })"#,
	)
	.await;
	assert!(res.is_err(), "Expected error on invalid trim mode");

	Ok(())
}

#[tokio::test]
async fn test_lua_text_trim_invalid_params() -> Result<()> {
	// -- Setup & Fixtures
	let engine = setup_text_engine()?;

	// -- Exec & Check: String passed instead of table
	let res = eval_script(&engine, r#"return aip.text.trim("hello")"#).await;
	assert!(res.is_err(), "Expected error when params is not a table");

	Ok(())
}
