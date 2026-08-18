use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use serde_json::{json, Value};

use crate::service::ManagedChild;

#[derive(Debug)]
pub enum BackendCommand {
    Prompt(String),
    Interrupt,
    NewConversation,
    Login,
    Approval { id: Value, accept: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendEvent {
    Starting,
    Missing,
    Ready { authenticated: bool },
    LoginCode { url: String, code: String },
    TurnStarted,
    Delta(String),
    Activity(String),
    Approval { id: Value, detail: String },
    Diff(String),
    Completed(String),
    Error(String),
}

pub struct AgentBackend {
    commands: Sender<BackendCommand>,
    events: Receiver<BackendEvent>,
}

impl AgentBackend {
    pub fn start(workspace: PathBuf) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        thread::Builder::new()
            .name("tted-codex-backend".into())
            .spawn(move || run_codex(workspace, command_rx, event_tx))
            .expect("spawn Codex backend worker");
        Self { commands, events }
    }

    pub fn send(&self, command: BackendCommand) {
        let _ = self.commands.send(command);
    }

    pub fn try_recv(&self) -> Option<BackendEvent> {
        self.events.try_recv().ok()
    }
}

fn send(stdin: &mut impl Write, value: Value) -> bool {
    serde_json::to_writer(&mut *stdin, &value).is_ok()
        && stdin.write_all(b"\n").is_ok()
        && stdin.flush().is_ok()
}

fn run_codex(workspace: PathBuf, commands: Receiver<BackendCommand>, events: Sender<BackendEvent>) {
    let _ = events.send(BackendEvent::Starting);
    if Command::new("codex").arg("--version").output().is_err() {
        let _ = events.send(BackendEvent::Missing);
        return;
    }
    let mut process = Command::new("codex");
    process
        .args(["app-server", "--stdio"])
        .current_dir(&workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let Ok(mut child) = ManagedChild::spawn(&mut process) else {
        let _ = events.send(BackendEvent::Missing);
        return;
    };
    let Some(mut stdin) = child.child_mut().stdin.take() else {
        let _ = events.send(BackendEvent::Error("Codex input unavailable".into()));
        return;
    };
    let Some(stdout) = child.child_mut().stdout.take() else {
        let _ = events.send(BackendEvent::Error("Codex output unavailable".into()));
        return;
    };
    let (lines_tx, lines_rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if lines_tx.send(value).is_err() {
                    break;
                }
            }
        }
    });

    send(
        &mut stdin,
        json!({"method":"initialize","id":1,"params":{"clientInfo":{
            "name":"tted","title":"TTED","version":env!("CARGO_PKG_VERSION")
        }}}),
    );
    send(&mut stdin, json!({"method":"initialized","params":{}}));
    send(
        &mut stdin,
        json!({"method":"account/read","id":2,"params":{"refreshToken":false}}),
    );
    send(
        &mut stdin,
        json!({"method":"thread/start","id":3,"params":{"cwd":workspace}}),
    );

    let mut thread_id = None::<String>;
    let mut turn_id = None::<String>;
    let mut queued_prompt = None::<String>;
    let mut next_id = 10_u64;
    loop {
        while let Ok(message) = lines_rx.try_recv() {
            handle_message(
                &message,
                &events,
                &mut thread_id,
                &mut turn_id,
                &mut queued_prompt,
                &mut stdin,
                &workspace,
                &mut next_id,
            );
        }
        match commands.recv_timeout(Duration::from_millis(20)) {
            Ok(BackendCommand::Prompt(prompt)) => {
                if let Some(thread_id) = &thread_id {
                    start_turn(&mut stdin, &workspace, thread_id, prompt, &mut next_id);
                } else {
                    queued_prompt = Some(prompt);
                }
            }
            Ok(BackendCommand::Interrupt) => {
                if let (Some(thread_id), Some(turn_id)) = (&thread_id, &turn_id) {
                    send(
                        &mut stdin,
                        json!({"method":"turn/interrupt","id":next_id,"params":{
                            "threadId":thread_id,"turnId":turn_id
                        }}),
                    );
                    next_id += 1;
                }
            }
            Ok(BackendCommand::NewConversation) => {
                thread_id = None;
                turn_id = None;
                send(
                    &mut stdin,
                    json!({"method":"thread/start","id":3,"params":{"cwd":workspace}}),
                );
            }
            Ok(BackendCommand::Login) => {
                send(
                    &mut stdin,
                    json!({"method":"account/login/start","id":4,"params":{
                        "type":"chatgptDeviceCode"
                    }}),
                );
            }
            Ok(BackendCommand::Approval { id, accept }) => {
                send(
                    &mut stdin,
                    json!({"id":id,"result":{"decision":if accept { "accept" } else { "decline" }}}),
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if child.child_mut().try_wait().ok().flatten().is_some() {
            let _ = events.send(BackendEvent::Error("Codex stopped".into()));
            break;
        }
    }
}

fn start_turn(
    stdin: &mut impl Write,
    workspace: &PathBuf,
    thread_id: &str,
    prompt: String,
    next_id: &mut u64,
) {
    send(
        stdin,
        json!({"method":"turn/start","id":*next_id,"params":{
            "threadId":thread_id,
            "input":[{"type":"text","text":prompt}],
            "cwd":workspace,
            "approvalPolicy":"on-request",
            "sandboxPolicy":{
                "type":"workspaceWrite",
                "writableRoots":[workspace],
                "networkAccess":false
            }
        }}),
    );
    *next_id += 1;
}

#[allow(clippy::too_many_arguments)]
fn handle_message(
    message: &Value,
    events: &Sender<BackendEvent>,
    thread_id: &mut Option<String>,
    turn_id: &mut Option<String>,
    queued_prompt: &mut Option<String>,
    stdin: &mut impl Write,
    workspace: &PathBuf,
    next_id: &mut u64,
) {
    if message.get("id") == Some(&json!(2)) {
        let authenticated = message
            .pointer("/result/account")
            .is_some_and(|account| !account.is_null())
            || message.pointer("/result/requiresOpenaiAuth") == Some(&Value::Bool(false));
        let _ = events.send(BackendEvent::Ready { authenticated });
    }
    if message.get("id") == Some(&json!(3)) {
        *thread_id = message
            .pointer("/result/thread/id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let (Some(id), Some(prompt)) = (thread_id.as_deref(), queued_prompt.take()) {
            start_turn(stdin, workspace, id, prompt, next_id);
        }
    }
    if message.get("id") == Some(&json!(4)) {
        if let (Some(url), Some(code)) = (
            message
                .pointer("/result/verificationUrl")
                .and_then(Value::as_str),
            message.pointer("/result/userCode").and_then(Value::as_str),
        ) {
            let _ = events.send(BackendEvent::LoginCode {
                url: url.into(),
                code: code.into(),
            });
        }
    }
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        if let Some(error) = message.pointer("/error/message").and_then(Value::as_str) {
            let _ = events.send(BackendEvent::Error(error.into()));
        }
        return;
    };
    if let Some(id) = message.get("id") {
        let detail = match method {
            "item/commandExecution/requestApproval" => message
                .pointer("/params/command")
                .map(display_approval_value)
                .unwrap_or_else(|| "run a workspace command".into()),
            "item/fileChange/requestApproval" => message
                .pointer("/params/reason")
                .and_then(Value::as_str)
                .unwrap_or("edit workspace files")
                .to_owned(),
            _ => String::new(),
        };
        if !detail.is_empty() {
            let _ = events.send(BackendEvent::Approval {
                id: id.clone(),
                detail,
            });
            return;
        }
    }
    match method {
        "turn/started" => {
            *turn_id = message
                .pointer("/params/turn/id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let _ = events.send(BackendEvent::TurnStarted);
        }
        "item/agentMessage/delta" => {
            if let Some(delta) = message.pointer("/params/delta").and_then(Value::as_str) {
                let _ = events.send(BackendEvent::Delta(delta.into()));
            }
        }
        "turn/diff/updated" => {
            if let Some(diff) = message.pointer("/params/diff").and_then(Value::as_str) {
                let _ = events.send(BackendEvent::Diff(diff.into()));
            }
        }
        "item/started" => {
            if let Some(kind) = message.pointer("/params/item/type").and_then(Value::as_str) {
                let label = match kind {
                    "commandExecution" => "Running a workspace command",
                    "fileChange" => "Editing workspace files",
                    "webSearch" => "Searching the web",
                    _ => return,
                };
                let _ = events.send(BackendEvent::Activity(label.into()));
            }
        }
        "turn/completed" => {
            *turn_id = None;
            let status = message
                .pointer("/params/turn/status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            let _ = events.send(BackendEvent::Completed(status.into()));
        }
        "account/login/completed" => {
            let success = message
                .pointer("/params/success")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let _ = events.send(if success {
                send(
                    stdin,
                    json!({"method":"thread/start","id":3,"params":{"cwd":workspace}}),
                );
                BackendEvent::Ready {
                    authenticated: true,
                }
            } else {
                BackendEvent::Error("Codex sign-in failed".into())
            });
        }
        "error" => {
            let message = message
                .pointer("/params/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Codex error");
            let _ = events.send(BackendEvent::Error(message.into()));
        }
        _ => {}
    }
}

fn display_approval_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_streamed_agent_events() {
        let (events, receiver) = mpsc::channel();
        let mut thread = None;
        let mut turn = None;
        let mut queued = None;
        let mut output = Vec::new();
        let mut id = 10;
        handle_message(
            &json!({"method":"item/agentMessage/delta","params":{"delta":"hello"}}),
            &events,
            &mut thread,
            &mut turn,
            &mut queued,
            &mut output,
            &PathBuf::from("."),
            &mut id,
        );
        assert_eq!(
            receiver.recv().unwrap(),
            BackendEvent::Delta("hello".into())
        );
    }

    #[test]
    fn parses_command_approval_requests() {
        let (events, receiver) = mpsc::channel();
        let mut thread = None;
        let mut turn = None;
        let mut queued = None;
        let mut output = Vec::new();
        let mut next_id = 10;
        handle_message(
            &json!({"id":42,"method":"item/commandExecution/requestApproval","params":{"command":["cargo","test"]}}),
            &events,
            &mut thread,
            &mut turn,
            &mut queued,
            &mut output,
            &PathBuf::from("."),
            &mut next_id,
        );
        assert_eq!(
            receiver.recv().unwrap(),
            BackendEvent::Approval {
                id: json!(42),
                detail: "cargo test".into()
            }
        );
    }

    #[test]
    fn turn_uses_supported_interactive_approval_policy() {
        let mut output = Vec::new();
        let mut next_id = 10;
        start_turn(
            &mut output,
            &PathBuf::from("/workspace"),
            "thread-1",
            "help".into(),
            &mut next_id,
        );
        let request: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            request.pointer("/params/approvalPolicy"),
            Some(&json!("on-request"))
        );
    }
}
