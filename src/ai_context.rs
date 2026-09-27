use std::collections::HashMap;
use std::sync::Arc;

/// Execution-scoped configuration for AI-backed operations.
#[derive(Clone, Default)]
pub struct AiContext {
	genai_client: Option<genai::Client>,
	model_map: Option<Arc<HashMap<String, String>>>,
	default_model: Option<String>,
}

impl AiContext {
	/// Sets the GenAI client used by AI-backed operations.
	pub fn with_genai_client(mut self, client: genai::Client) -> Self {
		self.genai_client = Some(client);
		self
	}

	/// Sets the model alias-to-provider-name mapping.
	pub fn with_model_map(mut self, model_map: HashMap<String, String>) -> Self {
		self.model_map = Some(Arc::new(model_map));
		self
	}

	/// Sets the default model used when a request does not select one.
	pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
		self.default_model = Some(model.into());
		self
	}

	/// Returns the configured GenAI client, if present.
	pub fn genai_client(&self) -> Option<&genai::Client> {
		self.genai_client.as_ref()
	}

	/// Returns the configured model alias-to-provider-name mapping, if present.
	pub fn model_map(&self) -> Option<&HashMap<String, String>> {
		self.model_map.as_deref()
	}

	/// Resolves a requested model or the configured default through the model map.
	pub fn resolve_model(&self, requested: Option<&str>) -> Option<String> {
		let model = requested.or(self.default_model.as_deref())?;
		Some(
			self.model_map
				.as_deref()
				.and_then(|models| models.get(model))
				.cloned()
				.unwrap_or_else(|| model.to_owned()),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn resolves_requested_and_default_model_aliases() {
		let context = AiContext::default()
			.with_model_map(HashMap::from([
				("fast".to_string(), "provider/fast".to_string()),
				("default-alias".to_string(), "provider/default".to_string()),
			]))
			.with_default_model("default-alias");

		assert_eq!(context.resolve_model(Some("fast")).as_deref(), Some("provider/fast"));
		assert_eq!(
			context.resolve_model(Some("provider/literal")).as_deref(),
			Some("provider/literal")
		);
		assert_eq!(
			context.resolve_model(None).as_deref(),
			Some("provider/default")
		);
	}

	#[test]
	fn resolves_no_model_when_request_and_default_are_absent() {
		let context = AiContext::default().with_model_map(HashMap::new());

		assert_eq!(context.resolve_model(None), None);
	}
}
