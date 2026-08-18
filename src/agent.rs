use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde_json::{json, Value};

pub struct AgentRequest {
    pub id: Value,
    pub method: String,
    pub params: Value,
    reply: SyncSender<Value>,
}

impl AgentRequest {
    pub fn reply(self, result: Result<Value, String>) {
        let response = match result {
            Ok(value) => json!({"jsonrpc":"2.0","id":self.id,"result":value}),
            Err(error) => {
                json!({"jsonrpc":"2.0","id":self.id,"error":{"code":-32000,"message":error}})
            }
        };
        let _ = self.reply.send(response);
    }
}

pub struct AgentServer {
    path: PathBuf,
    requests: Receiver<AgentRequest>,
    cancellation: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl AgentServer {
    pub fn start(path: PathBuf) -> io::Result<Self> {
        if path.exists() {
            fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let (sender, requests) = mpsc::channel();
        let cancellation = Arc::new(AtomicBool::new(false));
        let cancelled = cancellation.clone();
        let worker = thread::Builder::new()
            .name("tted-agent-api".into())
            .spawn(move || {
                while !cancelled.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let sender = sender.clone();
                            thread::spawn(move || handle_client(stream, sender));
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(25))
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self {
            path,
            requests,
            cancellation,
            worker: Some(worker),
        })
    }

    pub fn default_path() -> PathBuf {
        std::env::var_os("TTED_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("tted-{}.sock", std::process::id()))
            })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn try_recv(&self) -> Option<AgentRequest> {
        self.requests.try_recv().ok()
    }
}

impl Drop for AgentServer {
    fn drop(&mut self) {
        self.cancellation.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

fn handle_client(mut stream: UnixStream, sender: Sender<AgentRequest>) {
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    loop {
        let mut line = String::new();
        if reader
            .read_line(&mut line)
            .ok()
            .filter(|read| *read > 0)
            .is_none()
        {
            break;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(value) => {
                let id = value.get("id").cloned().unwrap_or(Value::Null);
                let Some(method) = value
                    .get("method")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    write_response(
                        &mut stream,
                        &json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"invalid request"}}),
                    );
                    continue;
                };
                let (reply, result) = mpsc::sync_channel(1);
                if sender
                    .send(AgentRequest {
                        id,
                        method,
                        params: value.get("params").cloned().unwrap_or_else(|| json!({})),
                        reply,
                    })
                    .is_err()
                {
                    break;
                }
                result.recv_timeout(Duration::from_secs(30)).unwrap_or_else(|_| json!({"jsonrpc":"2.0","id":null,"error":{"code":-32001,"message":"editor response timed out"}}))
            }
            Err(error) => {
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":error.to_string()}})
            }
        };
        write_response(&mut stream, &response);
    }
}

fn write_response(stream: &mut UnixStream, value: &Value) {
    if let Ok(mut data) = serde_json::to_vec(value) {
        data.push(b'\n');
        let _ = stream.write_all(&data);
        let _ = stream.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_round_trip_and_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("agent.sock");
        let server = AgentServer::start(path.clone()).unwrap();
        let client = thread::spawn({
            let path = path.clone();
            move || {
                let mut stream = UnixStream::connect(path).unwrap();
                writeln!(
                    stream,
                    "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"workspace.info\"}}"
                )
                .unwrap();
                let mut response = String::new();
                BufReader::new(stream).read_line(&mut response).unwrap();
                response
            }
        });
        let request = loop {
            if let Some(request) = server.try_recv() {
                break request;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(request.method, "workspace.info");
        request.reply(Ok(json!({"name":"test"})));
        assert!(client.join().unwrap().contains("test"));
        drop(server);
        assert!(!path.exists());
    }
}
