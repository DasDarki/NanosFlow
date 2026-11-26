use nf_compiler::{BreakpointHit, Debugger, DebuggerEvent};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

pub const DEBUGGER_EVENT: &str = "debugger";

pub struct DebuggerState {
    debugger: Option<Debugger>,
    error: Option<String>,
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

impl DebuggerState {
    pub fn start(app: &AppHandle) -> DebuggerState {
        let emitter = app.clone();
        let started = Debugger::start(nf_compiler::DEBUGGER_PORT, move |event| {
            let message = match event {
                DebuggerEvent::Paused(hit) => DebuggerMessage::Paused { hit },
                DebuggerEvent::Hit(hit) => DebuggerMessage::Hit { hit },
                DebuggerEvent::Resumed => DebuggerMessage::Resumed,
            };
            let _ = emitter.emit(DEBUGGER_EVENT, message);
        });
        match started {
            Ok(debugger) => DebuggerState { debugger: Some(debugger), error: None },
            Err(error) => DebuggerState {
                debugger: None,
                error: Some(format!(
                    "Port {} is already in use, so breakpoints cannot pause. Close the other NanosFlow window or program that uses it. ({error})",
                    nf_compiler::DEBUGGER_PORT
                )),
            },
        }
    }
}

#[tauri::command]
pub fn debugger_status(state: State<'_, DebuggerState>) -> DebuggerStatus {
    DebuggerStatus {
        listening: state.debugger.is_some(),
        port: nf_compiler::DEBUGGER_PORT,
        error: state.error.clone(),
        paused: state.debugger.as_ref().and_then(Debugger::paused),
    }
}

#[tauri::command]
pub fn debugger_resume(state: State<'_, DebuggerState>, mode: nf_compiler::ResumeMode) {
    if let Some(debugger) = &state.debugger {
        debugger.resume(mode);
    }
}
