use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemFn;

pub fn aip_handler_attr(_attr: TokenStream, item: TokenStream) -> TokenStream {
	let input = match syn::parse2::<ItemFn>(item) {
		Ok(v) => v,
		Err(e) => return e.to_compile_error(),
	};

	// Extract doc comment metadata.
	let (title, desc) = extract_title_desc(&input.attrs);

	let is_async = input.sig.asyncness.is_some();
	// All handlers (sync and async) must have exactly one typed parameter.
	// A mandatory HandlerCallContext parameter precedes the typed params.
	{
		let param_count = input
			.sig
			.inputs
			.iter()
			.filter(|arg| matches!(arg, syn::FnArg::Typed(_)))
			.count();
		if param_count != 2 {
			return syn::Error::new_spanned(
				&input.sig,
				"handler function must have exactly two parameters: HandlerCallContext and the typed params.",
			)
			.to_compile_error();
		}
	}

	// Keep the original function unchanged, but remove the #[aip_handler] attribute.
	let mut output_fn = input.clone();
	output_fn.attrs = input
		.attrs
		.iter()
		.filter(|attr| !attr.path().is_ident("aip_handler"))
		.cloned()
		.collect();

	// Extract trait associated types.
	let original_ident = input.sig.ident.clone();
	let params_ty = match get_params_ty(&input.sig) {
		Ok(params_ty) => params_ty,
		Err(error) => return error.to_compile_error(),
	};
	let output_ty = match get_output_inner_type(&input.sig) {
		Ok(output_ty) => output_ty,
		Err(error) => return error.to_compile_error(),
	};

	let marker = if is_async {
		quote! { ::aiprog::registry::handler_types::AsyncMarker }
	} else {
		quote! { ::aiprog::registry::handler_types::SyncMarker }
	};

	let handler_struct_ident = syn::Ident::new(&format!("__AiprogHandler_{}", original_ident), original_ident.span());

	let desc_owned_tokens = if let Some(s) = &desc {
		let lit = syn::LitStr::new(s, original_ident.span());
		quote! { ::core::option::Option::Some(#lit.to_string()) }
	} else {
		quote! { ::core::option::Option::None }
	};
	let title_owned_tokens = if let Some(s) = &title {
		let lit = syn::LitStr::new(s, original_ident.span());
		quote! { ::core::option::Option::Some(#lit.to_string()) }
	} else {
		quote! { ::core::option::Option::None }
	};

	let meta_fn_ident = syn::Ident::new(&format!("__aiprog_meta_{}", original_ident), original_ident.span());
	let meta_fn = quote! {
		fn #meta_fn_ident() -> ::aiprog::registry::AipHandlerMeta {
			::aiprog::registry::AipHandlerMeta {
				description: #desc_owned_tokens,
				title: #title_owned_tokens,
			}
		}
	};

	let struct_def = quote! {
		#[allow(non_camel_case_types)]
		struct #handler_struct_ident;
	};

	let impl_block = if is_async {
		quote! {
			impl ::aiprog::registry::AipHandler for #handler_struct_ident {
				type Marker = #marker;
				type Params = #params_ty;
				type Output = #output_ty;

				fn handler_meta() -> ::aiprog::registry::AipHandlerMeta {
					#meta_fn_ident()
				}

				fn create_definition(path: &str) -> ::aiprog::registry::registry_internal::HandlerDefinition {
					let params_schema = schemars::schema_for!(#params_ty);
					let output_schema = schemars::schema_for!(#output_ty);
					let error_schema = schemars::schema_for!(::aiprog::HandlerError);

					// Compile-time guard: async handler futures must be `Send + 'static` (required by mlua `send`).
					fn __aiprog_assert_send_future<F>(future: F) -> F
					where
						F: ::core::future::Future + ::core::marker::Send + 'static,
					{
						future
					}

					let factory: ::aiprog::registry::registry_internal::HandlerFactory = Box::new(|call_context| {
						let closure: ::aiprog::registry::registry_internal::LuaAsyncClosure = Box::new(move |lua: mlua::Lua, value: mlua::Value| {
							let call_context = call_context.clone();
							let params = match <#params_ty as ::aiprog::AipFromLua>::from_lua(&lua, value)
								.map_err(|e| mlua::Error::ExternalError(::std::sync::Arc::new(e))) {
								Ok(p) => p,
								Err(e) => return Box::pin(async move { Err(e) }),
							};
							let handler_fut = __aiprog_assert_send_future(#original_ident(call_context, params));

							Box::pin(async move {
								match handler_fut.await {
									Ok(output) => <#output_ty as ::aiprog::AipIntoLua>::into_lua(output, &lua)
										.map_err(|e| mlua::Error::ExternalError(::std::sync::Arc::new(e))),
									Err(e) => Err(e.into_lua_error()),
								}
							})
						});
						::aiprog::registry::registry_internal::AipHandlerClosure::Async(closure)
					});

					let meta = Self::handler_meta();
					::aiprog::registry::registry_internal::HandlerDefinition {
						path: path.to_string(),
						kind: ::aiprog::registry::AipFnKind::Async,
						params_schema,
						output_schema,
						error_schema,
						description: meta.description,
						title: meta.title,
						factory,
					}
				}
			}
		}
	} else {
		quote! {
			impl ::aiprog::registry::AipHandler for #handler_struct_ident {
				type Marker = #marker;
				type Params = #params_ty;
				type Output = #output_ty;

				fn handler_meta() -> ::aiprog::registry::AipHandlerMeta {
					#meta_fn_ident()
				}

				fn create_definition(path: &str) -> ::aiprog::registry::registry_internal::HandlerDefinition {
					let params_schema = schemars::schema_for!(#params_ty);
					let output_schema = schemars::schema_for!(#output_ty);
					let error_schema = schemars::schema_for!(::aiprog::HandlerError);

					let factory: ::aiprog::registry::registry_internal::HandlerFactory = Box::new(|call_context| {
						let closure: ::aiprog::registry::registry_internal::LuaSyncClosure = Box::new(move |lua, value| {
							let params = <#params_ty as ::aiprog::AipFromLua>::from_lua(&lua, value)
								.map_err(|e| mlua::Error::ExternalError(std::sync::Arc::new(e)))?;
							let result = #original_ident(call_context.clone(), params);
							match result {
								Ok(output) => {
									<#output_ty as ::aiprog::AipIntoLua>::into_lua(output, &lua)
										.map_err(|e| mlua::Error::ExternalError(std::sync::Arc::new(e)))
								}
								Err(e) => {
									let err: ::aiprog::Error = e.into();
									Err(mlua::Error::ExternalError(std::sync::Arc::new(err)))
								}
							}
						});
						::aiprog::registry::registry_internal::AipHandlerClosure::Sync(closure)
					});

					let meta = Self::handler_meta();
					::aiprog::registry::registry_internal::HandlerDefinition {
						path: path.to_string(),
						kind: ::aiprog::registry::AipFnKind::Sync,
						params_schema,
						output_schema,
						error_schema,
						description: meta.description,
						title: meta.title,
						factory,
					}
				}
			}
		}
	};

	let expanded = quote! {
		#output_fn
		#meta_fn
		#struct_def
		#impl_block
	};

	expanded
}

// region:    --- Helpers

fn extract_title_desc(attrs: &[syn::Attribute]) -> (Option<String>, Option<String>) {
	let doc_lines: Vec<String> = attrs
		.iter()
		.filter(|a| a.path().is_ident("doc"))
		.filter_map(|a| {
			if let syn::Meta::NameValue(nv) = &a.meta
				&& let syn::Expr::Lit(el) = &nv.value
				&& let syn::Lit::Str(s) = &el.lit
			{
				Some(s.value())
			} else {
				None
			}
		})
		.collect();
	let doc_str = doc_lines.join("\n");
	let doc_str = doc_str.trim();
	if doc_str.is_empty() {
		return (None, None);
	}

	let first_line = doc_str.lines().find(|l| !l.trim().is_empty()).map(|l| l.trim());
	if let Some(first) = first_line
		&& let Some(stripped) = first.strip_prefix("# ")
	{
		let title = stripped.trim().to_string();
		let idx = doc_str.lines().position(|l| l.trim() == first).unwrap();
		let rest = doc_str.lines().skip(idx + 1).collect::<Vec<_>>().join("\n").trim().to_string();
		let desc = if rest.is_empty() { None } else { Some(rest) };
		return (Some(title), desc);
	}

	(None, Some(doc_str.to_string()))
}

fn get_params_ty(sig: &syn::Signature) -> syn::Result<syn::Type> {
	let mut inputs = sig.inputs.iter().filter_map(|arg| match arg {
		syn::FnArg::Receiver(_) => None,
		syn::FnArg::Typed(pat) => Some(pat),
	});
	let call_context = inputs
		.next()
		.ok_or_else(|| syn::Error::new_spanned(sig, "handler function is missing HandlerCallContext"))?;
	if !is_handler_call_context(&call_context.ty) {
		return Err(syn::Error::new_spanned(
			&call_context.ty,
			"handler function first parameter must be HandlerCallContext",
		));
	}
	let params = inputs
		.next()
		.ok_or_else(|| syn::Error::new_spanned(sig, "handler function is missing typed params"))?;
	Ok((*params.ty).clone())
}

fn is_handler_call_context(ty: &syn::Type) -> bool {
	matches!(ty, syn::Type::Path(type_path) if type_path.path.segments.last().is_some_and(|segment| segment.ident == "HandlerCallContext"))
}

fn get_output_inner_type(sig: &syn::Signature) -> syn::Result<syn::Type> {
	let output = match &sig.output {
		syn::ReturnType::Default => {
			return Err(syn::Error::new_spanned(
				sig,
				"handler function must specify a return type",
			));
		}
		syn::ReturnType::Type(_, ty) => ty.as_ref().clone(),
	};
	if let syn::Type::Path(type_path) = &output
		&& let Some(segment) = type_path.path.segments.last()
		&& segment.ident == "HandlerResult"
		&& let syn::PathArguments::AngleBracketed(args) = &segment.arguments
		&& let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first()
	{
		return Ok(inner_ty.clone());
	}
	Err(syn::Error::new_spanned(
		&sig.output,
		"handler function must return `HandlerResult<T>`",
	))
}

// endregion: --- Helpers

// region:    --- Tests

#[cfg(test)]
mod tests {
	use super::*;
	use quote::quote;

	#[test]
	fn test_sync_handler_expands() {
		let input = quote! {
			/// Parses a JSON string and returns the parsed value.
			fn my_parse_handler(
				_call: ::aiprog::HandlerCallContext,
				params: super::MyParams,
			) -> ::aiprog::HandlerResult<super::MyOutput> {
				unimplemented!()
			}
		};
		let result = aip_handler_attr(TokenStream::new(), input);
		let output_str = result.to_string();
		assert!(output_str.contains("__AiprogHandler_my_parse_handler"));
		assert!(output_str.contains("struct"));
		assert!(output_str.contains("__aiprog_meta_my_parse_handler"));
	}

	#[test]
	fn test_async_handler_expands() {
		let input = quote! {
			/// An async handler that processes data.
			async fn my_async_handler(
				_call: ::aiprog::HandlerCallContext,
				params: super::MyParams,
			) -> ::aiprog::HandlerResult<super::MyOutput> {
				unimplemented!()
			}
		};
		let result = aip_handler_attr(TokenStream::new(), input);
		let output_str = result.to_string();
		assert!(output_str.contains("AsyncMarker"));
		assert!(output_str.contains("__AiprogHandler_my_async_handler"));
		assert!(output_str.contains("__aiprog_assert_send_future"));
		assert!(output_str.contains("struct"));
		assert!(output_str.contains("__aiprog_meta_my_async_handler"));
		assert!(!output_str.contains("serde :: Serialize"));
	}

	#[test]
	fn test_handler_rejects_invalid_arity() {
		let input = quote! {
			fn invalid_handler(
				params: super::MyParams,
			) -> ::aiprog::HandlerResult<super::MyOutput> {
				unimplemented!()
			}
		};
		let result = aip_handler_attr(TokenStream::new(), input);
		let output_str = result.to_string();
		assert!(output_str.contains("handler function must have exactly two parameters"));
	}
}

// endregion: --- Tests
