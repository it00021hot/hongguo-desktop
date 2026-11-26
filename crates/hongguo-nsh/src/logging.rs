use chrono::Local;
use env_logger::{Builder, Env, Logger};
use log::{Level, Log, Metadata, Record};
use std::{
    backtrace::Backtrace,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

const APP_LOG_FILE: &str = "app.log";
const APP_LOG_BACKUP: &str = "app.log.1";
const CRASH_LOG_FILE: &str = "crash.log";
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const EXCERPT_APP_LINES: usize = 80;
const EXCERPT_CRASH_LINES: usize = 40;
const DEFAULT_FILTER: &str = "warn,hongguo_nsh=info,hongguo_service=info,hongguo_api=info,hongguo_media=info,hongguo_stream=info,hongguo_video=info,hongguo_playback=info,hongguo_nsh_video_macos=info,hongguo_nsh_video_windows=info,nsh_platform_windows=info,nsh_platform_macos=info";

struct LogSink {
    path: PathBuf,
    writer: BufWriter<File>,
    written: u64,
}

impl LogSink {
    fn open(dir: &Path) -> Option<Self> {
        fs::create_dir_all(dir).ok()?;
        let path = dir.join(APP_LOG_FILE);
        let written = fs::metadata(&path).map_or(0, |meta| meta.len());
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        Some(Self {
            path,
            writer: BufWriter::new(file),
            written,
        })
    }

    fn write_line(&mut self, line: &str) {
        if self.written >= MAX_LOG_BYTES {
            self.rotate();
        }
        let bytes = line.as_bytes();
        if self.writer.write_all(bytes).is_ok() {
            let _ = self.writer.flush();
            self.written = self.written.saturating_add(bytes.len() as u64);
        }
    }

    fn rotate(&mut self) {
        let _ = self.writer.flush();
        let backup = self.path.with_file_name(APP_LOG_BACKUP);
        let _ = fs::remove_file(&backup);
        let _ = fs::rename(&self.path, &backup);
        if let Ok(file) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
        {
            self.writer = BufWriter::new(file);
            self.written = 0;
        }
    }
}

struct AppLogger {
    filter: Logger,
    sink: Option<Mutex<LogSink>>,
}

impl Log for AppLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.filter.enabled(metadata)
    }

    fn log(&self, record: &Record<'_>) {
        if !self.filter.matches(record) {
            return;
        }
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let line = format!(
            "[{timestamp} {} {}] {}\n",
            record.level(),
            record.target(),
            record.args()
        );
        let _ = std::io::stderr().write_all(line.as_bytes());
        if let Some(sink) = &self.sink
            && let Ok(mut guard) = sink.lock()
        {
            guard.write_line(&line);
        }
    }

    fn flush(&self) {
        let _ = std::io::stderr().flush();
        if let Some(sink) = &self.sink
            && let Ok(mut guard) = sink.lock()
        {
            let _ = guard.writer.flush();
        }
    }
}

/// Initializes desktop stderr + rotating file logging (`app.log`) and panic
/// capture (`crash.log`) under the application data directory.
pub fn init() {
    hongguo_store::paths::set_shell_data_root("guava");
    let data_dir = hongguo_store::paths::data_dir();
    let filter = Builder::from_env(Env::default().default_filter_or(DEFAULT_FILTER)).build();
    let max_level = filter.filter();
    let sink = LogSink::open(&data_dir).map(Mutex::new);
    let logger = Box::new(AppLogger { filter, sink });
    if log::set_boxed_logger(logger).is_ok() {
        log::set_max_level(max_level);
    }
    install_panic_hook(data_dir.join(CRASH_LOG_FILE));
    log::info!(
        "[App] started version={} os={} arch={} logLevel={:?} dataDir={}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        Level::Info.min(max_level.to_level().unwrap_or(Level::Info)),
        data_dir.display()
    );
}

fn install_panic_hook(crash_path: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let location = info.location().map_or_else(
            || "unknown".to_owned(),
            |loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()),
        );
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        let backtrace = Backtrace::force_capture();
        let entry = format!(
            "=== {timestamp} version={} os={} arch={} ===\nlocation: {location}\nmessage: {payload}\nbacktrace:\n{backtrace}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
        );
        log::error!("[Panic] {location}: {payload}");
        log::logger().flush();
        if let Some(parent) = crash_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&crash_path)
        {
            let _ = file.write_all(entry.as_bytes());
            let _ = file.flush();
        }
        previous(info);
    }));
}

pub(crate) fn app_log_path() -> PathBuf {
    hongguo_store::paths::data_dir().join(APP_LOG_FILE)
}

pub(crate) fn crash_log_path() -> PathBuf {
    hongguo_store::paths::data_dir().join(CRASH_LOG_FILE)
}

/// Collects a bounded, redacted diagnostic snippet from `app.log` and `crash.log`
/// for GitHub Issue submission.
pub(crate) fn read_diagnostic_excerpt() -> String {
    let mut out = format!(
        "version: {}\nos: {} ({})\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let app_tail = read_tail(&app_log_path(), EXCERPT_APP_LINES);
    if app_tail.is_empty() {
        out.push_str("\n[app.log] (empty or unavailable)\n");
    } else {
        out.push_str("\n```log\n");
        out.push_str(&app_tail);
        out.push_str("\n```\n");
    }
    let crash_tail = read_tail(&crash_log_path(), EXCERPT_CRASH_LINES);
    if !crash_tail.is_empty() {
        out.push_str("\n[crash.log]\n```log\n");
        out.push_str(&crash_tail);
        out.push_str("\n```\n");
    }
    out
}

fn read_tail(path: &Path, max_lines: usize) -> String {
    let Ok(content) = fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..]
        .iter()
        .map(|line| redact_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    if lower.contains("cookie")
        || lower.contains("authorization")
        || lower.contains("x-tt-token")
        || lower.contains("sessionid")
    {
        return "[redacted sensitive log line]".to_owned();
    }
    strip_url_queries(line)
}

fn strip_url_queries(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = find_http_scheme(rest) {
        out.push_str(&rest[..pos]);
        let url_part = &rest[pos..];
        let end = url_part
            .find(|ch: char| {
                ch.is_whitespace()
                    || matches!(ch, '"' | '\'' | ')' | ']' | '>' | ',' | '，' | '）' | '；')
            })
            .unwrap_or(url_part.len());
        let url = &url_part[..end];
        if let Some(cut) = url.find(['?', '#']) {
            out.push_str(&url[..cut]);
            out.push_str("?[redacted]");
        } else {
            out.push_str(url);
        }
        rest = &url_part[end..];
    }
    out.push_str(rest);
    out
}

fn find_http_scheme(text: &str) -> Option<usize> {
    let http = text.find("http://");
    let https = text.find("https://");
    match (http, https) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}
