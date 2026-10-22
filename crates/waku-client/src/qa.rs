//! Local QA terminal commands, shared by desktop clients.
use waku_protocol::{custom_commands::CustomCommand, workspace::QaConfig};

/// Install and serve in separate shells so a failed installation never starts
/// the server, including when either editable command contains multiple lines.
pub fn server_command(config: &QaConfig) -> CustomCommand {
    #[cfg(not(windows))]
    let (script, shell) = {
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        let dev = format!("sh -c {}", quote(&config.dev_command));
        let script = if config.install_command.is_empty() {
            dev
        } else {
            format!("sh -c {} && {dev}", quote(&config.install_command))
        };
        (script, "/bin/sh")
    };
    #[cfg(windows)]
    let (script, shell) = {
        let quote = |value: &str| format!("'{}'", value.replace('\'', "''"));
        let dev = format!(
            "powershell.exe -NoProfile -Command {}",
            quote(&config.dev_command)
        );
        let script = if config.install_command.is_empty() {
            dev
        } else {
            format!(
                "powershell.exe -NoProfile -Command {}; if ($?) {{ {dev} }}",
                quote(&config.install_command)
            )
        };
        (script, "powershell.exe")
    };
    let mut command = CustomCommand::new(script);
    command.shell = Some(shell.into());
    command
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn qa_server_waits_for_install_and_preserves_shell_input() {
        let mut config = QaConfig {
            install_command: "printf 'installed\\n'".into(),
            dev_command: "printf '%s\\n' \"server's ready\"".into(),
            branch: "dev".into(),
        };
        let run = |config: &QaConfig| {
            let command = server_command(config);
            std::process::Command::new(command.shell.unwrap())
                .args(["-c", &command.script])
                .output()
                .unwrap()
        };
        let output = run(&config);
        assert!(output.status.success());
        assert_eq!(output.stdout, b"installed\nserver's ready\n");
        config.install_command = "printf 'failed\\n'\nexit 17".into();
        let output = run(&config);
        assert_eq!(output.status.code(), Some(17));
        assert_eq!(output.stdout, b"failed\n");
        config.install_command.clear();
        assert_eq!(run(&config).stdout, b"server's ready\n");
    }
}
