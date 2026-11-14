use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant},
};

const EVENT_CAPACITY: usize = 16_384;
const FINISH_TIMEOUT: Duration = Duration::from_secs(5);

unsafe extern "C" {
    fn CACurrentMediaTime() -> f64;
}

pub fn host_time_seconds() -> f64 {
    unsafe { CACurrentMediaTime() }
}

enum Message {
    Event(Value),
    Finish(SyncSender<Result<(), String>>),
}

pub struct Recorder {
    started: Instant,
    sender: SyncSender<Message>,
    dropped: AtomicU64,
}

impl Recorder {
    pub fn open(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let (sender, receiver) = mpsc::sync_channel(EVENT_CAPACITY);
        std::thread::Builder::new()
            .name("playback-probe-jsonl".into())
            .spawn(move || {
                let mut writer = BufWriter::new(file);
                let mut failure = None;
                while let Ok(message) = receiver.recv() {
                    match message {
                        Message::Event(event) => {
                            if failure.is_none()
                                && let Err(error) = write_event(&mut writer, &event)
                            {
                                failure = Some(error.to_string());
                            }
                        }
                        Message::Finish(reply) => {
                            let result = failure.map_or_else(
                                || writer.flush().map_err(|error| error.to_string()),
                                Err,
                            );
                            let _ = reply.send(result);
                            return;
                        }
                    }
                }
            })?;
        Ok(Self {
            started: Instant::now(),
            sender,
            dropped: AtomicU64::new(0),
        })
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn event(&self, mut event: Value) {
        event["wallSeconds"] = json!(self.elapsed_secs());
        if self.sender.try_send(Message::Event(event)).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn finish(&self) -> Result<(), Box<dyn std::error::Error>> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.sender.send(Message::Finish(sender))?;
        receiver.recv_timeout(FINISH_TIMEOUT)??;
        Ok(())
    }
}

fn write_event(writer: &mut impl Write, event: &Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, event)?;
    writer.write_all(b"\n")
}
