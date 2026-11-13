use std::{
    collections::HashMap,
    io::Write,
    sync::{Mutex, Once, OnceLock},
    thread::ThreadId,
};

struct Capture {
    thread: ThreadId,
    messages: Vec<String>,
}

fn captures() -> &'static Mutex<HashMap<&'static str, Capture>> {
    static CAPTURES: OnceLock<Mutex<HashMap<&'static str, Capture>>> = OnceLock::new();
    CAPTURES.get_or_init(Default::default)
}

pub(crate) fn watch(name: &'static str) {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        env_logger::Builder::new()
            .is_test(true)
            .filter_level(log::LevelFilter::Error)
            .format(|buffer, record| {
                let thread = std::thread::current().id();
                for capture in captures().lock().unwrap().values_mut() {
                    if capture.thread == thread {
                        capture.messages.push(record.args().to_string());
                    }
                }
                writeln!(buffer, "{} {}", record.level(), record.args())
            })
            .try_init()
            .expect("install engine-scoped error capture");
    });
    captures().lock().unwrap().insert(
        name,
        Capture {
            thread: std::thread::current().id(),
            messages: Vec::new(),
        },
    );
}

pub(crate) fn take(name: &'static str) -> Vec<String> {
    captures()
        .lock()
        .unwrap()
        .remove(name)
        .expect("the component installed its error capture")
        .messages
}
