//! The low-code connector engine: executes declarative HTTP-API manifests as
//! a native, container-free source.
//!
//! [`DeclarativeSource`] implements the CDK [`Source`] trait; the manifest
//! travels inside the connector *config* under the reserved `manifest` key,
//! so a single registered binary (`exec:gauss-declarative`) serves any number
//! of API connectors — each source's config carries its own manifest plus
//! the user fields (credentials etc.) the manifest interpolates.

mod interpolate;
mod manifest;

pub use interpolate::interpolate;
pub use manifest::{Authenticator, CheckSection, Manifest, Paginator, Requester, StreamDef};

use std::cmp::Ordering;

use base64::Engine as _;
use gauss_cdk::protocol::*;
use gauss_cdk::{CdkError, Emitter, Source};
use serde_json::{json, Value};

/// Config-driven declarative source. Construct once; every operation loads
/// the manifest from the supplied config.
pub struct DeclarativeSource;

#[async_trait::async_trait]
impl Source for DeclarativeSource {
    fn spec(&self) -> ConnectorSpecification {
        // The binary's own spec is generic: a manifest plus arbitrary user
        // fields. Platform UIs show the *registered definition's* spec, which
        // can embed the manifest's connection_specification instead.
        let mut spec = ConnectorSpecification::new(json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "Declarative Source",
            "type": "object",
            "required": ["manifest"],
            "additionalProperties": true,
            "properties": {
                "manifest": {
                    "description": "Declarative manifest (inline object or YAML string)",
                    "type": ["object", "string"]
                }
            }
        }));
        spec.documentation_url = Some("https://github.com/rismanmattotorang/gaussdataflow".into());
        spec.supports_incremental = Some(true);
        spec
    }

    async fn check(&self, config: &Value) -> Result<AirbyteConnectionStatus, CdkError> {
        let manifest = Manifest::from_config(config)?;
        let stream_name = manifest
            .check
            .as_ref()
            .map(|c| c.stream.clone())
            .or_else(|| manifest.streams.first().map(|s| s.name.clone()))
            .ok_or_else(|| CdkError::Config("manifest has no streams".into()))?;
        let stream = find_stream(&manifest, &stream_name)?;

        // Probe: fetch the first page and resolve the record selector.
        match fetch_page(&manifest, stream, config, &first_page_params(stream)?).await {
            Ok((records, _)) => Ok(AirbyteConnectionStatus {
                status: ConnectionStatus::Succeeded,
                message: Some(format!(
                    "stream `{stream_name}` returned {} record(s) on the first page",
                    records.len()
                )),
            }),
            Err(error) => Ok(AirbyteConnectionStatus {
                status: ConnectionStatus::Failed,
                message: Some(error.to_string()),
            }),
        }
    }

    async fn discover(&self, config: &Value) -> Result<AirbyteCatalog, CdkError> {
        let manifest = Manifest::from_config(config)?;
        Ok(AirbyteCatalog {
            streams: manifest
                .streams
                .iter()
                .map(|def| {
                    let mut stream = AirbyteStream::new(
                        &def.name,
                        def.schema.clone().unwrap_or_else(
                            || json!({"type": "object", "additionalProperties": true}),
                        ),
                    );
                    let mut modes = vec![SyncMode::FullRefresh];
                    if def.cursor_field.is_some() {
                        modes.push(SyncMode::Incremental);
                        stream.source_defined_cursor = Some(true);
                        stream.default_cursor_field = def.cursor_field.clone().map(|f| vec![f]);
                    }
                    stream.supported_sync_modes = Some(modes);
                    stream.source_defined_primary_key = def
                        .primary_key
                        .clone()
                        .map(|pk| pk.into_iter().map(|f| vec![f]).collect());
                    stream
                })
                .collect(),
        })
    }

    async fn read(
        &self,
        config: &Value,
        catalog: &ConfiguredAirbyteCatalog,
        state: Option<&Value>,
        emitter: &mut Emitter,
    ) -> Result<(), CdkError> {
        let manifest = Manifest::from_config(config)?;
        for configured in &catalog.streams {
            let stream = find_stream(&manifest, &configured.stream.name)?;
            let incremental = configured.sync_mode == SyncMode::Incremental;
            sync_stream(&manifest, stream, config, state, incremental, emitter).await?;
        }
        Ok(())
    }
}

fn find_stream<'m>(manifest: &'m Manifest, name: &str) -> Result<&'m StreamDef, CdkError> {
    manifest
        .streams
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| CdkError::Config(format!("manifest defines no stream named `{name}`")))
}

/// Query params for the first page of a stream (static params + paginator).
fn first_page_params(stream: &StreamDef) -> Result<Vec<(String, String)>, CdkError> {
    Ok(PageCursor::start(stream.paginator.as_ref()).params())
}

