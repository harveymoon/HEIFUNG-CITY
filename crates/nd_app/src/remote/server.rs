//! The HTTP side of the remote API: a `tiny_http` thread that accepts
//! requests, checks who is asking, reads a bounded body and parks each
//! command, with a reply channel, in the queue `remote_commands` drains.

use std::io::Read;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::prelude::*;
use serde_json::{json, Value};

use super::guard;

/// How long the HTTP thread waits for the main thread to answer. A frame
/// normally takes milliseconds; a long `step` or a stalled window can take
/// longer, and the caller gets a 504 rather than hanging forever.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for a command the main thread has already started
/// (a `step` of many days can run for a minute).
const LONG_REPLY_TIMEOUT: Duration = Duration::from_secs(300);

/// One parked request: the parsed command, where the reply goes, and
/// whether it is still wanted ([`QUEUED`] until the main thread takes it,
/// [`CANCELLED`] if the HTTP side gave up first).
pub(super) struct Pending {
    pub(super) cmd: Value,
    pub(super) reply: Sender<Reply>,
    pub(super) state: Arc<AtomicU8>,
}

pub(super) const QUEUED: u8 = 0;
pub(super) const TAKEN: u8 = 1;
const CANCELLED: u8 = 2;

pub(super) struct Reply {
    pub(super) status: u16,
    pub(super) body: Value,
}

/// `Receiver` is `Send` but not `Sync`; a resource must be both, so the
/// mutex makes the compiler happy. Only `remote_commands` ever locks it,
/// and only for the length of a non-blocking drain.
#[derive(Resource)]
pub(super) struct Inbox(pub(super) Mutex<Receiver<Pending>>);

pub(super) fn serve(server: tiny_http::Server, tx: Sender<Pending>, port: u16) {
    let in_flight = Arc::new(AtomicUsize::new(0));
    for req in server.incoming_requests() {
        // One thread per request, so a client that trickles its body holds
        // up only itself; capped, so a flood is answered 503 at once.
        if in_flight.fetch_add(1, Ordering::SeqCst) >= guard::MAX_IN_FLIGHT {
            in_flight.fetch_sub(1, Ordering::SeqCst);
            respond(req, Reply { status: 503, body: json!({ "ok": false, "error": "too many requests at once; retry" }) });
            continue;
        }
        let tx = tx.clone();
        // [fix/review-app] The slot goes back however the thread ends: a
        // panic in `handle` used to keep it, and after 16 of them every
        // request got a 503. (A failed spawn drops the closure, and the
        // guard with it.)
        let slot = Slot(in_flight.clone());
        let _ = std::thread::Builder::new().name("nd-remote-req".into()).spawn(move || {
            let _slot = slot;
            handle(req, &tx, port);
        });
    }
}

/// One of the `MAX_IN_FLIGHT` request slots, given back on drop.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn respond(req: tiny_http::Request, reply: Reply) {
    let text = reply.body.to_string();
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).expect("static header");
    let _ = req.respond(tiny_http::Response::from_string(text).with_status_code(reply.status).with_header(header));
}

/// One request, on its own thread: check who is asking (before reading the
/// body), read a bounded body, queue the command and wait for its reply.
fn handle(mut req: tiny_http::Request, tx: &Sender<Pending>, port: u16) {
    // Owned copies: reading the body borrows the request mutably.
    let method = req.method().as_str().to_string();
    let url = req.url().split('?').next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> = req.headers().iter().map(|h| (h.field.as_str().as_str().to_string(), h.value.as_str().to_string())).collect();
    if let Err((status, error)) = guard::check_headers(&method, &headers, port) {
        return respond(req, Reply { status, body: json!({ "ok": false, "error": error }) });
    }
    let cmd = match (method.as_str(), url.as_str()) {
        ("GET", "/status") | ("GET", "/status/") => Ok(json!({ "cmd": "status" })),
        ("GET", "/help") => Ok(json!({ "cmd": "help" })),
        ("POST", "/cmd") | ("POST", "/cmd/") | ("POST", "/") => read_body(&mut req),
        _ => Err((404, "use POST /cmd with a JSON body {\"cmd\": ...}, or GET /status".to_string())),
    };
    let reply = match cmd {
        Err((status, error)) => Reply { status, body: json!({ "ok": false, "error": error }) },
        Ok(cmd) => {
            let (reply_tx, reply_rx) = mpsc::channel();
            let state = Arc::new(AtomicU8::new(QUEUED));
            if tx.send(Pending { cmd, reply: reply_tx, state: state.clone() }).is_err() {
                // The app is shutting down; the receiver is gone.
                return;
            }
            match reply_rx.recv_timeout(REPLY_TIMEOUT) {
                Ok(r) => r,
                // Not started yet: withdraw it, so the caller can retry
                // without the command running twice.
                Err(_) if state.compare_exchange(QUEUED, CANCELLED, Ordering::SeqCst, Ordering::SeqCst).is_ok() => {
                    Reply { status: 504, body: json!({ "ok": false, "error": "the game did not answer in time (frame stalled or window closing); the command was withdrawn and did not run" }) }
                }
                // Already running (a long `step`): its answer is coming.
                Err(_) => match reply_rx.recv_timeout(LONG_REPLY_TIMEOUT) {
                    Ok(r) => r,
                    Err(_) => Reply { status: 504, body: json!({ "ok": false, "error": "the command started but did not finish in time; it may still take effect" }) },
                },
            }
        }
    };
    respond(req, reply);
}

/// The body as JSON, at most [`guard::MAX_BODY`] bytes.
fn read_body(req: &mut tiny_http::Request) -> Result<Value, (u16, String)> {
    let too_big = || (413, format!("body larger than {} bytes", guard::MAX_BODY));
    if req.body_length().is_some_and(|n| n > guard::MAX_BODY) {
        return Err(too_big());
    }
    let mut body = Vec::new();
    req.as_reader().take(guard::MAX_BODY as u64 + 1).read_to_end(&mut body).map_err(|e| (400, format!("could not read body: {e}")))?;
    if body.len() > guard::MAX_BODY {
        return Err(too_big());
    }
    serde_json::from_slice::<Value>(&body).map_err(|e| (400, format!("body is not JSON: {e}")))
}
