use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use http_body_util::{BodyExt, Empty, LengthLimitError, Limited};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::header::{ACCEPT, ETAG, HOST, HeaderValue, IF_NONE_MATCH, USER_AGENT};
use hyper::{Request, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use moldette::framing::Framer;
use serde_json::{Map, Value};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

use crate::is_name;
use crate::load::{self, Built, Format, Message};
use crate::state::State;
use crate::targets::{Named, Target, Targets};

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const ENTRY_KEYS: [&str; 5] = ["name", "format", "body", "records", "record_start"];

/// A URL that lists configurations, read at an interval (#135, #142).
pub(crate) struct Source {
    pub(crate) uri: Uri,
    pub(crate) interval: Duration,
    pub(crate) max_bytes: usize,
}

impl Source {
    /// The source at `url`, which must be an `http` URL.
    pub(crate) fn new(url: &str, interval: Duration, max_bytes: usize) -> Result<Self, String> {
        let uri: Uri = url
            .parse()
            .map_err(|err| format!("--config-source: {err}"))?;
        if uri.scheme_str() != Some("http") || uri.authority().is_none() {
            return Err("--config-source: only http URLs are supported".to_owned());
        }
        Ok(Self {
            uri,
            interval,
            max_bytes,
        })
    }
}

enum Fetched {
    NotModified,
    List {
        body: Bytes,
        etag: Option<HeaderValue>,
    },
}

/// Reads the source until the process stops. The configurations named in
/// `files` were given at start and are not replaced.
pub(crate) async fn poll(
    source: Source,
    state: Arc<State>,
    files: HashSet<String>,
    max_record_bytes: usize,
) {
    let Targets::Named(named) = &state.targets else {
        return;
    };
    let mut etag = None;
    let mut applied = Applied {
        files,
        max_record_bytes,
        entries: HashMap::new(),
    };
    loop {
        let fetched = timeout(FETCH_TIMEOUT, fetch(&source, etag.as_ref()))
            .await
            .unwrap_or_else(|_| Err(format!("no answer in {} seconds", FETCH_TIMEOUT.as_secs())));
        let result = fetched.and_then(|fetched| match fetched {
            Fetched::NotModified => Ok("not_modified"),
            Fetched::List { body, etag: tag } => {
                tokio::task::block_in_place(|| applied.apply(&state, named, &body))?;
                etag = tag;
                state.ready.store(true, Ordering::Relaxed);
                Ok("ok")
            }
        });
        match result {
            Ok(result) => state.metrics.source_request(result),
            Err(message) => {
                state.metrics.source_request("error");
                eprintln!("error: configuration source: {message}");
            }
        }
        sleep(source.interval).await;
    }
}

async fn fetch(source: &Source, etag: Option<&HeaderValue>) -> Result<Fetched, String> {
    let fail = |err: &dyn std::fmt::Display| err.to_string();
    let authority = source
        .uri
        .authority()
        .ok_or_else(|| "the URL has no host".to_owned())?;
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let stream = TcpStream::connect((host, authority.port_u16().unwrap_or(80)))
        .await
        .map_err(|err| fail(&err))?;
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|err| fail(&err))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let path = source
        .uri
        .path_and_query()
        .map_or("/", |path| path.as_str());
    let mut request = Request::get(path)
        .header(HOST, authority.as_str())
        .header(ACCEPT, "application/json")
        .header(
            USER_AGENT,
            concat!("moldette-sidecar/", env!("CARGO_PKG_VERSION")),
        );
    if let Some(etag) = etag {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let request = request
        .body(Empty::<Bytes>::new())
        .map_err(|err| fail(&err))?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|err| fail(&err))?;
    match response.status() {
        StatusCode::NOT_MODIFIED => Ok(Fetched::NotModified),
        StatusCode::OK => {
            let etag = response.headers().get(ETAG).cloned();
            let body = Limited::new(response.into_body(), source.max_bytes)
                .collect()
                .await
                .map_err(|err| {
                    if err.is::<LengthLimitError>() {
                        format!("the list is larger than {} bytes", source.max_bytes)
                    } else {
                        err.to_string()
                    }
                })?
                .to_bytes();
            Ok(Fetched::List { body, etag })
        }
        status => Err(format!("the source answered {status}")),
    }
}

/// The entries of the last list, by name, kept to build only the changed
/// ones.
struct Applied {
    files: HashSet<String>,
    max_record_bytes: usize,
    entries: HashMap<String, Value>,
}

