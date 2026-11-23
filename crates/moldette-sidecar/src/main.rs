//! `moldette-sidecar`, which runs one configuration over records sent to it
//! over HTTP (#103).

mod budget;
mod load;
mod pool;
mod process;
mod server;
mod state;

use std::env;
use std::ffi::OsString;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::Path;
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use moldette::Pipeline;
use moldette::framing::Framer;

use state::{Limits, State};

// The static container image links musl, whose allocator serializes threads.
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

const USAGE: &str = "\
Usage: moldette-sidecar [options] <config>

Runs one configuration over records sent over HTTP.

Endpoints:
  POST /v1/process  Split the request body into records and stream one JSON
                    response per line, in input order. When the body cannot
                    be read to the end, the last line is an object with an
                    `error` entry holding the `record` and a `message`.
  GET /healthz      200 while the process runs
  GET /readyz       200 once the configuration is loaded and the address
                    is open

Options:
  --listen <address>      Address and port to listen on [default:
                          127.0.0.1:8095]
  --health-listen <address>
                          A second address that answers only GET /healthz
                          and GET /readyz, for probes that cannot reach the
                          loopback address
  --threads <n>           Threads that parse and map records [default: the
                          number of available cores]
  --records <how>         How a request body splits into records: `lines`,
                          one per line (the default); `multiline`, see
                          --record-start; `json`, one per JSON value, or per
                          element of a top-level array
  --record-start <regex>  For `multiline`, the pattern of a line that starts a
                          record; each following line that does not match
                          joins it
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
YAML. Only built-in formats and ops are available. SIGTERM stops accepting
connections and waits for the requests in progress.";

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
    threads: NonZeroUsize,
    framer: Framer,
    limits: Limits,
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
    let [config] = args.as_slice() else {
        return usage_error();
    };
    let config = Path::new(config);
    let specs = match load::specs(config) {
        Ok(specs) => specs,
        Err(code) => return code,
    };
    let pipeline = match Pipeline::from_specs(&specs) {
        Ok(pipeline) => pipeline,
        Err(err) => {
            eprintln!("error: {}: {err}", config.display());
            return ExitCode::FAILURE;
        }
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
        let address = listener.local_addr().unwrap_or(settings.listen);
        let state = Arc::new(State::new(
            pipeline,
            settings.threads,
            settings.framer,
            settings.limits,
        ));
        eprintln!(
            "moldette-sidecar {} listening on {address}",
            env!("CARGO_PKG_VERSION")
        );
        if let Some(Ok(address)) = health.as_ref().map(tokio::net::TcpListener::local_addr) {
            eprintln!("moldette-sidecar health checks on {address}");
        }
        server::serve(listener, health, state).await;
        eprintln!("moldette-sidecar stopped");
        ExitCode::SUCCESS
    })
}

fn settings(args: &mut Vec<OsString>) -> Result<Settings, ExitCode> {
    let mut option = |name| take_option(args, name).map_err(|()| usage_error());
    let listen = option("--listen")?;
    let health_listen = option("--health-listen")?;
    let threads = option("--threads")?;
    let records = option("--records")?;
    let record_start = option("--record-start")?;
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
        threads: parse(threads, available)?,
        framer: framer(records, record_start)?
            .max_record_len(parse(max_record_bytes, DEFAULT_MAX_RECORD_BYTES)?),
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

fn take_option(args: &mut Vec<OsString>, name: &str) -> Result<Option<String>, ()> {
    let Some(i) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    let value = args
        .get(i + 1)
        .and_then(|value| value.to_str())
        .map(str::to_owned);
    args.drain(i..(i + 2).min(args.len()));
    value.map(Some).ok_or(())
}

fn framer(records: Option<String>, record_start: Option<String>) -> Result<Framer, ExitCode> {
    match (records.as_deref(), record_start) {
        (None | Some("lines"), None) => Ok(Framer::lines()),
        (Some("json"), None) => Ok(Framer::json()),
        (Some("multiline"), Some(start)) => Framer::multiline(&start).map_err(|err| {
            eprintln!("error: --record-start: {err}");
            ExitCode::from(USAGE_ERROR)
        }),
        _ => Err(usage_error()),
    }
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(USAGE_ERROR)
}
