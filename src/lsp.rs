use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    diagnostics,
    service::{BackgroundService, ManagedChild, ServiceContext, ServiceEvent},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LanguageServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub language_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub severity: u8,
    pub message: String,
}

pub type TextEdit = (usize, usize, usize, usize, String);

#[derive(Clone, Debug)]
pub enum LspEvent {
    Ready,
    Diagnostics {
        path: PathBuf,
        items: Vec<Diagnostic>,
    },
    Hover(String),
    Definition {
        path: PathBuf,
        line: usize,
        column: usize,
    },
    Completions(Vec<String>),
    Locations(Vec<(PathBuf, usize, usize)>),
    WorkspaceEdits(Vec<(PathBuf, Vec<TextEdit>)>),
    Information(String),
}

enum LspCommand {
    Open {
        path: PathBuf,
        text: String,
    },
    Change {
        path: PathBuf,
        version: i64,
        text: String,
    },
    Save {
        path: PathBuf,
    },
    Hover {
        path: PathBuf,
        line: usize,
        column: usize,
    },
    Definition {
        path: PathBuf,
        line: usize,
        column: usize,
    },
    Completion {
        path: PathBuf,
        line: usize,
        column: usize,
    },
    References {
        path: PathBuf,
        line: usize,
        column: usize,
    },
    Rename {
        path: PathBuf,
        line: usize,
        column: usize,
        new_name: String,
    },
    CodeActions {
        path: PathBuf,
        line: usize,
        column: usize,
    },
    Formatting {
        path: PathBuf,
    },
    DocumentSymbols {
        path: PathBuf,
    },
    WorkspaceSymbols {
        query: String,
    },
    Signature {
        path: PathBuf,
        line: usize,
        column: usize,
    },
}

#[derive(Clone)]
enum RequestKind {
    Initialize,
    Hover,
    Definition,
    Completion,
    References,
    Rename,
    CodeActions,
    Formatting(PathBuf),
    DocumentSymbols,
    WorkspaceSymbols,
    Signature,
}

pub struct LspService {
    worker: BackgroundService<LspCommand, LspEvent>,
    diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    status: String,
}

impl LspService {
    pub fn start(root: PathBuf, config: LanguageServerConfig) -> io::Result<Self> {
        let worker = BackgroundService::spawn("tted-lsp", move |commands, context| {
            if let Err(error) = run_server(root, config, commands, &context) {
                diagnostics::log(&format!("LSP error: {error}"));
                context.error(error.to_string());
            }
        })?;
        Ok(Self {
            worker,
            diagnostics: HashMap::new(),
            status: "starting".into(),
        })
    }

