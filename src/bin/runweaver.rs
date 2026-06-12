use std::io::{self, Read};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let env = process_hook_env();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let mut read_stdin = || -> anyhow::Result<String> {
        let mut stdin = String::new();
        io::stdin().read_to_string(&mut stdin)?;
        Ok(stdin)
    };

    let load_runweaver_config =
        |_request: runweaver::LoadRunweaverConfigRequest<'_>| unsupported_loader();
    let load_agent_hooks_config =
        |_request: runweaver::LoadRunweaverAgentHooksConfigRequest<'_>| unsupported_loader();
    let compile_binary = |_request: runweaver::CompileRunweaverBinaryRequest<'_>| {
        Err(anyhow::anyhow!(
            "The generic Rust runweaver binary cannot compile a project binary without a project-specific Rust compiler callback."
        ))
    };
    let generated_surface_files = Vec::new;
    let git_surface = || None;

    let exit_code = match runweaver::run_runweaver_cli(
        &args,
        runweaver::RunweaverCliRuntime {
            load_runweaver_config: &load_runweaver_config,
            load_agent_hooks_config: &load_agent_hooks_config,
            compile_binary: &compile_binary,
            generated_surface_files: &generated_surface_files,
            git_surface: &git_surface,
        },
        runweaver::RunweaverCliIo {
            stdin: runweaver::RunweaverStdin::Reader(&mut read_stdin),
            stdout: &mut stdout,
            stderr: &mut stderr,
            env: &env,
        },
    ) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    };
    std::process::exit(exit_code);
}

fn process_hook_env() -> runweaver::HookEnv {
    std::env::vars().collect()
}

fn unsupported_loader<T>() -> anyhow::Result<T> {
    Err(anyhow::anyhow!(
        "The generic Rust runweaver binary does not dynamically load authored config files. Use a project-specific compiled Rust binary instead."
    ))
}
