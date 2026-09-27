use crate::LuaExt;
use mlua::{Lua, LuaSerdeExt as _, Table, Value};

const JSON_ARRAY_MARKER: &str = "__aip_json_array";

/// Lua JSON conversion extension trait for Lua values.
///
/// This trait provides custom JSON ↔ Lua conversion methods that replace
/// the default `mlua::Lua::to_value()` / `mlua::LuaSerdeExt` integration.
///
/// ## Why not use the default serde-based conversion?
///
/// 1. **Null / Nil mapping** — `serde_json::Value::Null` is serialised as
///    an opaque `UserData` value by `mlua`’s default serde bridge, not as
///    Lua `nil`.  The methods in this trait explicitly handle `Null` and
///    produce Lua `nil`, which matches common Lua scripting expectations.
///
/// 1. **Null / Sentinel mapping** — `serde_json::Value::Null` is serialised as
///    an opaque `UserData` value by `mlua Value::NULL`.  The methods in this trait explicitly handle `Null` and
///    produce `mlua::Value::NULL`, the standard Lua null sentinel
///    (a `LightUserData`).  This sentinel is recognised by `LuaExt::x_is_null`,
///    so callers can uniformly test for null values with `value.x_is_null()`.
///
/// 2. **Table ↔ Array heuristics** — When converting Lua tables to JSON,
///    the default `mlua` behaviour treats every table as a JSON object.
///    This trait inspects the table and, if it contains a contiguous
///    1..n integer-keyed sequence, emits a JSON array instead.  This
///    aligns with the common Lua convention where lists are represented
///    as tables with consecutive integer keys.
///
/// 3. **Explicit nil-to-None semantics** — `to_json_value` returns
///    `Result<Option<serde_json::Value>>`, allowing callers to
///    distinguish "not present" (`nil` → `None`) from a genuine JSON
///    `null`.  The default serde conversion does not provide this
///    distinction.
///
/// 4. **`LuaExt` integration** — The trait has a supertrait bound `LuaExt`,
///    so all query helpers from `LuaExt` (including `x_is_null`) are
///    automatically available on any value that supports JSON conversion.
///
/// 5. **Fallback key handling** — For tables that are not strict arrays,
///    keys are stringified according to their Lua type (e.g., integer
///    keys become their decimal string representation).  This deterministic
///    behaviour is superior to the opaque `map_key` callback often required
///    with `mlua`’s serde wrapper.
///
/// Implementors automatically get `LuaExt` query helpers (e.g., `x_as_list`, `x_get_string`)
/// because of the `: LuaExt` supertrait bound.
#[allow(dead_code)]
pub trait LuaJsonExt: LuaExt {
	/// Convert a `serde_json::Value` into a `mlua::Value`.
	fn x_from_json_value(lua: &Lua, val: serde_json::Value) -> crate::Result<Value>;

	/// Convert an iterable of JSON values into a Lua table (list) as a `mlua::Value`.
	/// The table uses 1-based integer keys to form a Lua list.
	fn x_from_json_values<I>(lua: &Lua, values: I) -> crate::Result<Value>
	where
		I: IntoIterator<Item = serde_json::Value>;

	/// Convert this Lua value into a JSON value.
	///
	/// - Returns `Ok(None)` when the value is `nil`.
	/// - Returns `Ok(Some(json))` for convertible types (booleans, numbers, strings, tables, etc.).
	/// - Tables are converted to JSON arrays (if contiguous 1..n integer keys) or objects (stringified keys).
	/// - Returns `Err` for unsupported Lua types (function, userdata, thread, error, …).
	fn x_to_json_value(&self) -> crate::Result<Option<serde_json::Value>>;

	/// If this Lua value is a table/list, convert its elements to JSON values.
	///
	/// - Returns `Ok(None)` when the value is `nil` or not a table.
	/// - Returns `Ok(Some(vec))` when the value is a table; the vector contains the JSON
	///   representation of each element (using `to_json_value`).
	/// - Returns `Err` if any element cannot be converted.
	fn x_to_json_values(&self) -> crate::Result<Option<Vec<serde_json::Value>>>;
}

impl LuaJsonExt for Value {
	fn x_from_json_value(lua: &Lua, val: serde_json::Value) -> crate::Result<Value> {
		// Map serde `Null` to `mlua::Value::NULL` — the null sentinel
		// that is compatible with `LuaExt::x_is_null`.
		match val {
			serde_json::Value::Null => Ok(Value::NULL),
			serde_json::Value::Array(values) => {
				let table = lua.create_table()?;
				if values.is_empty() {
					// Lua tables have no intrinsic empty-array/object distinction.
					let metatable = lua.create_table()?;
					metatable.set(JSON_ARRAY_MARKER, true)?;
					table.set_metatable(Some(metatable))?;
				}
				for (index, value) in values.into_iter().enumerate() {
					table.set(index + 1, Self::x_from_json_value(lua, value)?)?;
				}
				Ok(Value::Table(table))
			}
			serde_json::Value::Object(values) => {
				let table = lua.create_table()?;
				for (key, value) in values {
					table.set(key, Self::x_from_json_value(lua, value)?)?;
				}
				Ok(Value::Table(table))
			}
			other => Ok(lua.to_value(&other)?),
		}
	}

	fn x_from_json_values<I>(lua: &Lua, values: I) -> crate::Result<Value>
	where
		I: IntoIterator<Item = serde_json::Value>,
	{
		let table = lua.create_table()?;
		for (i, v) in values.into_iter().enumerate() {
			let lua_val = Self::x_from_json_value(lua, v)?;
			table.set(i + 1, lua_val)?;
		}
		Ok(Value::Table(table))
	}