    pub fn open(&self, path: PathBuf, text: String) {
        let _ = self.worker.send(LspCommand::Open { path, text });
    }
    pub fn change(&self, path: PathBuf, version: i64, text: String) {
        let _ = self.worker.send(LspCommand::Change {
            path,
            version,
            text,
        });
    }
    pub fn save(&self, path: PathBuf) {
        let _ = self.worker.send(LspCommand::Save { path });
    }
    pub fn hover(&self, path: PathBuf, line: usize, column: usize) {
        let _ = self.worker.send(LspCommand::Hover { path, line, column });
    }
    pub fn definition(&self, path: PathBuf, line: usize, column: usize) {
        let _ = self
            .worker
            .send(LspCommand::Definition { path, line, column });
    }
    pub fn completion(&self, path: PathBuf, line: usize, column: usize) {
        let _ = self
            .worker
            .send(LspCommand::Completion { path, line, column });
    }
    pub fn references(&self, path: PathBuf, line: usize, column: usize) {
        let _ = self
            .worker
            .send(LspCommand::References { path, line, column });
    }
    pub fn rename(&self, path: PathBuf, line: usize, column: usize, new_name: String) {
        let _ = self.worker.send(LspCommand::Rename {
            path,
            line,
            column,
            new_name,
        });
    }
    pub fn code_actions(&self, path: PathBuf, line: usize, column: usize) {
        let _ = self
            .worker
            .send(LspCommand::CodeActions { path, line, column });
    }
    pub fn formatting(&self, path: PathBuf) {
        let _ = self.worker.send(LspCommand::Formatting { path });
    }
    pub fn document_symbols(&self, path: PathBuf) {
        let _ = self.worker.send(LspCommand::DocumentSymbols { path });
    }
    pub fn workspace_symbols(&self, query: String) {
        let _ = self.worker.send(LspCommand::WorkspaceSymbols { query });
    }
    pub fn signature(&self, path: PathBuf, line: usize, column: usize) {
        let _ = self
            .worker
            .send(LspCommand::Signature { path, line, column });
    }
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.values().flatten()
    }
    pub fn diagnostic_at(&self, path: &Path, line: usize) -> Option<&Diagnostic> {
        self.diagnostics
            .get(path)?
            .iter()
            .find(|item| item.line == line)
    }
    pub fn diagnostic_count(&self, path: &Path) -> usize {
        self.diagnostics.get(path).map_or(0, Vec::len)
    }
    pub fn all_diagnostics(&self) -> Vec<Diagnostic> {
        let mut items = self.diagnostics().cloned().collect::<Vec<_>>();
        items.sort_by(|a, b| (&a.path, a.line, a.column).cmp(&(&b.path, b.line, b.column)));
        items
    }
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn poll(&mut self) -> Vec<LspEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.worker.try_recv() {
            match event {
                ServiceEvent::Item(LspEvent::Diagnostics { path, items }) => {
                    self.diagnostics.insert(path.clone(), items.clone());
                    events.push(LspEvent::Diagnostics { path, items });
                }
                ServiceEvent::Item(item) => {
                    if matches!(item, LspEvent::Ready) {
                        self.status = "ready".into();
                    }
                    events.push(item);
                }
                ServiceEvent::Error(error) => self.status = format!("error: {error}"),
                ServiceEvent::Finished => self.status = "stopped".into(),
                ServiceEvent::Output(output) => diagnostics::log(&format!("LSP: {output}")),
            }
        }
        events
    }
}

fn run_server(
    root: PathBuf,
    config: LanguageServerConfig,
    commands: mpsc::Receiver<LspCommand>,
    context: &ServiceContext<LspEvent>,
) -> io::Result<()> {
    let mut command = Command::new(&config.command);
    command
        .args(&config.args)
        .current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = ManagedChild::spawn(&mut command)?;
    let stdin = child
        .child_mut()
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("LSP stdin unavailable"))?;
    let stdout = child
        .child_mut()
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("LSP stdout unavailable"))?;
    let (incoming_tx, incoming_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        while let Ok(Some(value)) = read_message(&mut reader) {
            if incoming_tx.send(value).is_err() {
                break;
            }
        }
    });
    let mut writer = stdin;
    let mut next_id = 1_u64;
    let mut pending = HashMap::new();
    let root_uri = path_uri(&root);
    send_request(
        &mut writer,
        next_id,
        "initialize",
        json!({"processId": std::process::id(), "rootUri": root_uri, "capabilities": {"textDocument": {"publishDiagnostics": {}, "hover": {}, "definition": {}, "completion": {}}}}),
    )?;
    pending.insert(next_id, RequestKind::Initialize);
    next_id += 1;
    while !context.cancellation().is_cancelled() {
        while let Ok(value) = incoming_rx.try_recv() {
            handle_message(value, &mut writer, &mut pending, context)?;
        }
        match commands.recv_timeout(Duration::from_millis(20)) {
            Ok(command) => handle_command(
                command,
                &config.language_id,
                &mut writer,
                &mut next_id,
                &mut pending,
            )?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if child.child_mut().try_wait()?.is_some() {
            return Err(io::Error::other("language server exited"));
        }
    }
    send_request(&mut writer, next_id, "shutdown", json!(null))?;
    send_notification(&mut writer, "exit", json!(null))?;
    Ok(())
}

