use std::{
    io,
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc,
    },
    thread::{self, JoinHandle},
};

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub enum ServiceEvent<T> {
    Item(T),
    Output(String),
    Error(String),
    Finished,
}

pub struct ServiceContext<E> {
    pub(crate) events: Sender<ServiceEvent<E>>,
    pub(crate) cancellation: CancellationToken,
}

impl<E> ServiceContext<E> {
    pub fn emit(&self, event: E) -> bool {
        self.events.send(ServiceEvent::Item(event)).is_ok()
    }

    pub fn output(&self, output: impl Into<String>) -> bool {
        self.events
            .send(ServiceEvent::Output(output.into()))
            .is_ok()
    }

    pub fn error(&self, error: impl Into<String>) -> bool {
        self.events.send(ServiceEvent::Error(error.into())).is_ok()
    }

    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }
}

pub struct BackgroundService<C, E> {
    commands: Option<Sender<C>>,
    events: Receiver<ServiceEvent<E>>,
    cancellation: CancellationToken,
    worker: Option<JoinHandle<()>>,
}

impl<C: Send + 'static, E: Send + 'static> BackgroundService<C, E> {
    pub fn spawn(
        name: impl Into<String>,
        worker: impl FnOnce(Receiver<C>, ServiceContext<E>) + Send + 'static,
    ) -> io::Result<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let cancellation = CancellationToken::default();
        let context = ServiceContext {
            events: event_tx.clone(),
            cancellation: cancellation.clone(),
        };
        let handle = thread::Builder::new().name(name.into()).spawn(move || {
            worker(command_rx, context);
            let _ = event_tx.send(ServiceEvent::Finished);
        })?;
        Ok(Self {
            commands: Some(command_tx),
            events: event_rx,
            cancellation,
            worker: Some(handle),
        })
    }

    pub fn send(&self, command: C) -> Result<(), mpsc::SendError<C>> {
        self.commands
            .as_ref()
            .expect("live service sender")
            .send(command)
    }

    pub fn try_recv(&self) -> Result<ServiceEvent<E>, TryRecvError> {
        self.events.try_recv()
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}

impl<C, E> Drop for BackgroundService<C, E> {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.commands.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub struct ManagedChild(Child);

impl ManagedChild {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        command.spawn().map(Self)
    }

    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.0
    }

    pub fn terminate(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn service_streams_events_and_shuts_down() {
        let service = BackgroundService::spawn("test-service", |commands, context| {
            while !context.cancellation().is_cancelled() {
                match commands.recv_timeout(Duration::from_millis(10)) {
                    Ok(value) => {
                        context.output(format!("received {value}"));
                        context.emit(value * 2);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .unwrap();
        service.send(21).unwrap();
        let mut doubled = None;
        for _ in 0..20 {
            if let Ok(ServiceEvent::Item(value)) = service.try_recv() {
                doubled = Some(value);
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(doubled, Some(42));
        service.cancel();
    }
}
