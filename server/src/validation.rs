//! Validation helpers that strip sensitive data from error messages.

use validator::ValidationErrors;

/// Format validation errors for HTTP responses, stripping any `"value"` keys
/// from error params so that passwords and other sensitive inputs are never
/// echoed back to the client.
pub fn format_validation_errors(errors: &ValidationErrors) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (field, field_errors) in errors.field_errors() {
        for err in field_errors {
            if !out.is_empty() {
                out.push_str(", ");
            }
            // Use custom message if the validator provided one
            if let Some(ref msg) = err.message {
                let _ = write!(out, "{field}: {msg}");
            } else {
                // Build a params map WITHOUT the "value" key
                let safe_params: std::collections::HashMap<_, _> = err
                    .params
                    .iter()
                    .filter(|(k, _)| k.as_ref() != "value")
                    .collect();
                let _ = write!(out, "{field}: Validation error: {} {safe_params:?}", err.code);
            }
        }
    }
    out
}
