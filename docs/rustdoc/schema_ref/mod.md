# Schema reference helpers

This module provides lightweight, borrowed views over a [`schemars::Schema`](schemars::Schema).

[`SchemaRef`] exposes root-level schema information, object properties, the raw JSON schema value, and referenced `$defs` keys. [`SchemaPropRef`] exposes the name, required status, description, type, default value, and raw JSON value for one object property.

These types do not own or modify schema data. Their lifetimes are tied to the source schema, making them suitable for documentation generators and schema-driven presentation code.

## Example

```rust
use aiprog::schema_ref::SchemaRef;
use schemars::{schema_for, JsonSchema};

#[derive(JsonSchema)]
struct Input {
	name: String,
}

let schema = schema_for!(Input);
let schema_ref = SchemaRef::new(&schema);

for property in schema_ref.properties() {
	assert_eq!(property.name(), "name");
	assert!(property.is_required());
}
```
