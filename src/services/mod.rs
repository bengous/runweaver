//! Injected capability ports for operations, profiles, and hook commands.
//!
//! Runweaver code that needs to touch the host — read files, run Git, spawn
//! processes, log, read env vars — does so through the trait ports bundled in
//! [`RunweaverServices`] instead of calling `std` directly. Hosts supply real
//! implementations; tests supply fakes. Every fallible port method returns a
//! [`ServicePortError`] naming the capability that failed.
//!
//! The ports: [`FileSystemPort`] (text I/O and directories), [`GitPort`]
//! (repo root, changed files), [`ProcessRunnerPort`] (subprocesses),
//! [`SessionStatePort`] (JSON key-value state scoped to an agent session),
//! [`LoggerPort`] (leveled, structured logging via [`LogFields`]),
//! [`EnvPort`], [`ClockPort`], and [`TempPort`] (temp files/directories).

use std::collections::HashMap;
use std::time::SystemTime;

use serde_json::{Map, Value};

/// Structured logging fields attached to a [`LoggerPort`] message.
pub type LogFields = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{capability} failed: {message}")]
pub struct ServicePortError {
    pub capability: String,
    pub message: String,
}

impl ServicePortError {
    pub fn new(capability: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            capability: capability.into(),
            message: message.into(),
        }
    }
}

/// Bundle of all capability ports, passed by value into operations and
/// profiles. `Copy` so callees can hold it without lifetime gymnastics.
#[derive(Clone, Copy)]
pub struct RunweaverServices<'a> {
    pub file_system: &'a dyn FileSystemPort,
    pub git: &'a dyn GitPort,
    pub process_runner: &'a dyn ProcessRunnerPort,
    pub session_state: &'a dyn SessionStatePort,
    pub logger: &'a dyn LoggerPort,
    pub env: &'a dyn EnvPort,
    pub clock: &'a dyn ClockPort,
    pub temp: &'a dyn TempPort,
}

