use std::sync::{Mutex, MutexGuard};

use nf_compiler::{BreakpointHit, Debugger, DebuggerEvent};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

pub const DEBUGGER_EVENT: &str = "debugger";

struct Inner {
    debugger: Option<Debugger>,
    error: Option<String>,
    port: u16,
}

pub struct DebuggerState {
    inner: Mutex<Inner>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum DebuggerMessage {
    Paused { hit: BreakpointHit },
    Hit { hit: BreakpointHit },
    Resumed,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebuggerStatus {
    listening: bool,
    port: u16,
    error: Option<String>,
    paused: Option<BreakpointHit>,
}

fn listen(app: &AppHandle, port: u16) -> Inner {
    let emitter = app.clone();
    let started = Debugger::start(port, move |event| {
        let message = match event {
            DebuggerEvent::Paused(hit) => DebuggerMessage::Paused { hit },
            DebuggerEvent::Hit(hit) => DebuggerMessage::Hit { hit },
            DebuggerEvent::Resumed => DebuggerMessage::Resumed,
        };
        let _ = emitter.emit(DEBUGGER_EVENT, message);
    });
    nf_compiler::set_debugger_port(port);
    match started {
        Ok(debugger) => Inner { debugger: Some(debugger), error: None, port },
        Err(error) => Inner {
            debugger: None,
            error: Some(format!(
                "Port {port} is already in use, so breakpoints cannot pause. Pick another debugger port in Settings, then build again. ({error})"
            )),
            port,
        },
    }
}

impl DebuggerState {
    pub fn start(app: &AppHandle, port: u16) -> DebuggerState {
        DebuggerState {
            inner: Mutex::new(listen(app, port)),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn restart(&self, app: &AppHandle, port: u16) {
        let mut inner = self.lock();
        if inner.port == port && inner.debugger.is_some() {
            return;
        }
        if let Some(debugger) = inner.debugger.take() {
            debugger.stop();
        }
        *inner = listen(app, port);
    }

    pub fn status(&self) -> DebuggerStatus {
        let inner = self.lock();
        DebuggerStatus {
            listening: inner.debugger.is_some(),
            port: inner.port,
            error: inner.error.clone(),
            paused: inner.debugger.as_ref().and_then(Debugger::paused),
        }
    }
}

#[tauri::command]
pub fn debugger_status(state: State<'_, DebuggerState>) -> DebuggerStatus {
    state.status()
}

#[tauri::command]
pub fn debugger_resume(state: State<'_, DebuggerState>, mode: nf_compiler::ResumeMode) {
    if let Some(debugger) = &state.lock().debugger {
        debugger.resume(mode);
    }
}
