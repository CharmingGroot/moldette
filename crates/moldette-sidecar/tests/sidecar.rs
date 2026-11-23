use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use moldette::Pipeline;
use moldette::config::{Config, Registry};
use moldette_spec::json::JsonResponse;

const CONFIG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../moldette-config/tests/fixtures/order_api.yaml"
);
const DATA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../moldette/tests/data/order_api.ndjson"
);

struct Sidecar {
    child: Child,
    stderr: BufReader<ChildStderr>,
    address: String,
}

impl Sidecar {
    fn start(options: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_moldette-sidecar"))
            .args(["--listen", "127.0.0.1:0"])
            .args(options)
            .arg(CONFIG)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the sidecar starts");
        let mut stderr = BufReader::new(child.stderr.take().expect("piped standard error"));
        let mut line = String::new();
        stderr.read_line(&mut line).expect("start line");
        let address = line
            .trim_end()
            .rsplit_once("listening on ")
            .map(|(_, address)| address.to_owned())
            .unwrap_or_else(|| panic!("no address in {line:?}"));
        Self {
            child,
            stderr,
            address,
        }
    }

    /// Reads the address from the next line of standard error that holds
    /// `label`.
    fn next_address(&mut self, label: &str) -> String {
        let mut line = String::new();
        self.stderr.read_line(&mut line).expect("address line");
        line.trim_end()
            .rsplit_once(label)
            .map(|(_, address)| address.to_owned())
            .unwrap_or_else(|| panic!("no {label:?} in {line:?}"))
    }

    fn connect(&self) -> TcpStream {
        connect(&self.address)
    }

    fn send(&self, request: &[u8]) -> Reply {
        send(&self.address, request)
    }

    fn get(&self, path: &str) -> Reply {
        self.send(
            format!("GET {path} HTTP/1.1\r\nHost: sidecar\r\nConnection: close\r\n\r\n").as_bytes(),
        )
    }