/// Text file and directory access.
pub trait FileSystemPort {
    fn read_text(&self, path: &str) -> Result<String, ServicePortError>;
    fn write_text(&self, path: &str, contents: &str) -> Result<(), ServicePortError>;
    fn exists(&self, path: &str) -> Result<bool, ServicePortError>;
    fn remove(&self, path: &str) -> Result<(), ServicePortError>;
    fn make_directory(&self, path: &str) -> Result<(), ServicePortError>;
    fn read_directory(&self, path: &str) -> Result<Vec<String>, ServicePortError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangedFilesOptions {
    pub cwd: Option<String>,
    pub base: Option<String>,
}

/// Git repository queries: root discovery, changed files (optionally
/// relative to a base ref via [`ChangedFilesOptions`]), dirty-state checks.
pub trait GitPort {
    fn root(&self, cwd: Option<&str>) -> Result<String, ServicePortError>;
    fn changed_files(&self, options: ChangedFilesOptions) -> Result<Vec<String>, ServicePortError>;
    fn has_changes(&self, cwd: Option<&str>) -> Result<bool, ServicePortError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessRunOptions {
    pub cwd: Option<String>,
    pub env: HashMap<String, Option<String>>,
    pub stdin: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessRunOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

/// Subprocess execution. [`ProcessRunOutput::error`] reports spawn failures,
/// distinct from a non-zero `exit_code`.
pub trait ProcessRunnerPort {
    fn run(
        &self,
        program: &str,
        args: &[String],
        options: ProcessRunOptions,
    ) -> Result<ProcessRunOutput, ServicePortError>;
}

/// JSON key-value state persisted across hook invocations within an agent
/// session (e.g. touched-path tracking, validation fingerprints).
pub trait SessionStatePort {
    fn get(&self, key: &str) -> Result<Option<Value>, ServicePortError>;
    fn set(&self, key: &str, value: Value) -> Result<(), ServicePortError>;
    fn delete(&self, key: &str) -> Result<(), ServicePortError>;
    fn keys(&self) -> Result<Vec<String>, ServicePortError>;
}

/// Leveled logging with optional structured [`LogFields`].
pub trait LoggerPort {
    fn debug(&self, message: &str, fields: Option<&LogFields>);
    fn info(&self, message: &str, fields: Option<&LogFields>);
    fn warn(&self, message: &str, fields: Option<&LogFields>);
    fn error(&self, message: &str, fields: Option<&LogFields>);
}

/// Environment variable access.
pub trait EnvPort {
    fn get(&self, name: &str) -> Option<String>;
    fn snapshot(&self) -> HashMap<String, String>;
}

/// Time access and sleeping, injectable for deterministic tests.
pub trait ClockPort {
    fn now_millis(&self) -> u128;
    fn instant(&self) -> SystemTime;
    fn sleep(&self, milliseconds: u64) -> Result<(), ServicePortError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TempDirectoryOptions {
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TempFileOptions {
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub contents: Option<String>,
}

/// Temporary file and directory creation.
pub trait TempPort {
    fn directory(&self, options: TempDirectoryOptions) -> Result<String, ServicePortError>;
    fn file(&self, options: TempFileOptions) -> Result<String, ServicePortError>;
    fn remove(&self, path: &str) -> Result<(), ServicePortError>;
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime};

    use serde_json::Value;

    use super::{
        ChangedFilesOptions, ClockPort, EnvPort, FileSystemPort, GitPort, LogFields, LoggerPort,
        ProcessRunOptions, ProcessRunOutput, ProcessRunnerPort, RunweaverServices,
        ServicePortError, SessionStatePort, TempDirectoryOptions, TempFileOptions, TempPort,
    };

    #[derive(Default)]
    pub(crate) struct TestPorts {
        pub(crate) file_system: TestFileSystem,
        pub(crate) git: TestGit,
        pub(crate) process_runner: TestProcessRunner,
        pub(crate) session_state: TestSessionState,
        pub(crate) logger: TestLogger,
        pub(crate) env: TestEnv,
        pub(crate) clock: TestClock,
        pub(crate) temp: TestTemp,
    }

    impl TestPorts {
        pub(crate) fn services(&self) -> RunweaverServices<'_> {
            RunweaverServices {
                file_system: &self.file_system,
                git: &self.git,
                process_runner: &self.process_runner,
                session_state: &self.session_state,
                logger: &self.logger,
                env: &self.env,
                clock: &self.clock,
                temp: &self.temp,
            }
        }
    }

    #[derive(Default)]
    pub(crate) struct TestFileSystem;

    impl FileSystemPort for TestFileSystem {
        fn read_text(&self, path: &str) -> Result<String, ServicePortError> {
            Ok(format!("read:{path}"))
        }

        fn write_text(&self, _path: &str, _contents: &str) -> Result<(), ServicePortError> {
            Ok(())
        }

        fn exists(&self, _path: &str) -> Result<bool, ServicePortError> {
            Ok(false)
        }

        fn remove(&self, _path: &str) -> Result<(), ServicePortError> {
            Ok(())
        }

        fn make_directory(&self, _path: &str) -> Result<(), ServicePortError> {
            Ok(())
        }

        fn read_directory(&self, _path: &str) -> Result<Vec<String>, ServicePortError> {
            Ok(vec!["entry".to_owned()])
        }
    }

    #[derive(Default)]
    pub(crate) struct TestGit;

    impl GitPort for TestGit {
        fn root(&self, _cwd: Option<&str>) -> Result<String, ServicePortError> {
            Ok("/repo".to_owned())
        }

        fn changed_files(
            &self,
            _options: ChangedFilesOptions,
        ) -> Result<Vec<String>, ServicePortError> {
            Ok(vec!["src/main.rs".to_owned()])
        }

        fn has_changes(&self, _cwd: Option<&str>) -> Result<bool, ServicePortError> {
            Ok(true)
        }
    }

    #[derive(Default)]
    pub(crate) struct TestProcessRunner;

    impl ProcessRunnerPort for TestProcessRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
            _options: ProcessRunOptions,
        ) -> Result<ProcessRunOutput, ServicePortError> {
            Ok(ProcessRunOutput {
                exit_code: Some(0),
                stdout: format!("{program} {}", args.join(" ")),
                stderr: String::new(),
                error: None,
            })
        }
    }

    #[derive(Default)]
    pub(crate) struct TestSessionState {
        values: Mutex<HashMap<String, Value>>,
    }

    impl SessionStatePort for TestSessionState {
        fn get(&self, key: &str) -> Result<Option<Value>, ServicePortError> {
            let values = self
                .values
                .lock()
                .map_err(|error| ServicePortError::new("sessionState", error.to_string()))?;
            Ok(values.get(key).cloned())
        }

