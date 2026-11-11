//! Minimal localhost HTTP server for the editor UI.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use miette::{Result, miette};
use serde_json::json;

use super::{Action, Editor};

const HTML: &str = include_str!("assets/app.html");
const CSS: &str = include_str!("assets/app.css");
const JS: &str = include_str!("assets/app.js");
const MAX_BODY: usize = 8 * 1024 * 1024;

struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
}

/// Start the editor server. `port` 0 asks the OS for a free port.
pub fn serve(dir: &Path, port: u16, no_open: bool) -> Result<()> {
    if !dir.join("main.toml").is_file() {
        return Err(miette!(
            "`{}` is not a gwen project (no main.toml)",
            dir.display()
        ));
    }
    let editor = Arc::new(Mutex::new(Editor::open(dir)?));
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| miette!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| miette!("cannot read the server address: {e}"))?;
    let url = format!("http://127.0.0.1:{}/", addr.port());
    eprintln!("gwen edit: {url}");
    eprintln!("press Ctrl+C to stop");
    if !no_open {
        open_browser(&url);
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let editor = editor.clone();
        std::thread::spawn(move || {
            let _ = handle(stream, &editor);
        });
    }
    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "linux")]
    let command = Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let command = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let command = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let command: std::io::Result<std::process::Child> = Err(std::io::Error::other("unsupported"));
    let _ = command;
}

fn handle(mut stream: TcpStream, editor: &Arc<Mutex<Editor>>) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let request = read_request(&mut stream)?;
    let path = request.path.split('?').next().unwrap_or("/");
    match (request.method.as_str(), path) {
        ("GET", "/") => response(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            HTML.as_bytes(),
        ),
        ("GET", "/assets/app.css") => {
            response(&mut stream, 200, "text/css; charset=utf-8", CSS.as_bytes())
        }
        ("GET", "/assets/app.js") => response(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            JS.as_bytes(),
        ),
        ("GET", "/favicon.ico") => response(&mut stream, 204, "text/plain", b""),
        ("GET", "/api/project") => {
            let body = editor
                .lock()
                .map_err(|_| miette!("editor lock poisoned"))?
                .project_json();
            json_response(&mut stream, 200, &body)
        }
        ("GET", "/api/status") => {
            let result = editor
                .lock()
                .map_err(|_| miette!("editor lock poisoned"))?
                .disk_status();
            result_response(&mut stream, result)
        }
        ("GET", "/api/present") => {
            let body = editor
                .lock()
                .map_err(|_| miette!("editor lock poisoned"))?
                .present_json();
            json_response(&mut stream, 200, &body)
        }
        ("POST", "/api/undo") => {
            let result = editor
                .lock()
                .map_err(|_| miette!("editor lock poisoned"))?
                .undo();
            result_response(&mut stream, result)
        }
        ("POST", "/api/redo") => {
            let result = editor
                .lock()
                .map_err(|_| miette!("editor lock poisoned"))?
                .redo();
            result_response(&mut stream, result)
        }
        ("POST", "/api/reload") => {
            let result = editor
                .lock()
                .map_err(|_| miette!("editor lock poisoned"))?
                .reload_external()
                .map(|project| {
                    json!({
                        "revision": project.get("revision").cloned().unwrap_or(json!(0)),
                        "slide": null,
                        "selection": [],
                        "section": null,
                        "project": project,
                    })
                });
            result_response(&mut stream, result)
        }
        ("POST", "/api/action") => {
            let action: Action = match serde_json::from_slice(&request.body) {
                Ok(action) => action,
                Err(err) => {
                    return error_response(&mut stream, 400, &format!("invalid action: {err}"));
                }
            };
            let result = editor
                .lock()
                .map_err(|_| miette!("editor lock poisoned"))?
                .apply(&action);
            result_response(&mut stream, result)
        }
        _ if path.starts_with("/api/slide/") => {
            let rest = path.trim_start_matches("/api/slide/");
            if let Some(index_text) = rest.strip_suffix(".svg") {
                let index: usize = match index_text.parse() {
                    Ok(index) => index,
                    Err(_) => return error_response(&mut stream, 400, "invalid slide index"),
                };
                let editor = editor.lock().map_err(|_| miette!("editor lock poisoned"))?;
                match editor.svg(index) {
                    Ok(svg) => response(&mut stream, 200, "image/svg+xml", svg.as_bytes()),
                    Err(err) => error_response(&mut stream, 404, &format!("{err:?}")),
                }
            } else {
                let index: usize = match rest.parse() {
                    Ok(index) => index,
                    Err(_) => return error_response(&mut stream, 400, "invalid slide index"),
                };
                let result = editor
                    .lock()
                    .map_err(|_| miette!("editor lock poisoned"))?
                    .slide_json(index);
                result_response(&mut stream, result)
            }
        }
        _ => error_response(&mut stream, 404, "not found"),
    }
}