fn handle_command(
    command: LspCommand,
    language_id: &str,
    writer: &mut impl Write,
    next_id: &mut u64,
    pending: &mut HashMap<u64, RequestKind>,
) -> io::Result<()> {
    let position = |path: &Path, line, column| json!({"textDocument":{"uri":path_uri(path)},"position":{"line":line,"character":column}});
    match command {
        LspCommand::Open { path, text } => send_notification(
            writer,
            "textDocument/didOpen",
            json!({"textDocument":{"uri":path_uri(&path),"languageId":language_id,"version":1,"text":text}}),
        ),
        LspCommand::Change {
            path,
            version,
            text,
        } => send_notification(
            writer,
            "textDocument/didChange",
            json!({"textDocument":{"uri":path_uri(&path),"version":version},"contentChanges":[{"text":text}]}),
        ),
        LspCommand::Save { path } => send_notification(
            writer,
            "textDocument/didSave",
            json!({"textDocument":{"uri":path_uri(&path)}}),
        ),
        LspCommand::Hover { path, line, column } => request(
            writer,
            next_id,
            pending,
            RequestKind::Hover,
            "textDocument/hover",
            position(&path, line, column),
        ),
        LspCommand::Definition { path, line, column } => request(
            writer,
            next_id,
            pending,
            RequestKind::Definition,
            "textDocument/definition",
            position(&path, line, column),
        ),
        LspCommand::Completion { path, line, column } => request(
            writer,
            next_id,
            pending,
            RequestKind::Completion,
            "textDocument/completion",
            position(&path, line, column),
        ),
        LspCommand::References { path, line, column } => request(
            writer,
            next_id,
            pending,
            RequestKind::References,
            "textDocument/references",
            json!({"textDocument":{"uri":path_uri(&path)},"position":{"line":line,"character":column},"context":{"includeDeclaration":true}}),
        ),
        LspCommand::Rename {
            path,
            line,
            column,
            new_name,
        } => request(
            writer,
            next_id,
            pending,
            RequestKind::Rename,
            "textDocument/rename",
            json!({"textDocument":{"uri":path_uri(&path)},"position":{"line":line,"character":column},"newName":new_name}),
        ),
        LspCommand::CodeActions { path, line, column } => request(
            writer,
            next_id,
            pending,
            RequestKind::CodeActions,
            "textDocument/codeAction",
            json!({"textDocument":{"uri":path_uri(&path)},"range":{"start":{"line":line,"character":column},"end":{"line":line,"character":column}},"context":{"diagnostics":[]}}),
        ),
        LspCommand::Formatting { path } => request(
            writer,
            next_id,
            pending,
            RequestKind::Formatting(path.clone()),
            "textDocument/formatting",
            json!({"textDocument":{"uri":path_uri(&path)},"options":{"tabSize":4,"insertSpaces":true}}),
        ),
        LspCommand::DocumentSymbols { path } => request(
            writer,
            next_id,
            pending,
            RequestKind::DocumentSymbols,
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":path_uri(&path)}}),
        ),
        LspCommand::WorkspaceSymbols { query } => request(
            writer,
            next_id,
            pending,
            RequestKind::WorkspaceSymbols,
            "workspace/symbol",
            json!({"query":query}),
        ),
        LspCommand::Signature { path, line, column } => request(
            writer,
            next_id,
            pending,
            RequestKind::Signature,
            "textDocument/signatureHelp",
            position(&path, line, column),
        ),
    }
}

fn request(
    writer: &mut impl Write,
    next_id: &mut u64,
    pending: &mut HashMap<u64, RequestKind>,
    kind: RequestKind,
    method: &str,
    params: Value,
) -> io::Result<()> {
    send_request(writer, *next_id, method, params)?;
    pending.insert(*next_id, kind);
    *next_id += 1;
    Ok(())
}