        fn set(&self, key: &str, value: Value) -> Result<(), ServicePortError> {
            let mut values = self
                .values
                .lock()
                .map_err(|error| ServicePortError::new("sessionState", error.to_string()))?;
            values.insert(key.to_owned(), value);
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), ServicePortError> {
            let mut values = self
                .values
                .lock()
                .map_err(|error| ServicePortError::new("sessionState", error.to_string()))?;
            values.remove(key);
            Ok(())
        }

        fn keys(&self) -> Result<Vec<String>, ServicePortError> {
            let values = self
                .values
                .lock()
                .map_err(|error| ServicePortError::new("sessionState", error.to_string()))?;
            Ok(values.keys().cloned().collect())
        }
    }

    #[derive(Default)]
    pub(crate) struct TestLogger {
        messages: Mutex<Vec<String>>,
    }

    impl TestLogger {
        pub(crate) fn messages(&self) -> Vec<String> {
            self.messages
                .lock()
                .map_or_else(|_| Vec::new(), |messages| messages.clone())
        }

        fn push(&self, level: &str, message: &str) {
            if let Ok(mut messages) = self.messages.lock() {
                messages.push(format!("{level}:{message}"));
            }
        }
    }

    impl LoggerPort for TestLogger {
        fn debug(&self, message: &str, _fields: Option<&LogFields>) {
            self.push("debug", message);
        }

        fn info(&self, message: &str, _fields: Option<&LogFields>) {
            self.push("info", message);
        }

        fn warn(&self, message: &str, _fields: Option<&LogFields>) {
            self.push("warn", message);
        }

        fn error(&self, message: &str, _fields: Option<&LogFields>) {
            self.push("error", message);
        }
    }

    #[derive(Default)]
    pub(crate) struct TestEnv;

    impl EnvPort for TestEnv {
        fn get(&self, name: &str) -> Option<String> {
            (name == "RUNWEAVER_TEST").then(|| "1".to_owned())
        }

        fn snapshot(&self) -> HashMap<String, String> {
            HashMap::from([("RUNWEAVER_TEST".to_owned(), "1".to_owned())])
        }
    }

    #[derive(Default)]
    pub(crate) struct TestClock;

    impl ClockPort for TestClock {
        fn now_millis(&self) -> u128 {
            1_700_000_000_000
        }

        fn instant(&self) -> SystemTime {
            SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_000)
        }

        fn sleep(&self, _milliseconds: u64) -> Result<(), ServicePortError> {
            Ok(())
        }
    }

    #[derive(Default)]
    pub(crate) struct TestTemp;

    impl TempPort for TestTemp {
        fn directory(&self, options: TempDirectoryOptions) -> Result<String, ServicePortError> {
            Ok(format!(
                "/tmp/{}dir",
                options.prefix.unwrap_or_else(|| "runweaver-".to_owned())
            ))
        }

        fn file(&self, options: TempFileOptions) -> Result<String, ServicePortError> {
            Ok(format!(
                "/tmp/{}file{}",
                options.prefix.unwrap_or_else(|| "runweaver-".to_owned()),
                options.suffix.unwrap_or_default()
            ))
        }

        fn remove(&self, _path: &str) -> Result<(), ServicePortError> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::test_support::TestPorts;
    use super::*;

    #[test]
    fn runweaver_services_exposes_runtime_adapter_ports() {
        let ports = TestPorts::default();
        let services = ports.services();

        services
            .session_state
            .set("selected", json!(["src/main.rs"]))
            .unwrap();
        services.logger.info("started", None);

        assert_eq!(
            services.session_state.get("selected").unwrap(),
            Some(json!(["src/main.rs"]))
        );
        assert_eq!(
            services.file_system.read_text("a.txt").unwrap(),
            "read:a.txt"
        );
        assert_eq!(services.git.root(None).unwrap(), "/repo");
        assert_eq!(
            services
                .process_runner
                .run("cargo", &["test".to_owned()], ProcessRunOptions::default())
                .unwrap()
                .stdout,
            "cargo test"
        );
        assert_eq!(services.env.get("RUNWEAVER_TEST"), Some("1".to_owned()));
        assert_eq!(services.clock.now_millis(), 1_700_000_000_000);
        assert_eq!(
            services
                .temp
                .directory(TempDirectoryOptions {
                    prefix: Some("rw-".to_owned())
                })
                .unwrap(),
            "/tmp/rw-dir"
        );
        assert_eq!(ports.logger.messages(), vec!["info:started".to_owned()]);
    }
}
