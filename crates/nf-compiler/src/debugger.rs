use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use nf_project::Id;
use serde::{Deserialize, Serialize};

const HOLD_DURATION: Duration = Duration::from_secs(2);
const STALE_AFTER: Duration = Duration::from_secs(6);
const MAXIMUM_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<Id>,
}

impl StackFrame {
    fn parse(location: String) -> StackFrame {
        let (graph, node) = location
            .split_once('/')
            .map(|(graph, node)| (graph.parse().ok(), node.parse().ok()))
            .unwrap_or((None, None));
        StackFrame { location, graph, node }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResumeMode {
    Continue,
    Into,
    Over,
    Out,
}

impl ResumeMode {
    fn answer(self) -> &'static str {
        match self {
            ResumeMode::Continue => "continue",
            ResumeMode::Into => "into",
            ResumeMode::Over => "over",
            ResumeMode::Out => "out",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointHit {
    pub id: u64,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<Id>,
    pub side: String,
    pub values: Vec<(String, String)>,
    pub stack: Vec<StackFrame>,
    pub can_pause: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebuggerEvent {
    Paused(BreakpointHit),
    Hit(BreakpointHit),
    Resumed,
}

#[derive(Deserialize)]
struct Payload {
    location: String,
    #[serde(default)]
    side: String,
    #[serde(default)]
    values: Vec<(String, String)>,
    #[serde(default)]
    stack: Vec<String>,
}

#[derive(Default)]
struct State {
    paused: Option<BreakpointHit>,
    resume_requested: bool,
    mode: Option<ResumeMode>,
    last_contact: Option<Instant>,
    next_id: u64,
}

type Listener = Arc<dyn Fn(DebuggerEvent) + Send + Sync>;

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    listener: Listener,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Clone)]
pub struct Debugger {
    shared: Arc<Shared>,
    port: u16,
}

struct Request {
    method: String,
    path: String,
    body: String,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.split('?').next()?.to_owned();
    let mut length = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().ok()?;
        }
    }
    if length > MAXIMUM_BODY_BYTES {
        return None;
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn respond(mut stream: &TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

impl Debugger {
    pub fn start(port: u16, listener: impl Fn(DebuggerEvent) + Send + Sync + 'static) -> std::io::Result<Debugger> {
        let socket = TcpListener::bind(("127.0.0.1", port))?;
        let port = socket.local_addr()?.port();
        let debugger = Debugger {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                changed: Condvar::new(),
                listener: Arc::new(listener),
            }),
            port,
        };
        let server = debugger.clone();
        thread::spawn(move || {
            for stream in socket.incoming().filter_map(Result::ok) {
                let connection = server.clone();
                thread::spawn(move || connection.handle(stream));
            }
        });
        Ok(debugger)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn paused(&self) -> Option<BreakpointHit> {
        self.shared.lock().paused.clone()
    }

    pub fn resume(&self, mode: ResumeMode) {
        let mut state = self.shared.lock();
        if state.paused.is_some() {
            state.resume_requested = true;
            state.mode = Some(mode);
            self.shared.changed.notify_all();
        }
        let was_stale = state.paused.is_some() && state.last_contact.is_some_and(|contact| contact.elapsed() > STALE_AFTER);
        if was_stale {
            state.paused = None;
            state.resume_requested = false;
            drop(state);
            (self.shared.listener)(DebuggerEvent::Resumed);
        }
    }

    fn handle(&self, stream: TcpStream) {
        let Some(request) = read_request(&stream) else {
            respond(&stream, 400, "");
            return;
        };
        if request.method != "POST" {
            respond(&stream, 404, "");
            return;
        }
        match request.path.as_str() {
            "/hit" => match self.hit_from(&request.body, false) {
                Some(hit) => {
                    (self.shared.listener)(DebuggerEvent::Hit(hit));
                    respond(&stream, 200, "continue");
                }
                None => respond(&stream, 400, ""),
            },
            "/pause" => match self.hit_from(&request.body, true) {
                Some(hit) => {
                    let (status, body) = self.pause(hit);
                    respond(&stream, status, &body);
                }
                None => respond(&stream, 400, ""),
            },
            "/wait" => {
                let (status, body) = self.wait(request.body.trim().parse().ok());
                respond(&stream, status, &body);
            }
            _ => respond(&stream, 404, ""),
        }
    }

    fn hit_from(&self, body: &str, can_pause: bool) -> Option<BreakpointHit> {
        let payload: Payload = serde_json::from_str(body).ok()?;
        let StackFrame { location, graph, node } = StackFrame::parse(payload.location);
        let mut state = self.shared.lock();
        state.next_id += 1;
        Some(BreakpointHit {
            id: state.next_id,
            location,
            graph,
            node,
            side: payload.side,
            values: payload.values,
            stack: payload.stack.into_iter().map(StackFrame::parse).collect(),
            can_pause,
        })
    }

    fn pause(&self, hit: BreakpointHit) -> (u16, String) {
        {
            let mut state = self.shared.lock();
            let is_busy = state.paused.is_some() && state.last_contact.is_some_and(|contact| contact.elapsed() <= STALE_AFTER);
            if is_busy {
                return (200, "continue".to_owned());
            }
            state.paused = Some(hit.clone());
            state.resume_requested = false;
            state.last_contact = Some(Instant::now());
        }
        (self.shared.listener)(DebuggerEvent::Paused(hit.clone()));
        self.hold(hit.id)
    }

    fn wait(&self, id: Option<u64>) -> (u16, String) {
        let matches = {
            let mut state = self.shared.lock();
            let matches = id.is_some() && state.paused.as_ref().map(|paused| paused.id) == id;
            if matches {
                state.last_contact = Some(Instant::now());
            }
            matches
        };
        match id {
            Some(id) if matches => self.hold(id),
            _ => (200, "continue".to_owned()),
        }
    }

    fn hold(&self, id: u64) -> (u16, String) {
        let state = self.shared.lock();
        let (mut state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, HOLD_DURATION, |state| !state.resume_requested)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.resume_requested {
            let answer = state.mode.take().unwrap_or(ResumeMode::Continue).answer();
            state.paused = None;
            state.resume_requested = false;
            drop(state);
            (self.shared.listener)(DebuggerEvent::Resumed);
            return (200, answer.to_owned());
        }
        state.last_contact = Some(Instant::now());
        (202, id.to_string())
    }
}