    fn post(&self, body: &[u8]) -> Reply {
        let mut request = format!(
            "POST /v1/process HTTP/1.1\r\nHost: sidecar\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(body);
        self.send(&request)
    }

    /// Posts `body` from another thread while reading the reply.
    fn post_reading_while_sending(&self, body: &[u8]) -> Reply {
        let mut stream = self.connect();
        let mut writer = stream.try_clone().expect("stream clone");
        let mut request = format!(
            "POST /v1/process HTTP/1.1\r\nHost: sidecar\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(body);
        let sending = thread::spawn(move || writer.write_all(&request));
        let reply = Reply::read(&mut stream);
        sending
            .join()
            .expect("the sending thread ends")
            .expect("request is written");
        reply
    }

    fn post_chunked(&self, chunks: &[&[u8]]) -> Reply {
        let mut request = b"POST /v1/process HTTP/1.1\r\nHost: sidecar\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for chunk in chunks {
            request.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
            request.extend_from_slice(chunk);
            request.extend_from_slice(b"\r\n");
        }
        request.extend_from_slice(b"0\r\n\r\n");
        self.send(&request)
    }

    #[cfg(unix)]
    fn terminate(&mut self) {
        let status = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()
            .expect("kill runs");
        assert!(status.success());
    }

    fn wait(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait().expect("sidecar status") {
                return status;
            }
            assert!(Instant::now() < deadline, "the sidecar did not stop");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn connect(address: &str) -> TcpStream {
    let stream = TcpStream::connect(address).expect("connected");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .expect("read timeout");
    stream
}

fn send(address: &str, request: &[u8]) -> Reply {
    let mut stream = connect(address);
    stream.write_all(request).expect("request is written");
    Reply::read(&mut stream)
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
struct Reply {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

impl Reply {
    fn read(stream: &mut TcpStream) -> Self {
        let mut reader = BufReader::new(stream);
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("header line");
            if line == "\r\n" || line.is_empty() {
                break;
            }
            headers.push_str(&line.to_ascii_lowercase());
        }
        let status = headers
            .split(' ')
            .nth(1)
            .and_then(|status| status.parse().ok())
            .unwrap_or_else(|| panic!("no status in {headers:?}"));
        let mut body = Vec::new();
        if headers.contains("transfer-encoding: chunked") {
            loop {
                let mut size = String::new();
                reader.read_line(&mut size).expect("chunk size");
                let size = usize::from_str_radix(size.trim_end(), 16).expect("hex chunk size");
                let mut chunk = vec![0; size + 2];
                reader.read_exact(&mut chunk).expect("chunk");
                if size == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..size]);
            }
        } else if let Some(length) = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
        {
            body = vec![0; length.trim_end().parse().expect("content length")];
            reader.read_exact(&mut body).expect("body");
        }
        Self {
            status,
            headers,
            body,
        }
    }

    fn text(&self) -> String {
        String::from_utf8(self.body.clone()).expect("UTF-8 body")
    }

    fn lines(&self) -> Vec<serde_json::Value> {
        self.text()
            .lines()
            .map(|line| serde_json::from_str(line).expect("one JSON value per line"))
            .collect()
    }
}

fn orders() -> String {
    fs::read_to_string(DATA).expect("order API data")
}

fn expected(input: &str) -> String {
    let yaml = fs::read_to_string(CONFIG).expect("configuration");
    let specs = Config::from_yaml(&yaml)
        .expect("configuration parses")
        .build(&Registry::new())
        .expect("configuration builds");
    let pipeline = Pipeline::from_specs(&specs).expect("pipeline");
    input
        .lines()
        .map(|line| {
            serde_json::to_string(&JsonResponse::new(&pipeline.process(line))).expect("JSON") + "\n"
        })
        .collect()
}

fn error(record: usize, message: &str) -> serde_json::Value {
    serde_json::json!({ "error": { "record": record, "message": message } })
}

#[test]
fn health_and_readiness_answer_while_running() {
    let sidecar = Sidecar::start(&[]);
    let health = sidecar.get("/healthz");
    assert_eq!((health.status, health.text().as_str()), (200, "ok\n"));
    let ready = sidecar.get("/readyz");
    assert_eq!((ready.status, ready.text().as_str()), (200, "ready\n"));
}

#[test]
fn responses_match_the_library_in_input_order() {
    let sidecar = Sidecar::start(&["--threads", "3"]);
    let input = orders().repeat(400);
    let reply = sidecar.post(input.as_bytes());
    assert_eq!(reply.status, 200);
    assert!(reply.headers.contains("content-type: application/x-ndjson"));
    assert_eq!(reply.text(), expected(&input));
}

#[test]
fn chunks_may_split_records_anywhere() {
    let sidecar = Sidecar::start(&[]);
    let input = orders();
    let chunks: Vec<&[u8]> = input.as_bytes().chunks(7).collect();
    let reply = sidecar.post_chunked(&chunks);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.text(), expected(&input));
}

#[test]
fn json_records_read_the_elements_of_an_array() {
    let sidecar = Sidecar::start(&["--records", "json"]);
    let orders = orders();
    let lines: Vec<&str> = orders.lines().take(6).collect();
    let array = format!("[\n{}\n]", lines.join(",\n"));
    let reply = sidecar.post(array.as_bytes());
    assert_eq!(reply.status, 200);
    assert_eq!(reply.text(), expected(&lines.join("\n")));
}

#[test]
fn a_declared_body_over_the_limit_is_rejected_before_reading() {
    let sidecar = Sidecar::start(&["--max-body-bytes", "10"]);
    let reply = sidecar.post(b"{\"id\":\"A-1\"}\n");
    assert_eq!(reply.status, 413);
    assert_eq!(
        reply.lines(),
        [serde_json::json!({ "error": { "message": "the request body is larger than 10 bytes" } })]
    );
}

#[test]
fn a_streamed_body_over_the_limit_ends_with_an_error_after_the_records_before_it() {
    let sidecar = Sidecar::start(&["--max-body-bytes", "100"]);
    let first = "{\"id\":\"A-1\"}\n".repeat(3);
    let second = "{\"id\":\"A-2\"}\n".repeat(8);
    let reply = sidecar.post_chunked(&[first.as_bytes(), second.as_bytes()]);
    assert_eq!(reply.status, 200);
    let lines = reply.lines();
    assert_eq!(lines.len(), 4);
    assert_eq!(
        lines[3],
        error(4, "the request body is larger than 100 bytes")
    );
}

#[test]
fn a_record_that_is_not_utf8_ends_the_output_with_its_number() {
    let sidecar = Sidecar::start(&[]);
    let reply = sidecar.post(b"{\"id\":\"A-1\"}\n\xff\n{\"id\":\"A-3\"}\n");
    let lines = reply.lines();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1], error(2, "not valid UTF-8"));
}

