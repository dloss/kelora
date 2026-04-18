use anyhow::{Context, Result};
use rhai::{Array, Engine, ImmutableString};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::LazyLock;
use std::sync::{Arc, Mutex, RwLock};

/// Runtime configuration controlling whether file operations are permitted and how errors behave.
#[derive(Clone, Debug, Default)]
pub struct RuntimeConfig {
    pub allow_fs_writes: bool,
    pub strict: bool,
    pub quiet_level: u8,
}

/// Execution mode for file operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FileOpMode {
    Sequential = 0,
    ParallelOrdered = 1,
    ParallelUnordered = 2,
}

/// File operation recorded during Rhai execution.
#[derive(Clone, Debug)]
pub enum FileOp {
    Mkdir { path: PathBuf, recursive: bool },
    Truncate { path: PathBuf },
    Append { path: PathBuf, payload: Vec<u8> },
}

/// Shared runtime configuration for all threads.
static RUNTIME_CONFIG: LazyLock<RwLock<RuntimeConfig>> =
    LazyLock::new(|| RwLock::new(RuntimeConfig::default()));

/// One-time warning guard for missing `--allow-fs-writes`.
static WARNED_DISALLOWED: AtomicBool = AtomicBool::new(false);

/// Cache of emitted error warnings to avoid spamming.
static ERROR_LOG_CACHE: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Memorised per-path mutexes to serialize append operations when needed.
static PATH_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Current execution mode (sequential / parallel).
static FILE_OP_MODE: AtomicU8 = AtomicU8::new(FileOpMode::Sequential as u8);

thread_local! {
    static PENDING_OPS: RefCell<Vec<FileOp>> = const { RefCell::new(Vec::new()) };
}

/// Register file operation helpers with the Rhai engine.
pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("mkdir", mkdir_single);
    engine.register_fn("mkdir", mkdir_with_parents);
    engine.register_fn("truncate_file", truncate_file);
    engine.register_fn("append_file", append_file_string);
    engine.register_fn("append_file", append_file_array);
}

/// Update runtime configuration (must be called before executing scripts that use file ops).
pub fn set_runtime_config(config: RuntimeConfig) {
    let mut guard = RUNTIME_CONFIG
        .write()
        .expect("file ops runtime config poisoned");
    *guard = config;
}

/// Retrieve the current runtime configuration.
pub fn get_runtime_config() -> RuntimeConfig {
    RUNTIME_CONFIG
        .read()
        .expect("file ops runtime config poisoned")
        .clone()
}

/// Set the current execution mode for file operations.
pub fn set_mode(mode: FileOpMode) {
    FILE_OP_MODE.store(mode as u8, Ordering::Relaxed);
}

/// Drain pending file operations recorded for the current thread.
pub fn take_pending_ops() -> Vec<FileOp> {
    PENDING_OPS.with(|slot| slot.borrow_mut().drain(..).collect())
}

/// Clear any pending file operations without executing them.
pub fn clear_pending_ops() {
    PENDING_OPS.with(|slot| slot.borrow_mut().clear());
}

/// Execute a collection of file operations using the global executor.
pub fn execute_ops(ops: &[FileOp]) -> Result<()> {
    if ops.is_empty() {
        return Ok(());
    }

    let runtime = get_runtime_config();
    if !runtime.allow_fs_writes {
        return Ok(());
    }

    for op in ops {
        if let Err(err) = execute_single_op(op) {
            if runtime.strict {
                return Err(err.context("file operation failed in strict mode"));
            }

            warn_once(
                format!("{}::{:?}", op_path_display(op), op_kind(op)),
                &format!("kelora: file operation failed: {}", err),
                runtime.quiet_level,
            );
        }
    }

    Ok(())
}

/// Internal helper to execute a single operation.
fn execute_single_op(op: &FileOp) -> Result<()> {
    match op {
        FileOp::Mkdir { path, recursive } => {
            if *recursive {
                match fs::create_dir_all(path) {
                    Ok(_) => Ok(()),
                    Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(()),
                    Err(err) => Err(err.into()),
                }
            } else {
                match fs::create_dir(path) {
                    Ok(_) => Ok(()),
                    Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(()),
                    Err(err) => Err(err.into()),
                }
            }
        }
        FileOp::Truncate { path } => OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map(|_| ())
            .map_err(Into::into),
        FileOp::Append { path, payload } => {
            if payload.is_empty() {
                return Ok(());
            }

            let lock = lock_for_path(path);
            let _guard = lock.lock().expect("file append mutex poisoned");

            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("failed to open {} for append", path.display()))?;
            file.write_all(payload)?;
            Ok(())
        }
    }
}