fn handle_message(
    value: Value,
    writer: &mut impl Write,
    pending: &mut HashMap<u64, RequestKind>,
    context: &ServiceContext<LspEvent>,
) -> io::Result<()> {
    if value.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") {
        let params = &value["params"];
        let Some(path) = params["uri"].as_str().and_then(uri_path) else {
            return Ok(());
        };
        let items = params["diagnostics"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                Some(Diagnostic {
                    path: path.clone(),
                    line: item["range"]["start"]["line"].as_u64()? as usize,
                    column: item["range"]["start"]["character"].as_u64()? as usize,
                    severity: item["severity"].as_u64().unwrap_or(3) as u8,
                    message: item["message"].as_str()?.to_owned(),
                })
            })
            .collect();
        context.emit(LspEvent::Diagnostics { path, items });
    } else if let Some(id) = value.get("id").and_then(Value::as_u64) {
        match pending.remove(&id) {
            Some(RequestKind::Initialize) => {
                send_notification(writer, "initialized", json!({}))?;
                context.emit(LspEvent::Ready);
            }
            Some(RequestKind::Hover) => {
                context.emit(LspEvent::Hover(extract_text(&value["result"])));
            }
            Some(RequestKind::Definition) => {
                if let Some((path, line, column)) = extract_location(&value["result"]) {
                    context.emit(LspEvent::Definition { path, line, column });
                } else {
                    context.output("No definition found");
                }
            }
            Some(RequestKind::Completion) => {
                context.emit(LspEvent::Completions(extract_completions(&value["result"])));
            }
            Some(RequestKind::References) => {
                context.emit(LspEvent::Locations(extract_locations(&value["result"])));
            }
            Some(RequestKind::Rename) => {
                context.emit(LspEvent::WorkspaceEdits(extract_workspace_edits(
                    &value["result"],
                )));
            }
            Some(RequestKind::Formatting(path)) => {
                context.emit(LspEvent::WorkspaceEdits(vec![(
                    path.clone(),
                    extract_text_edits(&value["result"]),
                )]));
            }
            Some(RequestKind::CodeActions) => {
                context.emit(LspEvent::Information(format_items(
                    "Code actions",
                    &value["result"],
                )));
            }
            Some(RequestKind::DocumentSymbols) => {
                context.emit(LspEvent::Information(format_items(
                    "Document symbols",
                    &value["result"],
                )));
            }
            Some(RequestKind::WorkspaceSymbols) => {
                context.emit(LspEvent::Information(format_items(
                    "Workspace symbols",
                    &value["result"],
                )));
            }
            Some(RequestKind::Signature) => {
                context.emit(LspEvent::Information(format_items(
                    "Signature help",
                    &value["result"],
                )));
            }
            None => {}
        };
    }
    Ok(())
}

fn send_request(writer: &mut impl Write, id: u64, method: &str, params: Value) -> io::Result<()> {
    send_value(
        writer,
        &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )
}
fn send_notification(writer: &mut impl Write, method: &str, params: Value) -> io::Result<()> {
    send_value(
        writer,
        &json!({"jsonrpc":"2.0","method":method,"params":params}),
    )
}
fn send_value(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = length else {
        return Err(io::Error::other("missing LSP Content-Length"));
    };
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(io::Error::other)
}

fn path_uri(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy().replace(' ', "%20"))
}
fn uri_path(uri: &str) -> Option<PathBuf> {
    uri.strip_prefix("file://")
        .map(|path| PathBuf::from(path.replace("%20", " ")))
}
fn extract_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(extract_text)
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => map
            .get("value")
            .or_else(|| map.get("contents"))
            .map(extract_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}