impl Applied {
    /// Builds the changed entries of a list and replaces those that build;
    /// an entry that fails keeps the configuration it had.
    fn apply(&mut self, state: &State, named: &Named, body: &[u8]) -> Result<(), String> {
        let list: Value =
            serde_json::from_slice(body).map_err(|err| format!("the list is not JSON: {err}"))?;
        let configs = list
            .get("configs")
            .and_then(Value::as_array)
            .ok_or_else(|| "the list has no `configs` array".to_owned())?;
        let mut listed = HashSet::new();
        for entry in configs {
            let Some(name) = entry.get("name").and_then(Value::as_str) else {
                eprintln!("error: configuration source: an entry has no `name`");
                continue;
            };
            if !is_name(name) {
                eprintln!("error: configuration source: `{name}` is not a configuration name");
                continue;
            }
            if !listed.insert(name.to_owned()) {
                eprintln!("error: configuration source: `{name}` is listed twice");
                continue;
            }
            if self.entries.get(name) == Some(entry) {
                continue;
            }
            self.entries.insert(name.to_owned(), entry.clone());
            if self.files.contains(name) {
                eprintln!(
                    "warning: configuration source: `{name}` was given at start and is not replaced"
                );
                continue;
            }
            let (messages, built) = build(entry, self.max_record_bytes);
            for message in &messages {
                eprintln!(
                    "{}: configuration source: {name}: {}",
                    message.severity, message.text
                );
            }
            state.metrics.build(name, built.is_some());
            if let Some((built, framer)) = built {
                let target = Target::new(built, framer, state.metrics.config(name));
                named.insert(name.to_owned(), Arc::new(target));
                eprintln!("moldette-sidecar loaded `{name}` from the configuration source");
            }
        }
        let removed: Vec<String> = self
            .entries
            .keys()
            .filter(|name| !listed.contains(*name))
            .cloned()
            .collect();
        for name in removed {
            self.entries.remove(&name);
            if !self.files.contains(&name) {
                named.remove(&name);
                state.metrics.remove(&name);
                eprintln!(
                    "moldette-sidecar removed `{name}`, which the configuration source no longer lists"
                );
            }
        }
        Ok(())
    }
}

/// Builds one entry of the list with its framing.
fn build(entry: &Value, max_record_bytes: usize) -> (Vec<Message>, Option<(Built, Framer)>) {
    match parts(entry, max_record_bytes) {
        Ok((format, body, framer)) => {
            let checked = load::check(body, format);
            (checked.messages, checked.built.map(|built| (built, framer)))
        }
        Err(message) => (vec![message], None),
    }
}

/// The format, the text and the framing of an entry.
fn parts(entry: &Value, max_record_bytes: usize) -> Result<(Format, &str, Framer), Message> {
    let entry = entry
        .as_object()
        .ok_or_else(|| Message::error("the entry is not an object"))?;
    if let Some(key) = entry.keys().find(|key| !ENTRY_KEYS.contains(&key.as_str())) {
        return Err(Message::error(format!("unknown key `{key}`")));
    }
    let format = entry
        .get("format")
        .and_then(Value::as_str)
        .and_then(Format::from_name)
        .ok_or_else(|| Message::error("`format` must be `yaml` or `json`"))?;
    let body = entry
        .get("body")
        .and_then(Value::as_str)
        .ok_or_else(|| Message::error("`body` must be a string"))?;
    Ok((format, body, framer(entry, max_record_bytes)?))
}

/// The framing of an entry, from its keys `records` and `record_start`.
fn framer(entry: &Map<String, Value>, max_record_bytes: usize) -> Result<Framer, Message> {
    let text = |key| match entry.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.as_str())),
        Some(_) => Err(Message::error(format!("`{key}` must be a string"))),
    };
    let framer = match (text("records")?, text("record_start")?) {
        (None | Some("lines"), None) => Framer::lines(),
        (Some("json"), None) => Framer::json(),
        (Some("multiline"), Some(start)) => Framer::multiline(start)
            .map_err(|err| Message::error(format!("`record_start`: {err}")))?,
        _ => {
            return Err(Message::error(
                "`records` must be `lines`, `json` or `multiline`, and `record_start` goes with `multiline` only",
            ));
        }
    };
    Ok(framer.max_record_len(max_record_bytes))
}