fn result_response(stream: &mut TcpStream, result: Result<serde_json::Value>) -> Result<()> {
    match result {
        Ok(body) => json_response(stream, 200, &body),
        Err(err) => error_response(stream, 409, &format!("{err:?}")),
    }
}

fn json_response(stream: &mut TcpStream, status: u16, body: &serde_json::Value) -> Result<()> {
    response(
        stream,
        status,
        "application/json; charset=utf-8",
        body.to_string().as_bytes(),
    )
}

fn error_response(stream: &mut TcpStream, status: u16, message: &str) -> Result<()> {
    let body = json!({ "error": message });
    response(
        stream,
        status,
        "application/json; charset=utf-8",
        body.to_string().as_bytes(),
    )
}

fn response(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) -> Result<()> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body))
        .map_err(|e| miette!("cannot write response: {e}"))
}

fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = stream
            .read(&mut buf)
            .map_err(|e| miette!("cannot read request: {e}"))?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        if data.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if data.len() > 64 * 1024 {
            return Err(miette!("request headers are too large"));
        }
    }
    let header_end = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| miette!("malformed request"))?;
    let head = std::str::from_utf8(&data[..header_end]).map_err(|_| miette!("invalid request"))?;
    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| miette!("missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    if length > MAX_BODY {
        return Err(miette!("request body is too large"));
    }
    let mut body = data[header_end + 4..].to_vec();
    while body.len() < length {
        let n = stream
            .read(&mut buf)
            .map_err(|e| miette!("cannot read request body: {e}"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    body.truncate(length);
    Ok(Request { method, path, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn project() -> std::path::PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gwen-edit-server-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("slides")).unwrap();
        std::fs::create_dir_all(dir.join("masters")).unwrap();
        std::fs::write(
            dir.join("main.toml"),
            "[presentation]\ntitle = \"server\"\nwidth = \"10in\"\nheight = \"7.5in\"\n\n[theme]\nmajor_font = \"Arial\"\nminor_font = \"Arial\"\n\n[[sections]]\ntitle = \"S\"\nslides = [\"a.toml\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("masters").join("base.toml"),
            "background = { color = \"FFFFFF\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("slides").join("a.toml"),
            "master = \"base\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nh = \"1in\"\ntext = \"hi\"\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn serves_project_and_slide_json() {
        let dir = project();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let editor = Arc::new(Mutex::new(Editor::open(&dir).unwrap()));
        let worker = editor.clone();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle(stream, &worker).unwrap();
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .write_all(b"GET /api/project HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("application/json"), "{response}");
        assert!(response.contains("\"slides\""), "{response}");
    }
}

#[cfg(test)]
mod reload_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn project() -> std::path::PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gwen-edit-reload-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("slides")).unwrap();
        std::fs::create_dir_all(dir.join("masters")).unwrap();
        std::fs::write(
            dir.join("main.toml"),
            "[presentation]\ntitle = \"server\"\nwidth = \"10in\"\nheight = \"7.5in\"\n\n[theme]\nmajor_font = \"Arial\"\nminor_font = \"Arial\"\n\n[[sections]]\ntitle = \"S\"\nslides = [\"a.toml\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("masters").join("base.toml"),
            "background = { color = \"FFFFFF\" }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("slides").join("a.toml"),
            "master = \"base\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nh = \"1in\"\ntext = \"hi\"\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn reload_returns_the_action_envelope() {
        let dir = project();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let editor = Arc::new(Mutex::new(Editor::open(&dir).unwrap()));
        let worker = editor.clone();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle(stream, &worker).unwrap();
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .write_all(b"POST /api/reload HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("\"project\""), "{response}");
        assert!(response.contains("\"slides\""), "{response}");
        assert!(response.contains("\"sections\""), "{response}");
    }
}