/// Pagination position across the page loop.
enum PageCursor<'p> {
    Single {
        done: bool,
    },
    Offset {
        paginator: &'p Paginator,
        offset: u64,
    },
    Page {
        paginator: &'p Paginator,
        page: u64,
    },
    Cursor {
        paginator: &'p Paginator,
        token: Option<String>,
        first: bool,
    },
}

impl<'p> PageCursor<'p> {
    fn start(paginator: Option<&'p Paginator>) -> Self {
        match paginator {
            None => Self::Single { done: false },
            Some(p @ Paginator::Offset { .. }) => Self::Offset {
                paginator: p,
                offset: 0,
            },
            Some(p @ Paginator::Page { start_page, .. }) => Self::Page {
                paginator: p,
                page: *start_page,
            },
            Some(p @ Paginator::Cursor { .. }) => Self::Cursor {
                paginator: p,
                token: None,
                first: true,
            },
        }
    }

    fn params(&self) -> Vec<(String, String)> {
        match self {
            Self::Single { .. } => vec![],
            Self::Offset { paginator, offset } => {
                let Paginator::Offset {
                    page_size,
                    limit_param,
                    offset_param,
                } = paginator
                else {
                    unreachable!()
                };
                vec![
                    (limit_param.clone(), page_size.to_string()),
                    (offset_param.clone(), offset.to_string()),
                ]
            }
            Self::Page { paginator, page } => {
                let Paginator::Page {
                    page_param,
                    page_size,
                    size_param,
                    ..
                } = paginator
                else {
                    unreachable!()
                };
                let mut params = vec![(page_param.clone(), page.to_string())];
                if let (Some(size), Some(param)) = (page_size, size_param) {
                    params.push((param.clone(), size.to_string()));
                }
                params
            }
            Self::Cursor {
                paginator, token, ..
            } => {
                let Paginator::Cursor { cursor_param, .. } = paginator else {
                    unreachable!()
                };
                token
                    .as_ref()
                    .map(|t| vec![(cursor_param.clone(), t.clone())])
                    .unwrap_or_default()
            }
        }
    }

    /// Advance past a fetched page; `false` means the loop is done.
    fn advance(&mut self, records: usize, body: &Value) -> bool {
        match self {
            Self::Single { done } => {
                *done = true;
                false
            }
            Self::Offset { paginator, offset } => {
                let Paginator::Offset { page_size, .. } = paginator else {
                    unreachable!()
                };
                *offset += *page_size;
                records as u64 == *page_size && records > 0
            }
            Self::Page { paginator, page } => {
                let Paginator::Page { page_size, .. } = paginator else {
                    unreachable!()
                };
                *page += 1;
                match page_size {
                    Some(size) => records as u64 == *size && records > 0,
                    None => records > 0,
                }
            }
            Self::Cursor {
                paginator,
                token,
                first,
            } => {
                let Paginator::Cursor { cursor_path, .. } = paginator else {
                    unreachable!()
                };
                *first = false;
                *token =
                    select_path(body, cursor_path).and_then(|v| v.as_str().map(str::to_string));
                token.is_some()
            }
        }
    }
}

/// Fetch one page: returns the selected record array and the full body.
async fn fetch_page(
    manifest: &Manifest,
    stream: &StreamDef,
    config: &Value,
    page_params: &[(String, String)],
) -> Result<(Vec<Value>, Value), CdkError> {
    let url = format!(
        "{}{}",
        interpolate(&manifest.requester.url_base, config)?.trim_end_matches('/'),
        interpolate(&stream.path, config)?
    );

    let client = reqwest::Client::new();
    let mut request = client.get(&url);

    for (key, value) in &manifest.requester.headers {
        request = request.header(key, interpolate(value, config)?);
    }
    request = apply_auth(request, manifest.requester.authenticator.as_ref(), config)?;

    let mut query: Vec<(String, String)> = Vec::new();
    for (key, value) in &stream.params {
        query.push((key.clone(), interpolate(value, config)?));
    }
    query.extend_from_slice(page_params);
    if !query.is_empty() {
        request = request.query(&query);
    }

    let response = request
        .send()
        .await
        .map_err(|e| CdkError::Transient(format!("request to {url} failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(200).collect();
        return Err(if status.as_u16() == 429 || status.is_server_error() {
            CdkError::Transient(format!("{url} returned {status}: {snippet}"))
        } else {
            CdkError::Config(format!("{url} returned {status}: {snippet}"))
        });
    }

    let body: Value = response
        .json()
        .await
        .map_err(|e| CdkError::Other(format!("{url} returned non-JSON body: {e}")))?;

    let selected = match &stream.record_selector {
        Some(path) => select_path(&body, path).cloned().ok_or_else(|| {
            CdkError::Config(format!(
                "record_selector `{path}` matched nothing in the {url} response"
            ))
        })?,
        None => body.clone(),
    };
    let records = match selected {
        Value::Array(records) => records,
        single @ Value::Object(_) => vec![single],
        other => {
            return Err(CdkError::Config(format!(
                "record selection produced {other}, expected an array or object"
            )))
        }
    };
    Ok((records, body))
}

fn apply_auth(
    request: reqwest::RequestBuilder,
    auth: Option<&Authenticator>,
    config: &Value,
) -> Result<reqwest::RequestBuilder, CdkError> {
    Ok(match auth {
        None | Some(Authenticator::NoAuth {}) => request,
        Some(Authenticator::ApiKey { header, api_token }) => {
            request.header(header, interpolate(api_token, config)?)
        }
        Some(Authenticator::Bearer { api_token }) => request.header(
            "authorization",
            format!("Bearer {}", interpolate(api_token, config)?),
        ),
        Some(Authenticator::Basic { username, password }) => {
            let credentials = format!(
                "{}:{}",
                interpolate(username, config)?,
                interpolate(password, config)?
            );
            request.header(
                "authorization",
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode(credentials)
                ),
            )
        }
    })
}

/// Resolve a dot-path (`data.items`) into a JSON value.
fn select_path<'v>(value: &'v Value, path: &str) -> Option<&'v Value> {
    path.split('.')
        .filter(|part| !part.is_empty())
        .try_fold(value, |acc, part| acc.get(part))
}

