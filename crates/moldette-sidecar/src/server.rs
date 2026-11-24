use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Bytes, Incoming};
use hyper::header::{ALLOW, CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::server::graceful::GracefulShutdown;
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;

use crate::process;
use crate::state::State;
use crate::targets::Target;

const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
/// The digest of the configuration that processed a request (#122).
const CONFIG_DIGEST: HeaderName = HeaderName::from_static("moldette-config-digest");
const ACCEPT_RETRY: Duration = Duration::from_millis(100);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

type ResponseBody = BoxBody<Bytes, Infallible>;

/// The endpoints a listener serves.
#[derive(Clone, Copy)]
enum Endpoints {
    All,
    Health,
}

/// Serves connections until SIGTERM or Ctrl-C, then waits for the requests in
/// progress for up to the request time limit. `health`, when given, answers
/// only the probe endpoints.
pub(crate) async fn serve(listener: TcpListener, health: Option<TcpListener>, state: Arc<State>) {
    let graceful = GracefulShutdown::new();
    let mut shutdown = pin!(shutdown_signal());
    loop {
        let (accepted, endpoints) = tokio::select! {
            accepted = listener.accept() => (accepted, Endpoints::All),
            accepted = accept(health.as_ref()) => (accepted, Endpoints::Health),
            () = &mut shutdown => break,
        };
        let Ok((stream, _)) = accepted else {
            sleep(ACCEPT_RETRY).await;
            continue;
        };
        let state = Arc::clone(&state);
        let service = service_fn(move |request| {
            let state = Arc::clone(&state);
            async move { Ok::<_, Infallible>(route(state, request, endpoints)) }
        });
        let connection = http1::Builder::new()
            .timer(TokioTimer::new())
            .header_read_timeout(HEADER_TIMEOUT)
            .serve_connection(TokioIo::new(stream), service);
        let connection = graceful.watch(connection);
        tokio::spawn(async move {
            let _ = connection.await;
        });
    }
    drop(listener);
    drop(health);
    tokio::select! {
        () = graceful.shutdown() => {}
        () = sleep(state.limits.timeout + SHUTDOWN_GRACE) => {}
    }
}

async fn accept(listener: Option<&TcpListener>) -> io::Result<(TcpStream, SocketAddr)> {
    match listener {
        Some(listener) => listener.accept().await,
        None => std::future::pending().await,
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = terminate.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

fn route(
    state: Arc<State>,
    request: Request<Incoming>,
    endpoints: Endpoints,
) -> Response<ResponseBody> {
    let method = request.method();
    let path = request.uri().path();
    match (path, endpoints) {
        ("/healthz", _) if method == Method::GET => return text(StatusCode::OK, "ok\n"),
        ("/readyz", _) if method == Method::GET => return text(StatusCode::OK, "ready\n"),
        ("/healthz" | "/readyz", _) => return not_allowed("GET"),
        (_, Endpoints::Health) => return error(StatusCode::NOT_FOUND, "no such endpoint"),
        _ => {}
    }
    let Some(name) = process_name(path) else {
        return error(StatusCode::NOT_FOUND, "no such endpoint");
    };
    let Some(target) = state.targets.get(name).map(Arc::clone) else {
        let message = match name {
            Some(name) => format!("no configuration is named `{name}`"),
            None => "name the configuration: /v1/process/{name}".to_owned(),
        };
        return error(StatusCode::NOT_FOUND, &message);
    };
    if method != Method::POST {
        return not_allowed("POST");
    }
    process(state, target, request)
}

/// The configuration name of a processing path: `None` for `/v1/process`,
/// the name for `/v1/process/{name}`.
fn process_name(path: &str) -> Option<Option<&str>> {
    match path.strip_prefix("/v1/process")? {
        "" => Some(None),
        rest => rest
            .strip_prefix('/')
            .filter(|name| !name.is_empty() && !name.contains('/'))
            .map(Some),
    }
}

fn process(
    state: Arc<State>,
    target: Arc<Target>,
    request: Request<Incoming>,
) -> Response<ResponseBody> {
    let limit = state.limits.max_body_bytes;
    if request.body().size_hint().lower() > limit {
        let message = format!("the request body is larger than {limit} bytes");
        return error(StatusCode::PAYLOAD_TOO_LARGE, &message);
    }
    let Ok(permit) = Arc::clone(&state.requests).try_acquire_owned() else {
        let mut response = error(
            StatusCode::SERVICE_UNAVAILABLE,
            "too many requests in progress",
        );
        response
            .headers_mut()
            .insert(RETRY_AFTER, HeaderValue::from_static("1"));
        return response;
    };
    let digest = target.digest.clone();
    let output = process::start(state, target, request.into_body(), permit);
    let mut response = Response::new(output.boxed());
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    headers.insert(CONFIG_DIGEST, digest);
    response
}

fn text(status: StatusCode, body: &'static str) -> Response<ResponseBody> {
    let mut response = Response::new(Full::new(Bytes::from_static(body.as_bytes())).boxed());
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn error(status: StatusCode, message: &str) -> Response<ResponseBody> {
    let body = json!({ "error": { "message": message } }).to_string() + "\n";
    let mut response = Response::new(Full::new(Bytes::from(body)).boxed());
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

fn not_allowed(allow: &'static str) -> Response<ResponseBody> {
    let mut response = error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allow));
    response
}
