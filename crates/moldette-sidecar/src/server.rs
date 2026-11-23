use std::convert::Infallible;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Bytes, Incoming};
use hyper::header::{ALLOW, CONTENT_TYPE, HeaderValue, RETRY_AFTER};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::server::graceful::GracefulShutdown;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::time::sleep;

use crate::process;
use crate::state::State;

const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const ACCEPT_RETRY: Duration = Duration::from_millis(100);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

type ResponseBody = BoxBody<Bytes, Infallible>;

/// Serves connections until SIGTERM or Ctrl-C, then waits for the requests in
/// progress for up to the request time limit.
pub(crate) async fn serve(listener: TcpListener, state: Arc<State>) {
    let graceful = GracefulShutdown::new();
    let mut shutdown = pin!(shutdown_signal());
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    sleep(ACCEPT_RETRY).await;
                    continue;
                };
                let state = Arc::clone(&state);
                let service = service_fn(move |request| {
                    let state = Arc::clone(&state);
                    async move { Ok::<_, Infallible>(route(state, request)) }
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
            () = &mut shutdown => break,
        }
    }
    drop(listener);
    tokio::select! {
        () = graceful.shutdown() => {}
        () = sleep(state.limits.timeout + SHUTDOWN_GRACE) => {}
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

fn route(state: Arc<State>, request: Request<Incoming>) -> Response<ResponseBody> {
    let method = request.method();
    match request.uri().path() {
        "/healthz" if method == Method::GET => text(StatusCode::OK, "ok\n"),
        "/readyz" if method == Method::GET => text(StatusCode::OK, "ready\n"),
        "/v1/process" if method == Method::POST => process(state, request),
        "/healthz" | "/readyz" => not_allowed("GET"),
        "/v1/process" => not_allowed("POST"),
        _ => error(StatusCode::NOT_FOUND, "no such endpoint"),
    }
}

fn process(state: Arc<State>, request: Request<Incoming>) -> Response<ResponseBody> {
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
    let output = process::start(state, request.into_body(), permit);
    let mut response = Response::new(output.boxed());
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
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
