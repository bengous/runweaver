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

    let exit_code = match runweaver::run_generic_runweaver_cli(
        &args,
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