fn extract_location(value: &Value) -> Option<(PathBuf, usize, usize)> {
    let value = value.as_array().and_then(|v| v.first()).unwrap_or(value);
    Some((
        uri_path(value["uri"].as_str()?)?,
        value["range"]["start"]["line"].as_u64()? as usize,
        value["range"]["start"]["character"].as_u64()? as usize,
    ))
}
fn extract_completions(value: &Value) -> Vec<String> {
    let items = value.get("items").unwrap_or(value);
    items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("label").and_then(Value::as_str).map(str::to_owned))
        .take(100)
        .collect()
}

fn extract_locations(value: &Value) -> Vec<(PathBuf, usize, usize)> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(extract_location)
        .collect()
}

fn extract_text_edits(value: &Value) -> Vec<TextEdit> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|edit| {
            Some((
                edit["range"]["start"]["line"].as_u64()? as usize,
                edit["range"]["start"]["character"].as_u64()? as usize,
                edit["range"]["end"]["line"].as_u64()? as usize,
                edit["range"]["end"]["character"].as_u64()? as usize,
                edit["newText"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn extract_workspace_edits(value: &Value) -> Vec<(PathBuf, Vec<TextEdit>)> {
    value
        .get("changes")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(uri, edits)| Some((uri_path(uri)?, extract_text_edits(edits))))
        .collect()
}

fn format_items(title: &str, value: &Value) -> String {
    let mut labels = Vec::new();
    collect_labels(value, &mut labels);
    if labels.is_empty() {
        format!("{title}\n\nNo results")
    } else {
        format!("{title}\n\n{}", labels.join("\n"))
    }
}

fn collect_labels(value: &Value, labels: &mut Vec<String>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| collect_labels(item, labels)),
        Value::Object(map) => {
            if let Some(label) = map
                .get("title")
                .or_else(|| map.get("name"))
                .or_else(|| map.get("label"))
                .and_then(Value::as_str)
            {
                labels.push(label.to_owned());
            } else {
                map.values().for_each(|item| collect_labels(item, labels));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framed_json_round_trip() {
        let mut bytes = Vec::new();
        send_notification(&mut bytes, "ready", json!({"yes":true})).unwrap();
        let parsed = read_message(&mut BufReader::new(bytes.as_slice()))
            .unwrap()
            .unwrap();
        assert_eq!(parsed["method"], "ready");
    }
    #[test]
    fn parses_diagnostic_notification() {
        let (tx, rx) = mpsc::channel();
        let context = ServiceContext {
            events: tx,
            cancellation: Default::default(),
        };
        let value = json!({"method":"textDocument/publishDiagnostics","params":{"uri":"file:///tmp/a.rs","diagnostics":[{"range":{"start":{"line":2,"character":4}},"severity":1,"message":"broken"}]}});
        handle_message(value, &mut Vec::new(), &mut HashMap::new(), &context).unwrap();
        let ServiceEvent::Item(LspEvent::Diagnostics { items, .. }) = rx.recv().unwrap() else {
            panic!()
        };
        assert_eq!(items[0].message, "broken");
    }

    #[test]
    fn parses_locations_and_workspace_edits() {
        let locations = extract_locations(
            &json!([{"uri":"file:///tmp/a.rs","range":{"start":{"line":3,"character":2}}}]),
        );
        assert_eq!(locations, vec![(PathBuf::from("/tmp/a.rs"), 3, 2)]);
        let edits = extract_workspace_edits(
            &json!({"changes":{"file:///tmp/a.rs":[{"range":{"start":{"line":0,"character":1},"end":{"line":0,"character":3}},"newText":"value"}]}}),
        );
        assert_eq!(edits[0].1[0], (0, 1, 0, 3, "value".into()));
    }

    #[test]
    fn formats_symbol_and_action_labels() {
        let output = format_items("Symbols", &json!([{"name":"main"},{"title":"Fix import"}]));
        assert!(output.contains("main"));
        assert!(output.contains("Fix import"));
    }
}
