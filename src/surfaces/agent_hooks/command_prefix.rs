const DEFAULT_RUNWEAVER_BIN: &str = "./node_modules/.bin/runweaver";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunweaverHookCommandCwd {
    Env(String),
    GitRoot,
    Path(String),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunweaverHookCommandOptions {
    pub harness: String,
    pub cwd: RunweaverHookCommandCwd,
    pub config: String,
    pub export_name: Option<String>,
    pub bin: Option<String>,
}

impl RunweaverHookCommandOptions {
    pub fn new(
        harness: impl Into<String>,
        cwd: RunweaverHookCommandCwd,
        config: impl Into<String>,
    ) -> Self {
        Self {
            harness: harness.into(),
            cwd,
            config: config.into(),
            export_name: None,
            bin: None,
        }
    }

    pub fn with_export_name(mut self, export_name: impl Into<String>) -> Self {
        self.export_name = Some(export_name.into());
        self
    }

    pub fn with_bin(mut self, bin: impl Into<String>) -> Self {
        self.bin = Some(bin.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledRunweaverHookCommandOptions {
    pub harness: String,
    pub cwd: RunweaverHookCommandCwd,
    pub bin: String,
}

impl CompiledRunweaverHookCommandOptions {
    pub fn new(
        harness: impl Into<String>,
        cwd: RunweaverHookCommandCwd,
        bin: impl Into<String>,
    ) -> Self {
        Self {
            harness: harness.into(),
            cwd,
            bin: bin.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookCommandPrefixError {
    #[error("Runweaver hook command {name} must not be empty.")]
    EmptyOption { name: String },
    #[error("Runweaver hook command env name must be a shell variable identifier: {name}")]
    InvalidEnvName { name: String },
}

pub fn runweaver_hook_command(
    options: &RunweaverHookCommandOptions,
) -> Result<String, HookCommandPrefixError> {
    let bin = required_option(
        "bin",
        options.bin.as_deref().unwrap_or(DEFAULT_RUNWEAVER_BIN),
    )?;
    let harness = required_option("harness", &options.harness)?;
    let config = required_option("config", &options.config)?;
    let export_arg = options
        .export_name
        .as_deref()
        .map(|export_name| {
            Ok(format!(
                " --export {}",
                shell_arg(required_option("exportName", export_name)?)
            ))
        })
        .transpose()?
        .unwrap_or_default();

    Ok(format!(
        "{}{} hook {} --config {}{}",
        cwd_prefix(&options.cwd)?,
        shell_arg(bin),
        shell_arg(harness),
        shell_arg(config),
        export_arg
    ))
}

pub fn compiled_runweaver_hook_command(
    options: &CompiledRunweaverHookCommandOptions,
) -> Result<String, HookCommandPrefixError> {
    let bin = required_option("bin", &options.bin)?;
    let harness = required_option("harness", &options.harness)?;

    Ok(format!(
        "{}{} hook {}",
        cwd_prefix(&options.cwd)?,
        shell_arg(bin),
        shell_arg(harness)
    ))
}

fn cwd_prefix(cwd: &RunweaverHookCommandCwd) -> Result<String, HookCommandPrefixError> {
    match cwd {
        RunweaverHookCommandCwd::Env(env_name) => {
            let env_name = required_option("cwd.env", env_name)?;
            if !is_shell_identifier(env_name) {
                return Err(HookCommandPrefixError::InvalidEnvName {
                    name: env_name.to_owned(),
                });
            }
            Ok(format!("cd \"${env_name}\" && "))
        }
        RunweaverHookCommandCwd::GitRoot => {
            Ok("cd \"$(git rev-parse --show-toplevel)\" && ".to_owned())
        }
        RunweaverHookCommandCwd::Path(path) => Ok(format!(
            "cd {} && ",
            shell_arg(required_option("cwd.path", path)?)
        )),
        RunweaverHookCommandCwd::None => Ok(String::new()),
    }
}

fn required_option<'a>(name: &str, value: &'a str) -> Result<&'a str, HookCommandPrefixError> {
    if value.is_empty() {
        return Err(HookCommandPrefixError::EmptyOption {
            name: name.to_owned(),
        });
    }
    Ok(value)
}

fn shell_arg(value: &str) -> String {
    if is_safe_shell_token(value) {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn is_shell_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_safe_shell_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_@%+=:,./-".contains(ch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runweaver_hook_command_renders_current_agent_hook_prefixes() {
        let codex = runweaver_hook_command(
            &RunweaverHookCommandOptions::new(
                "codex",
                RunweaverHookCommandCwd::GitRoot,
                "runweaver.config.ts",
            )
            .with_export_name("agentHooksConfig"),
        )
        .unwrap();
        let claude = runweaver_hook_command(
            &RunweaverHookCommandOptions::new(
                "claude",
                RunweaverHookCommandCwd::Env("CLAUDE_PROJECT_DIR".to_owned()),
                "runweaver.config.ts",
            )
            .with_export_name("agentHooksConfig"),
        )
        .unwrap();

        assert_eq!(
            codex,
            "cd \"$(git rev-parse --show-toplevel)\" && ./node_modules/.bin/runweaver hook codex --config runweaver.config.ts --export agentHooksConfig"
        );
        assert_eq!(
            claude,
            "cd \"$CLAUDE_PROJECT_DIR\" && ./node_modules/.bin/runweaver hook claude --config runweaver.config.ts --export agentHooksConfig"
        );
    }

    #[test]
    fn compiled_runweaver_hook_command_renders_project_binary_prefixes_without_config_loader() {
        let command = compiled_runweaver_hook_command(&CompiledRunweaverHookCommandOptions::new(
            "codex",
            RunweaverHookCommandCwd::GitRoot,
            "./.runweaver/bin/demo",
        ))
        .unwrap();

        assert_eq!(
            command,
            "cd \"$(git rev-parse --show-toplevel)\" && ./.runweaver/bin/demo hook codex"
        );
    }

    #[test]
    fn runweaver_hook_command_quotes_shell_sensitive_option_values() {
        let command = runweaver_hook_command(
            &RunweaverHookCommandOptions::new(
                "fixture harness",
                RunweaverHookCommandCwd::Path("repo with spaces".to_owned()),
                "config dir/runweaver.config.ts",
            )
            .with_export_name("agentHooksConfig")
            .with_bin("bin dir/runweaver"),
        )
        .unwrap();

        assert_eq!(
            command,
            "cd 'repo with spaces' && 'bin dir/runweaver' hook 'fixture harness' --config 'config dir/runweaver.config.ts' --export agentHooksConfig"
        );
    }

    #[test]
    fn runweaver_hook_command_supports_current_directory_and_default_export() {
        let command = runweaver_hook_command(&RunweaverHookCommandOptions::new(
            "fixture",
            RunweaverHookCommandCwd::None,
            "runweaver.config.ts",
        ))
        .unwrap();

        assert_eq!(
            command,
            "./node_modules/.bin/runweaver hook fixture --config runweaver.config.ts"
        );
    }

    #[test]
    fn runweaver_hook_command_rejects_invalid_env_and_empty_fields() {
        let invalid_env = runweaver_hook_command(&RunweaverHookCommandOptions::new(
            "fixture",
            RunweaverHookCommandCwd::Env("FIXTURE PROJECT DIR".to_owned()),
            "runweaver.config.ts",
        ))
        .unwrap_err();
        let empty_harness = runweaver_hook_command(&RunweaverHookCommandOptions::new(
            "",
            RunweaverHookCommandCwd::None,
            "runweaver.config.ts",
        ))
        .unwrap_err();

        assert_eq!(
            invalid_env.to_string(),
            "Runweaver hook command env name must be a shell variable identifier: FIXTURE PROJECT DIR"
        );
        assert_eq!(
            empty_harness.to_string(),
            "Runweaver hook command harness must not be empty."
        );
    }
}
