use chrono::Local;
use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use serde_json::json;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

// Global operation counter to create unique operation IDs
static OPERATION_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// LogFormat defines how logs are formatted
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Text format is more human-readable but less machine-parseable
    Text,
    /// JSON format is better for log analysis tools but less human-readable
    Json,
}

/// LogTarget defines where logs are sent (used during configuration phase)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogTarget {
    /// Log to stdout only
    Stdout,
    /// Log to a specific file only
    File(PathBuf),
    /// Log to both stdout and a file
    Both(PathBuf),
}

/// HoneyBadgerLogger is our custom logger implementation
pub struct HoneyBadgerLogger {
    /// The minimum level to log
    level: LevelFilter,
    /// How logs are formatted
    format: LogFormat,
    /// Optional thread-safe file writer
    file_writer: Option<Mutex<File>>,
    /// Flag to indicate if logging to stdout is also required
    log_to_stdout: bool,
    /// Optional module filter (only log modules that contain these strings)
    module_filters: Vec<String>,
    /// Optional module level overrides (module -> level)
    module_levels: Vec<(String, LevelFilter)>,
}

impl Log for HoneyBadgerLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // Check if the level is enabled
        let level_enabled = metadata.level() <= self.level;
        if !level_enabled {
            return false;
        }

        // If we have module filters, check if the target matches any of them
        if !self.module_filters.is_empty() {
            let target = metadata.target();
            let matches_filter = self
                .module_filters
                .iter()
                .any(|filter| target.contains(filter));
            if !matches_filter {
                return false;
            }
        }

        // Check for module-specific level overrides
        if let Some((_, level)) = self
            .module_levels
            .iter()
            .find(|(module, _)| metadata.target().contains(module))
        {
            return metadata.level() <= *level;
        }

        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let now = Local::now();
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S%.3f").to_string();
        let level = record.level();
        let target = record.target();
        let message = record.args().to_string();

        // Format the log entry
        let log_entry = match self.format {
            LogFormat::Text => {
                format!("[{}] {} [{}] {}\\n", timestamp, level, target, message)
            }
            LogFormat::Json => {
                let json = json!({
                    "timestamp": timestamp,
                    "level": level.to_string(),
                    "target": target,
                    "message": message,
                });
                format!("{}\n", json)
            }
        };
        let log_entry_bytes = log_entry.as_bytes();

        if self.log_to_stdout {
            // If stdout write fails, there's not much we can do, just ignore the error.
            let _ = io::stdout().write_all(log_entry_bytes);
            // Flushed in the flush method
        }

        if let Some(mutex_file) = &self.file_writer {
            // Attempt to lock. If poisoned, we might choose to log an error to stderr once
            // or simply stop trying to log to file for this call.
            if let Ok(mut file_guard) = mutex_file.lock() {
                // If file write fails, ignore the error for now.
                // Robust handling might involve retries or disabling file logging.
                let _ = file_guard.write_all(log_entry_bytes);
                // Flushed in the flush method
            } else {
                // Mutex was poisoned. Log to stderr once if desired, or handle as a silent failure.
                // eprintln!("[ERROR] Log file mutex poisoned. Cannot write to file.");
            }
        }
    }

    fn flush(&self) {
        if self.log_to_stdout {
            let _ = io::stdout().flush();
        }
        if let Some(mutex_file) = &self.file_writer {
            if let Ok(mut file_guard) = mutex_file.lock() {
                let _ = file_guard.flush();
            }
            // Else: Mutex poisoned, cannot flush.
        }
    }
}

