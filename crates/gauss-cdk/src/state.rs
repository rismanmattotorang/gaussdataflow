//! Helpers for working with the standard state input (a JSON array of state
//! messages, the same shape the platform persists per connection).

use serde_json::Value;

/// Extract the `stream_state` object for one stream (no namespace) from the
/// state input, if present.
pub fn stream_state<'a>(state: Option<&'a Value>, stream_name: &str) -> Option<&'a Value> {
    let messages = state?.as_array()?;
    messages.iter().rev().find_map(|message| {
        let stream = message.get("stream")?;
        let descriptor = stream.get("stream_descriptor")?;
        if descriptor.get("name")?.as_str()? == stream_name
            && descriptor.get("namespace").is_none_or(Value::is_null)
        {
            stream.get("stream_state")
        } else {
            None
        }
    })
}

/// Convenience: a single field out of the stream's state object.
pub fn cursor_value<'a>(
    state: Option<&'a Value>,
    stream_name: &str,
    field: &str,
) -> Option<&'a Value> {
    stream_state(state, stream_name)?.get(field)
}