	fn x_to_json_value(&self) -> crate::Result<Option<serde_json::Value>> {
		fn number_from_f64(v: f64) -> crate::Result<serde_json::Number> {
			serde_json::Number::from_f64(v).ok_or(crate::Error::custom(
				"Cannot convert non-finite Lua number to JSON (NaN or Infinity)",
			))
		}

		fn convert_table(table: mlua::Table) -> crate::Result<serde_json::Value> {
			let marked_empty_array = table
				.metatable()
				.is_some_and(|metatable| metatable.get::<bool>(JSON_ARRAY_MARKER).unwrap_or(false));
			if marked_empty_array {
				let marked_table = table.clone();
				let mut pairs = marked_table.pairs::<Value, Value>();
				if pairs.next().is_none() {
					return Ok(serde_json::Value::Array(Vec::new()));
				}
			}

			// Try to treat as an array (1..n contiguous integer keys, no gaps)
			let mut max_idx: usize = 0;
			let mut numeric_only = true;

			for pair in table.clone().pairs::<mlua::Value, mlua::Value>() {
				let (k, _) = pair?;
				match k {
					Value::Integer(i) if i > 0 => {
						let i = i as usize;
						if i > max_idx {
							max_idx = i;
						}
					}
					Value::Number(n) if n.is_finite() && n.fract() == 0.0 && n > 0.0 => {
						let i = n as usize;
						if i > max_idx {
							max_idx = i;
						}
					}
					_ => {
						numeric_only = false;
						break;
					}
				}
			}

			if numeric_only {
				if max_idx == 0 {
					return Ok(serde_json::Value::Object(serde_json::Map::new()));
				}
				let mut vec: Vec<Option<serde_json::Value>> = vec![None; max_idx];
				for pair in table.clone().pairs::<mlua::Value, mlua::Value>() {
					let (k, v) = pair?;
					let idx_opt = match k {
						Value::Integer(i) if i > 0 => Some(i as usize),
						Value::Number(n) if n.is_finite() && n.fract() == 0.0 && n > 0.0 => Some(n as usize),
						_ => None,
					};
					if let Some(idx) = idx_opt {
						if idx == 0 || idx > max_idx {
							numeric_only = false;
							break;
						}
						vec[idx - 1] = Some(v.x_to_json_value()?.unwrap_or(serde_json::Value::Null));
					} else {
						numeric_only = false;
						break;
					}
				}

				if numeric_only && vec.iter().all(|o| o.is_some()) {
					let arr = vec.into_iter().flatten().collect();
					return Ok(serde_json::Value::Array(arr));
				}
			}

			// Fallback: treat as object with stringified keys
			let mut map = serde_json::Map::new();
			for pair in table.pairs::<mlua::Value, mlua::Value>() {
				let (k, v) = pair?;
				let key = match k {
					Value::String(s) => s.to_str()?.to_string(),
					Value::Integer(i) => i.to_string(),
					Value::Number(n) => n.to_string(),
					Value::Boolean(b) => b.to_string(),
					other => {
						return Err(crate::Error::custom(format!(
							"Unsupported Lua table key type '{}' for JSON object",
							other.type_name()
						)));
					}
				};
				map.insert(key, v.x_to_json_value()?.unwrap_or(serde_json::Value::Null));
			}
			Ok(serde_json::Value::Object(map))
		}

		let lua_value = self.clone();
		match lua_value {
			Value::Nil => Ok(None),
			Value::Boolean(b) => Ok(Some(serde_json::Value::Bool(b))),
			Value::Integer(i) => Ok(Some(serde_json::Value::Number(serde_json::Number::from(i)))),
			Value::Number(n) => Ok(Some(serde_json::Value::Number(number_from_f64(n)?))),
			Value::String(s) => Ok(Some(serde_json::Value::String(s.to_str()?.to_string()))),
			Value::Table(t) => Ok(Some(convert_table(t)?)),
			Value::LightUserData(_ldata) => {
				// for now, still null (treat all LightUserData as null)
				Ok(Some(serde_json::Value::Null))
			}
			Value::Function(_) | Value::Thread(_) | Value::UserData(_) => Err(crate::Error::custom(
				"Cannot serialize Lua value to JSON: unsupported type (Function/LightUserData/UserData)",
			)),
			Value::Error(_) => Err(crate::Error::custom(
				"Cannot serialize Lua value to JSON: unsupported type (error)",
			)),
			Value::Other(_) => Err(crate::Error::custom(
				"Cannot serialize Lua value to JSON: unsupported type (other)",
			)),
		}
	}

	fn x_to_json_values(&self) -> crate::Result<Option<Vec<serde_json::Value>>> {
		let Some(list) = self.x_as_list() else {
			return Ok(None);
		};
		let json_list = list
			.into_iter()
			.map(|v| v.x_to_json_value().map(|opt| opt.unwrap_or(serde_json::Value::Null)))
			.collect::<crate::Result<Vec<_>>>()?;
		Ok(Some(json_list))
	}
}

impl LuaJsonExt for Table {
	fn x_from_json_value(lua: &Lua, val: serde_json::Value) -> crate::Result<Value> {
		Value::x_from_json_value(lua, val)
	}

	fn x_from_json_values<I>(lua: &Lua, values: I) -> crate::Result<Value>
	where
		I: IntoIterator<Item = serde_json::Value>,
	{
		Value::x_from_json_values(lua, values)
	}

	fn x_to_json_value(&self) -> crate::Result<Option<serde_json::Value>> {
		let val: Value = Value::Table(self.clone());
		val.x_to_json_value()
	}

	fn x_to_json_values(&self) -> crate::Result<Option<Vec<serde_json::Value>>> {
		let val: Value = Value::Table(self.clone());
		val.x_to_json_values()
	}
}