impl HoneyBadgerLogger {
    /// Create a new logger with the specified configuration.
    /// File opening is attempted here.
    pub fn new(
        level: LevelFilter,
        format: LogFormat,
        initial_target: LogTarget, // The configured intent
        module_filters: Vec<String>,
        module_levels: Vec<(String, LevelFilter)>,
    ) -> Self {
        let mut file_writer = None;
        let mut log_to_stdout = false;

        match initial_target {
            LogTarget::Stdout => {
                log_to_stdout = true;
            }
            LogTarget::File(path) => match open_log_file_once(&path) {
                Ok(file) => file_writer = Some(Mutex::new(file)),
                Err(e) => {
                    eprintln!(
                        "[ERROR] Failed to open log file {:?}: {}. File logging will be disabled.",
                        path, e
                    );
                }
            },
            LogTarget::Both(path) => {
                log_to_stdout = true;
                match open_log_file_once(&path) {
                    Ok(file) => file_writer = Some(Mutex::new(file)),
                    Err(e) => {
                        eprintln!(
                            "[ERROR] Failed to open log file {:?} for combined logging: {}. File part of logging will be disabled.",
                            path, e
                        );
                        // log_to_stdout remains true
                    }
                }
            }
        }

        Self {
            level,
            format,
            file_writer,
            log_to_stdout,
            module_filters,
            module_levels,
        }
    }

    /// Consumes the logger and sets it as the global logger.
    pub fn actual_init(self) -> Result<(), SetLoggerError> {
        // No need to create_dir_all here as open_log_file_once handles it.
        log::set_max_level(self.level);
        log::set_boxed_logger(Box::new(self))
    }
}

/// Generate a unique operation ID for tracking related log messages
pub fn generate_operation_id() -> String {
    let counter = OPERATION_COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = Local::now().format("%Y%m%d%H%M%S").to_string();
    format!("op-{}-{}", timestamp, counter)
}

/// Helper function to open a log file once with append mode.
/// Creates parent directories if they don't exist.
fn open_log_file_once(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?; // Create parent directory if it doesn't exist
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Initialize the global logger with settings from config and CLI.
pub fn init_logger(
    log_level: Option<&str>,
    debug: bool,
    log_file: Option<&str>,
    logs_dir: &str,
    command_name: &str,
    module_filters: Option<&str>,
) -> Result<(), SetLoggerError> {
    // Determine log level
    let level = if let Some(level) = log_level {
        match level {
            "error" => LevelFilter::Error,
            "warn" => LevelFilter::Warn,
            "info" => LevelFilter::Info,
            "debug" => LevelFilter::Debug,
            "trace" => LevelFilter::Trace,
            _ => LevelFilter::Info,
        }
    } else if debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    // Parse module filters
    let module_filters_vec = module_filters
        .map(|filters| filters.split(',').map(String::from).collect())
        .unwrap_or_else(Vec::new);

    // Determine log target intent (LogTarget still uses PathBuf here)
    let initial_target = if let Some(log_file_arg) = log_file {
        let log_path = if Path::new(log_file_arg).is_absolute() {
            PathBuf::from(log_file_arg)
        } else {
            PathBuf::from(logs_dir).join(log_file_arg)
        };
        LogTarget::Both(log_path) // Defaulting to Both if file is specified, like before
    } else {
        // Automatically create a log file based on command if --log-file is not provided
        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let default_log_name = format!("{}_{}.log", command_name, timestamp);
        let log_path = PathBuf::from(logs_dir).join(default_log_name);
        LogTarget::Both(log_path) // Defaulting to Both for auto-created files too
    };

    // Module-specific levels - can be expanded as needed
    let module_levels = vec![
        // Example: Lower level for noisy modules
        (
            "honeybadger::infra::db::transaction".to_string(),
            LevelFilter::Warn,
        ),
    ];

    // Create and initialize logger
    let logger_instance = HoneyBadgerLogger::new(
        level,
        LogFormat::Json, // Default to JSON for better analysis
        initial_target,  // Pass the configured intent
        module_filters_vec,
        module_levels,
    );
    logger_instance.actual_init() // Call the method that sets the global logger
}

/// Helper function for logging errors and returning a default value
pub fn log_and_default<T, E: std::fmt::Display>(
    result: Result<T, E>,
    context: &str,
    default: T,
) -> T {
    match result {
        Ok(value) => value,
        Err(err) => {
            log::error!("{}: {}", context, err);
            default
        }
    }
}

/// Helper function for logging errors and returning the error
pub fn log_error<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> Result<T, E> {
    result.map_err(|err| {
        log::error!("{}: {}", context, err);
        err
    })
}