/// Compare cursor values: numbers numerically, everything else as strings
/// (ISO-8601 timestamps compare correctly lexically).
fn compare_cursor(a: &Value, b: &Value) -> Ordering {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => json_as_string(a).cmp(&json_as_string(b)),
    }
}

fn json_as_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

async fn sync_stream(
    manifest: &Manifest,
    stream: &StreamDef,
    config: &Value,
    state: Option<&Value>,
    incremental: bool,
    emitter: &mut Emitter,
) -> Result<(), CdkError> {
    emitter.stream_status(&stream.name, StreamStatus::Started)?;

    let state_cursor = if incremental {
        stream
            .cursor_field
            .as_ref()
            .and_then(|field| gauss_cdk::state::cursor_value(state, &stream.name, field).cloned())
    } else {
        None
    };
    let mut max_cursor = state_cursor.clone();
    let mut emitted = 0u64;

    let mut page = PageCursor::start(stream.paginator.as_ref());
    loop {
        let (records, body) = fetch_page(manifest, stream, config, &page.params()).await?;
        let fetched = records.len();

        for record in records {
            if let Some(field) = &stream.cursor_field {
                if let Some(value) = record.get(field) {
                    if incremental {
                        if let Some(seen) = &state_cursor {
                            if compare_cursor(value, seen) != Ordering::Greater {
                                continue; // already synced
                            }
                        }
                    }
                    let is_new_max = max_cursor
                        .as_ref()
                        .map(|m| compare_cursor(value, m) == Ordering::Greater)
                        .unwrap_or(true);
                    if is_new_max {
                        max_cursor = Some(value.clone());
                    }
                }
            }
            emitter.record(&stream.name, None, record)?;
            emitted += 1;
        }

        if !page.advance(fetched, &body) {
            break;
        }
    }

    // Checkpoint the high-water mark for cursored streams.
    if let (Some(field), Some(cursor)) = (&stream.cursor_field, &max_cursor) {
        emitter.stream_state(&stream.name, json!({ field: cursor }), Some(emitted as f64))?;
    }
    emitter.stream_status(&stream.name, StreamStatus::Complete)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_path_resolves_nested() {
        let body = json!({"data": {"items": [1, 2]}, "next": null});
        assert_eq!(select_path(&body, "data.items"), Some(&json!([1, 2])));
        assert_eq!(select_path(&body, "missing.path"), None);
    }

    #[test]
    fn cursor_comparison_handles_numbers_and_timestamps() {
        assert_eq!(compare_cursor(&json!(10), &json!(9)), Ordering::Greater);
        assert_eq!(
            compare_cursor(
                &json!("2026-02-01T00:00:00Z"),
                &json!("2026-01-01T00:00:00Z")
            ),
            Ordering::Greater
        );
    }

    #[test]
    fn manifest_parses_from_yaml_string_and_object() {
        let yaml = r#"
requester:
  url_base: https://api.example.com
  authenticator: { type: bearer, api_token: "{{ config.token }}" }
streams:
  - name: users
    path: /users
    record_selector: data
    cursor_field: updated_at
    paginator: { type: offset, page_size: 50 }
"#;
        let from_string = Manifest::from_config(&json!({"manifest": yaml})).expect("yaml string");
        assert_eq!(from_string.streams[0].name, "users");

        let object: Value = serde_yaml::from_str(yaml).unwrap();
        let from_object =
            Manifest::from_config(&json!({"manifest": object})).expect("inline object");
        assert!(matches!(
            from_object.streams[0].paginator,
            Some(Paginator::Offset { page_size: 50, .. })
        ));

        assert!(Manifest::from_config(&json!({})).is_err());
    }
}
