//! `moldette-sidecar`, which runs configurations over records sent to it
//! over HTTP (#103, #122).

mod budget;
mod load;
mod metrics;
mod pool;
mod process;
mod server;
mod state;
mod targets;

use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use hyper::header::HeaderValue;
use moldette::Pipeline;
use moldette::framing::Framer;

use metrics::{ConfigMetrics, Metrics};
use state::{Limits, State};
use targets::{Target, Targets};

// The static container image links musl, whose allocator serializes threads.
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

const USAGE: &str = "\
Usage: moldette-sidecar [options] <config>
       moldette-sidecar [options] <name>=<config>...

Runs configurations over records sent over HTTP.

Endpoints:
  POST /v1/process         With one configuration given without a name:
                           split the request body into records and stream
                           one JSON response per line, in input order. When
                           the body cannot be read to the end, the last line
                           is an object with an `error` entry holding the
                           `record` and a `message`. The response header
                           `moldette-config-digest` names the configuration.
  POST /v1/process/<name>  The same with the configuration of that name
  GET /healthz             200 while the process runs
  GET /readyz              200 once the configurations are loaded and the
                           address is open

Options (an option and its value may also be joined with `=`, as in
`--threads=4`):
  --listen <address>      Address and port to listen on [default:
                          127.0.0.1:8095]
  --health-listen <address>
                          A second address that answers only GET /healthz
                          and GET /readyz, for probes that cannot reach the
                          loopback address
  --metrics-listen <address>
                          An address that answers only GET /metrics with the
                          metrics in the OpenMetrics text format; without it,
                          no metrics address is opened
  --threads <n>           Threads that parse and map records [default: the
                          number of available cores]
  --records [<name>=]<how>
                          How a request body splits into records: `lines`,
                          one per line (the default); `multiline`, see
                          --record-start; `json`, one per JSON value, or per
                          element of a top-level array. With a name, for that
                          configuration only
  --record-start [<name>=]<regex>
                          For `multiline`, the pattern of a line that starts a
                          record; each following line that does not match
                          joins it. A prefix that names a configuration is
                          for that configuration only
  --max-body-bytes <n>    Largest request body [default: 67108864]
  --max-record-bytes <n>  Largest record [default: 1048576]
  --max-buffer-bytes <n>  Bytes one request holds: records being processed
                          and responses the client has not read; the body is
                          not read further until it holds less [default:
                          16777216]
  --timeout <seconds>     Time for one request, from reading its body to
                          writing the last response [default: 30]
  --max-requests <n>      Requests processed at once; more are answered with
                          503 [default: 16]

A configuration file whose name ends in `.json` is read as JSON, any other as
YAML. Only built-in formats and ops are available. The configurations share
the threads and the limits, and a changed configuration takes effect on
restart. SIGTERM stops accepting connections and waits for the requests in
progress.";

const USAGE_ERROR: u8 = 2;
const DEFAULT_LISTEN: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 8095));
const DEFAULT_MAX_BODY_BYTES: u64 = 64 << 20;
const DEFAULT_MAX_RECORD_BYTES: usize = 1 << 20;
const DEFAULT_MAX_BUFFER_BYTES: NonZeroUsize = NonZeroUsize::new(16 << 20).unwrap();
const DEFAULT_TIMEOUT_SECONDS: NonZeroU64 = NonZeroU64::new(30).unwrap();
const DEFAULT_MAX_REQUESTS: NonZeroUsize = NonZeroUsize::new(16).unwrap();
const IO_THREADS: usize = 2;

struct Settings {
    listen: SocketAddr,
    health_listen: Option<SocketAddr>,
    metrics_listen: Option<SocketAddr>,
    threads: NonZeroUsize,
    records: Vec<String>,
    record_starts: Vec<String>,
    max_record_bytes: usize,
    limits: Limits,
}

/// A configuration on the command line: `<config>`, or `<name>=<config>`.
struct ConfigArg {
    name: Option<String>,
    path: PathBuf,
}