#[test]
fn records_longer_than_the_limit_fail() {
    let sidecar = Sidecar::start(&["--max-record-bytes", "16"]);
    let reply = sidecar.post(b"{\"id\":\"A-1\"}\n{\"id\":\"A-1002-long\"}\n");
    let lines = reply.lines();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1], error(2, "longer than 16 bytes"));
}

#[test]
fn a_request_that_outlasts_the_time_limit_ends_with_an_error() {
    let sidecar = Sidecar::start(&["--timeout", "1"]);
    let mut stream = sidecar.connect();
    stream
        .write_all(b"POST /v1/process HTTP/1.1\r\nHost: sidecar\r\nContent-Length: 1000\r\n\r\n{\"id\":\"A-1\"}\n")
        .expect("partial request");
    let started = Instant::now();
    let reply = Reply::read(&mut stream);
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(reply.status, 200);
    let lines = reply.lines();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1], error(2, "the request took longer than 1 seconds"));
}

#[test]
fn requests_over_the_limit_at_once_get_503() {
    let sidecar = Sidecar::start(&["--max-requests", "1", "--timeout", "5"]);
    let mut first = sidecar.connect();
    first
        .write_all(b"POST /v1/process HTTP/1.1\r\nHost: sidecar\r\nContent-Length: 1000\r\n\r\n")
        .expect("partial request");
    thread::sleep(Duration::from_millis(300));
    let second = sidecar.post(b"{\"id\":\"A-1\"}\n");
    assert_eq!(second.status, 503);
    assert!(second.headers.contains("retry-after: 1"));
}

#[test]
fn the_health_address_answers_only_probes() {
    let mut sidecar = Sidecar::start(&["--health-listen", "127.0.0.1:0"]);
    let health = sidecar.next_address("health checks on ");
    let get = |path: &str| {
        send(
            &health,
            format!("GET {path} HTTP/1.1\r\nHost: sidecar\r\nConnection: close\r\n\r\n").as_bytes(),
        )
    };
    assert_eq!(get("/healthz").status, 200);
    assert_eq!(get("/readyz").text(), "ready\n");
    let process = send(
        &health,
        b"POST /v1/process HTTP/1.1\r\nHost: sidecar\r\nConnection: close\r\nContent-Length: 2\r\n\r\n{}",
    );
    assert_eq!(process.status, 404);
    assert_eq!(sidecar.post(b"{}").status, 200);
}