fn lock_for_path(path: &Path) -> Arc<Mutex<()>> {
    let mut guard = PATH_LOCKS.lock().expect("path lock map poisoned");
    guard
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn mkdir_single(path: ImmutableString) -> bool {
    mkdir_impl(path, false)
}

fn mkdir_with_parents(path: ImmutableString, recursive: bool) -> bool {
    mkdir_impl(path, recursive)
}

fn truncate_file(path: ImmutableString) -> bool {
    if !check_allowed() {
        return false;
    }

    let Some(pathbuf) = normalise_path(path) else {
        return false;
    };

    record_op(FileOp::Truncate { path: pathbuf });
    true
}

fn append_file_string(path: ImmutableString, content: ImmutableString) -> bool {
    append_file_impl(path, AppendPayload::Single(content.into_owned()))
}

fn append_file_array(path: ImmutableString, items: Array) -> bool {
    append_file_impl(path, AppendPayload::Array(items))
}

fn mkdir_impl(path: ImmutableString, recursive: bool) -> bool {
    if !check_allowed() {
        return false;
    }

    let Some(pathbuf) = normalise_path(path) else {
        return false;
    };

    record_op(FileOp::Mkdir {
        path: pathbuf,
        recursive,
    });
    true
}

fn append_file_impl(path: ImmutableString, payload: AppendPayload) -> bool {
    if !check_allowed() {
        return false;
    }

    let Some(pathbuf) = normalise_path(path) else {
        return false;
    };

    let payload_bytes = match payload.into_bytes() {
        Ok(data) => data,
        Err(_) => return false,
    };

    if payload_bytes.is_empty() {
        return true;
    }

    record_op(FileOp::Append {
        path: pathbuf,
        payload: payload_bytes,
    });
    true
}

fn check_allowed() -> bool {
    let runtime = get_runtime_config();
    if runtime.allow_fs_writes {
        return true;
    }

    if !WARNED_DISALLOWED.swap(true, Ordering::Relaxed) && runtime.quiet_level < 3 {
        eprintln!("kelora: enable --allow-fs-writes to use mkdir/truncate_file/append_file");
    }
    false
}

fn normalise_path(path: ImmutableString) -> Option<PathBuf> {
    let owned = path.into_owned();
    let trimmed = owned.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn record_op(op: FileOp) {
    PENDING_OPS.with(|slot| slot.borrow_mut().push(op));
}

fn op_kind(op: &FileOp) -> &'static str {
    match op {
        FileOp::Mkdir { .. } => "mkdir",
        FileOp::Truncate { .. } => "truncate",
        FileOp::Append { .. } => "append",
    }
}

fn op_path_display(op: &FileOp) -> String {
    match op {
        FileOp::Mkdir { path, .. } | FileOp::Truncate { path } | FileOp::Append { path, .. } => {
            path.display().to_string()
        }
    }
}

fn warn_once(key: String, message: &str, quiet_level: u8) {
    if quiet_level >= 2 {
        return;
    }

    let mut cache = ERROR_LOG_CACHE
        .lock()
        .expect("file ops warning cache poisoned");
    if cache.insert(key) {
        eprintln!("{}", message);
    }
}

enum AppendPayload {
    Single(String),
    Array(Array),
}

impl AppendPayload {
    fn into_bytes(self) -> Result<Vec<u8>, ()> {
        match self {
            AppendPayload::Single(line) => Ok(normalise_line(line)),
            AppendPayload::Array(items) => {
                if items.is_empty() {
                    return Ok(Vec::new());
                }
                let mut buffer = Vec::new();
                for value in items {
                    let line = if let Some(s) = value.clone().try_cast::<ImmutableString>() {
                        s.into_owned()
                    } else if let Ok(s) = value.clone().into_string() {
                        s
                    } else {
                        return Err(());
                    };
                    buffer.extend_from_slice(&normalise_line(line));
                }
                Ok(buffer)
            }
        }
    }
}

fn normalise_line(mut line: String) -> Vec<u8> {
    if !line.ends_with('\n') {
        line.push('\n');
    }
    line.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn with_runtime<F: FnOnce() -> T, T>(allow: bool, strict: bool, quiet: u8, f: F) -> T {
        let _guard = TEST_LOCK.lock().unwrap();
        set_runtime_config(RuntimeConfig {
            allow_fs_writes: allow,
            strict,
            quiet_level: quiet,
        });
        PENDING_OPS.with(|slot| slot.borrow_mut().clear());
        WARNED_DISALLOWED.store(false, Ordering::Relaxed);
        f()
    }

    #[test]
    fn append_string_records_payload() {
        with_runtime(true, false, 0, || {
            assert!(append_file_string("foo.txt".into(), "hello".into()));
            let ops = take_pending_ops();
            assert_eq!(ops.len(), 1);
            match &ops[0] {
                FileOp::Append { payload, .. } => {
                    assert_eq!(payload, b"hello\n");
                }
                _ => panic!("expected append op"),
            }
        });
    }

    #[test]
    fn append_array_handles_empty() {
        with_runtime(true, false, 0, || {
            assert!(append_file_array("foo.txt".into(), Array::new()))
        });
    }

    #[test]
    fn operations_blocked_without_permission() {
        with_runtime(false, false, 0, || {
            assert!(!mkdir_single("blocked".into()));
            assert!(!truncate_file("nope".into()));
            assert!(!append_file_string("out.log".into(), "line".into()));
            assert!(take_pending_ops().is_empty());
        });
    }

    #[test]
    fn ops_execute_in_tempdir() {
        with_runtime(true, true, 0, || {
            let dir = tempdir().unwrap();
            let file_path = dir.path().join("out.log");

            record_op(FileOp::Mkdir {
                path: dir.path().join("nested"),
                recursive: true,
            });
            record_op(FileOp::Truncate {
                path: file_path.clone(),
            });
            record_op(FileOp::Append {
                path: file_path.clone(),
                payload: b"hello\n".to_vec(),
            });

            execute_ops(&take_pending_ops()).unwrap();

            let contents = std::fs::read_to_string(&file_path).unwrap();
            assert_eq!(contents, "hello\n");
        });
    }
}