fn main() -> ExitCode {
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    if let [flag] = args.as_slice() {
        if flag == "--help" || flag == "-h" {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        if flag == "--version" || flag == "-V" {
            println!("moldette-sidecar {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
    }
    let settings = match settings(&mut args) {
        Ok(settings) => settings,
        Err(code) => return code,
    };
    let metrics = Metrics::new();
    let targets = match config_args(&args).and_then(|configs| targets(configs, &settings, &metrics))
    {
        Ok(targets) => targets,
        Err(code) => return code,
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(IO_THREADS)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("error: the runtime did not start: {err}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(settings.listen).await {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("error: {}: {err}", settings.listen);
                return ExitCode::FAILURE;
            }
        };
        let health = match settings.health_listen {
            Some(address) => match tokio::net::TcpListener::bind(address).await {
                Ok(listener) => Some(listener),
                Err(err) => {
                    eprintln!("error: {address}: {err}");
                    return ExitCode::FAILURE;
                }
            },
            None => None,
        };
        let metrics_listener = match settings.metrics_listen {
            Some(address) => match tokio::net::TcpListener::bind(address).await {
                Ok(listener) => Some(listener),
                Err(err) => {
                    eprintln!("error: {address}: {err}");
                    return ExitCode::FAILURE;
                }
            },
            None => None,
        };
        let address = listener.local_addr().unwrap_or(settings.listen);
        let state = Arc::new(State::new(
            settings.threads,
            targets,
            metrics,
            settings.limits,
        ));
        eprintln!(
            "moldette-sidecar {} listening on {address}",
            env!("CARGO_PKG_VERSION")
        );
        if let Some(Ok(address)) = health.as_ref().map(tokio::net::TcpListener::local_addr) {
            eprintln!("moldette-sidecar health checks on {address}");
        }
        if let Some(Ok(address)) = metrics_listener
            .as_ref()
            .map(tokio::net::TcpListener::local_addr)
        {
            eprintln!("moldette-sidecar metrics on {address}");
        }
        server::serve(listener, health, metrics_listener, state).await;
        eprintln!("moldette-sidecar stopped");
        ExitCode::SUCCESS
    })
}

fn settings(args: &mut Vec<OsString>) -> Result<Settings, ExitCode> {
    let records = take_options(args, "--records").map_err(|()| usage_error())?;
    let record_starts = take_options(args, "--record-start").map_err(|()| usage_error())?;
    let mut option = |name| take_option(args, name).map_err(|()| usage_error());
    let listen = option("--listen")?;
    let health_listen = option("--health-listen")?;
    let metrics_listen = option("--metrics-listen")?;
    let threads = option("--threads")?;
    let max_body_bytes = option("--max-body-bytes")?;
    let max_record_bytes = option("--max-record-bytes")?;
    let max_buffer_bytes = option("--max-buffer-bytes")?;
    let timeout = option("--timeout")?;
    let max_requests = option("--max-requests")?;
    let available = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    Ok(Settings {
        listen: parse(listen, DEFAULT_LISTEN)?,
        health_listen: health_listen
            .map(|address| address.parse().map_err(|_| usage_error()))
            .transpose()?,
        metrics_listen: metrics_listen
            .map(|address| address.parse().map_err(|_| usage_error()))
            .transpose()?,
        threads: parse(threads, available)?,
        records,
        record_starts,
        max_record_bytes: parse(max_record_bytes, DEFAULT_MAX_RECORD_BYTES)?,
        limits: Limits {
            max_body_bytes: parse(max_body_bytes, DEFAULT_MAX_BODY_BYTES)?,
            max_buffer_bytes: parse(max_buffer_bytes, DEFAULT_MAX_BUFFER_BYTES)?.get(),
            timeout: Duration::from_secs(parse(timeout, DEFAULT_TIMEOUT_SECONDS)?.get()),
            max_requests: parse(max_requests, DEFAULT_MAX_REQUESTS)?.get(),
        },
    })
}

fn parse<T: FromStr>(value: Option<String>, default: T) -> Result<T, ExitCode> {
    match value {
        Some(value) => value.parse().map_err(|_| usage_error()),
        None => Ok(default),
    }
}

/// Takes the first `<name> <value>` or `<name>=<value>` from `args`.
fn take_option(args: &mut Vec<OsString>, name: &str) -> Result<Option<String>, ()> {
    let joined = |arg: &OsString| {
        arg.to_str()
            .and_then(|arg| arg.strip_prefix(name)?.strip_prefix('='))
            .map(str::to_owned)
    };
    let Some(i) = args
        .iter()
        .position(|arg| arg == name || joined(arg).is_some())
    else {
        return Ok(None);
    };
    if let Some(value) = joined(&args[i]) {
        args.remove(i);
        return Ok(Some(value));
    }
    let value = args
        .get(i + 1)
        .and_then(|value| value.to_str())
        .map(str::to_owned);
    args.drain(i..(i + 2).min(args.len()));
    value.map(Some).ok_or(())
}

/// Takes every occurrence of an option that may be given more than once.
fn take_options(args: &mut Vec<OsString>, name: &str) -> Result<Vec<String>, ()> {
    let mut values = Vec::new();
    while let Some(value) = take_option(args, name)? {
        values.push(value);
    }
    Ok(values)
}

fn config_args(args: &[OsString]) -> Result<Vec<ConfigArg>, ExitCode> {
    let args: Vec<&str> = args
        .iter()
        .map(|arg| arg.to_str())
        .collect::<Option<_>>()
        .ok_or_else(usage_error)?;
    if let [path] = args.as_slice()
        && !path.contains('=')
    {
        return Ok(vec![ConfigArg {
            name: None,
            path: PathBuf::from(path),
        }]);
    }
    if args.is_empty() {
        return Err(usage_error());
    }
    let mut configs: Vec<ConfigArg> = Vec::with_capacity(args.len());
    for arg in args {
        let Some((name, path)) = arg.split_once('=') else {
            return Err(usage_error());
        };
        if !is_name(name) || path.is_empty() {
            return Err(usage_error());
        }
        if configs
            .iter()
            .any(|config| config.name.as_deref() == Some(name))
        {
            return Err(argument_error(&format!(
                "configuration `{name}` is given twice"
            )));
        }
        configs.push(ConfigArg {
            name: Some(name.to_owned()),
            path: PathBuf::from(path),
        });
    }
    Ok(configs)
}

fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// Loads each configuration with its framing.
fn targets(
    configs: Vec<ConfigArg>,
    settings: &Settings,
    metrics: &Metrics,
) -> Result<Targets, ExitCode> {
    let names: Vec<&str> = configs.iter().filter_map(|c| c.name.as_deref()).collect();
    let (records, default_records) = per_config("--records", &settings.records, &names, |value| {
        value.split_once('=')
    })?;
    let (starts, default_start) =
        per_config("--record-start", &settings.record_starts, &names, |value| {
            value
                .split_once('=')
                .filter(|(name, _)| names.contains(name))
        })?;
    let mut unnamed = None;
    let mut named = HashMap::new();
    let mut multiline = false;
    for config in configs {
        let name = config.name.as_deref();
        let how = name
            .and_then(|name| records.get(name))
            .copied()
            .or(default_records);
        let start = match name.and_then(|name| starts.get(name)) {
            Some(start) => Some(*start),
            None => default_start.filter(|_| how == Some("multiline")),
        };
        multiline |= how == Some("multiline");
        let framer = framer(how, start)?.max_record_len(settings.max_record_bytes);
        let config_metrics = metrics.config(name.unwrap_or_default());
        let target = Arc::new(target(&config.path, framer, config_metrics)?);
        match config.name {
            Some(name) => {
                named.insert(name, target);
            }
            None => unnamed = Some(target),
        }
    }
    if default_start.is_some() && !multiline {
        return Err(usage_error());
    }
    Ok(match unnamed {
        Some(target) => Targets::Unnamed(target),
        None => Targets::Named(named),
    })
}

fn target(path: &Path, framer: Framer, metrics: ConfigMetrics) -> Result<Target, ExitCode> {
    let specs = load::specs(path)?;
    let fail = |message: &dyn std::fmt::Display| {
        eprintln!("error: {}: {message}", path.display());
        ExitCode::FAILURE
    };
    let pipeline = Pipeline::from_specs(&specs).map_err(|err| fail(&err))?;
    let digest = HeaderValue::from_str(specs.digest()).map_err(|err| fail(&err))?;
    Ok(Target {
        pipeline: Arc::new(pipeline),
        framer,
        digest,
        metrics,
    })
}

/// Splits the values of an option given as `<value>` for every configuration
/// or as `<name>=<value>` for one, which `named` recognizes.
fn per_config<'v>(
    option: &str,
    values: &'v [String],
    names: &[&str],
    named: impl Fn(&'v str) -> Option<(&'v str, &'v str)>,
) -> Result<(HashMap<&'v str, &'v str>, Option<&'v str>), ExitCode> {
    let mut per_config = HashMap::new();
    let mut default = None;
    for value in values {
        match named(value) {
            Some((name, _)) if !names.contains(&name) => {
                return Err(argument_error(&format!(
                    "{option}: no configuration is named `{name}`"
                )));
            }
            Some((name, value)) => {
                if per_config.insert(name, value).is_some() {
                    return Err(argument_error(&format!(
                        "{option} is given twice for `{name}`"
                    )));
                }
            }
            None if default.replace(value.as_str()).is_some() => {
                return Err(argument_error(&format!("{option} is given twice")));
            }
            None => {}
        }
    }
    Ok((per_config, default))
}

fn framer(records: Option<&str>, record_start: Option<&str>) -> Result<Framer, ExitCode> {
    match (records, record_start) {
        (None | Some("lines"), None) => Ok(Framer::lines()),
        (Some("json"), None) => Ok(Framer::json()),
        (Some("multiline"), Some(start)) => Framer::multiline(start).map_err(|err| {
            eprintln!("error: --record-start: {err}");
            ExitCode::from(USAGE_ERROR)
        }),
        _ => Err(usage_error()),
    }
}

fn argument_error(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(USAGE_ERROR)
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(USAGE_ERROR)
}