#[test]
fn unknown_paths_and_methods_are_refused() {
    let sidecar = Sidecar::start(&[]);
    assert_eq!(sidecar.get("/v1/explain").status, 404);
    let get = sidecar.get("/v1/process");
    assert_eq!(get.status, 405);
    assert!(get.headers.contains("allow: post"));
    let post = sidecar.send(b"POST /healthz HTTP/1.1\r\nHost: sidecar\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
    assert_eq!(post.status, 405);
    assert!(post.headers.contains("allow: get"));
}

#[cfg(unix)]
#[test]
fn sigterm_lets_the_request_in_progress_finish() {
    let mut sidecar = Sidecar::start(&[]);
    let body = b"{\"id\":\"A-1\"}\n{\"id\":\"A-2\"}\n";
    let mut stream = sidecar.connect();
    stream
        .write_all(format!("POST /v1/process HTTP/1.1\r\nHost: sidecar\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes())
        .expect("request head");
    stream.write_all(&body[..13]).expect("first record");
    thread::sleep(Duration::from_millis(200));
    sidecar.terminate();
    thread::sleep(Duration::from_millis(200));
    stream.write_all(&body[13..]).expect("second record");
    let reply = Reply::read(&mut stream);
    assert_eq!(reply.lines().len(), 2);
    assert!(sidecar.wait().success());
    let mut rest = String::new();
    sidecar
        .stderr
        .read_to_string(&mut rest)
        .expect("standard error");
    assert_eq!(rest, "moldette-sidecar stopped\n");
}

#[test]
fn fails_on_bad_arguments_and_configuration_before_listening() {
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_moldette-sidecar"))
            .args(args)
            .output()
            .expect("the sidecar runs")
    };
    for args in [
        &[][..],
        &["--threads", "0", CONFIG],
        &["--timeout", "0", CONFIG],
        &["--listen", "localhost", CONFIG],
        &["--records", "multiline", CONFIG],
        &["--max-requests", "0", CONFIG],
        &["--max-buffer-bytes", "0", CONFIG],
        &["--health-listen", "localhost", CONFIG],
        &[CONFIG, CONFIG],
    ] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with("Usage: moldette-sidecar"),
            "{args:?}"
        );
    }
    let missing = run(&["missing.yaml"]);
    assert_eq!(missing.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(stderr.starts_with("error: missing.yaml: "), "{stderr}");
    assert!(!stderr.contains("listening"));
}

#[test]
fn the_last_record_needs_no_line_end() {
    let sidecar = Sidecar::start(&[]);
    let reply = sidecar.post(b"{\"id\":\"A-1\"}\n{\"id\":\"A-2\"}");
    assert_eq!(reply.text(), expected("{\"id\":\"A-1\"}\n{\"id\":\"A-2\"}"));
}

#[test]
fn a_client_reading_while_sending_is_served_with_a_small_buffer() {
    let sidecar = Sidecar::start(&["--max-buffer-bytes", "1"]);
    let input = orders().repeat(4000);
    let reply = sidecar.post_reading_while_sending(input.as_bytes());
    assert_eq!(reply.status, 200);
    assert_eq!(reply.text(), expected(&input));
}

#[test]
fn a_client_reading_after_sending_is_served_while_the_responses_fit_the_buffer() {
    let sidecar = Sidecar::start(&["--max-buffer-bytes", "268435456"]);
    let input = orders().repeat(8000);
    let mut stream = sidecar.connect();
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("write timeout");
    let reply = {
        let mut request = format!(
            "POST /v1/process HTTP/1.1\r\nHost: sidecar\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            input.len()
        )
        .into_bytes();
        request.extend_from_slice(input.as_bytes());
        stream.write_all(&request).expect("the whole body is read");
        Reply::read(&mut stream)
    };
    assert_eq!(reply.text(), expected(&input));
}

#[test]
fn stops_reading_the_body_while_the_response_is_not_read() {
    let sidecar = Sidecar::start(&["--threads", "1", "--max-buffer-bytes", "1048576"]);
    let total = 48 << 20;
    let mut stream = sidecar.connect();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .expect("write timeout");
    stream
        .write_all(
            format!(
                "POST /v1/process HTTP/1.1\r\nHost: sidecar\r\nContent-Length: {total}\r\n\r\n"
            )
            .as_bytes(),
        )
        .expect("request head");
    let chunk = "{\"id\":\"A-1\",\"total\":1}\n".repeat(4096);
    let mut sent = 0;
    while sent < total {
        let end = chunk.len().min(total - sent);
        match stream.write(&chunk.as_bytes()[..end]) {
            Ok(written) => sent += written,
            Err(_) => break,
        }
    }
    assert!(sent < total / 2, "{sent} of {total} bytes were read");
}
