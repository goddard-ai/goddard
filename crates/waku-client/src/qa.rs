//! Local QA terminal commands, shared by desktop clients.
use waku_protocol::{custom_commands::CustomCommand, workspace::QaConfig};

/// Install and serve in separate shells so a failed installation never starts
/// the server, including when either editable command contains multiple lines.
pub fn server_command(config: &QaConfig) -> CustomCommand {
    server_command_with_install(config, false)
}

/// A forced install still records the successful dependency fingerprint.
pub fn server_command_with_install(config: &QaConfig, force_install: bool) -> CustomCommand {
    #[cfg(not(windows))]
    let (script, shell) = {
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        let dev = format!("sh -c {}", quote(&config.dev_command));
        let script = if config.install_command.is_empty() {
            dev
        } else {
            if config.primary_checkout {
                let install = quote(&config.install_command);
                let force = if force_install { "true" } else { "false" };
                let ready = if config.dependencies_ready {
                    "true"
                } else {
                    "false"
                };
                format!(
                    r#"qa_git=$(git rev-parse --absolute-git-dir) && qa_key=$({{ printf '%s\n' {install}; git ls-files --stage -- 'package.json' '**/package.json' '*.lock' '*.lockb' '*-lock.json' '*-lock.yaml' 'npm-shrinkwrap.json' '**/npm-shrinkwrap.json' 'packages.lock.json' '**/packages.lock.json' '.npmrc' '**/.npmrc' 'pnpm-workspace.yaml' 'bunfig.toml' 'Cargo.toml' '**/Cargo.toml' 'pyproject.toml' '**/pyproject.toml' 'requirements*.txt'; }} | git hash-object --stdin) && qa_stamp="$qa_git/goddard-qa-install" && if {force} || [ "$(cat "$qa_stamp" 2>/dev/null)" != "$qa_key" ]; then if ! {force} && [ ! -f "$qa_stamp" ] && {ready}; then :; else sh -c {install} || exit $?; fi && printf '%s\n' "$qa_key" > "$qa_stamp" || exit $?; fi && {dev}"#
                )
            } else {
                format!("sh -c {} && {dev}", quote(&config.install_command))
            }
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
            let install = quote(&config.install_command);
            if config.primary_checkout {
                let force = if force_install { "$true" } else { "$false" };
                let ready = if config.dependencies_ready {
                    "$true"
                } else {
                    "$false"
                };
                format!(
                    r#"$ErrorActionPreference = 'Stop'; $qaGit = git rev-parse --absolute-git-dir; if ($LASTEXITCODE) {{ exit $LASTEXITCODE }}; $qaInputs = git ls-files --stage -- 'package.json' '**/package.json' '*.lock' '*.lockb' '*-lock.json' '*-lock.yaml' 'npm-shrinkwrap.json' '**/npm-shrinkwrap.json' 'packages.lock.json' '**/packages.lock.json' '.npmrc' '**/.npmrc' 'pnpm-workspace.yaml' 'bunfig.toml' 'Cargo.toml' '**/Cargo.toml' 'pyproject.toml' '**/pyproject.toml' 'requirements*.txt'; if ($LASTEXITCODE) {{ exit $LASTEXITCODE }}; $qaKey = (@({install}) + @($qaInputs)) | git hash-object --stdin; if ($LASTEXITCODE) {{ exit $LASTEXITCODE }}; $qaStamp = Join-Path $qaGit 'goddard-qa-install'; $qaOld = if (Test-Path $qaStamp) {{ (Get-Content $qaStamp -Raw).Trim() }} else {{ '' }}; if ({force} -or $qaOld -ne $qaKey) {{ if ({force} -or (Test-Path $qaStamp) -or !{ready}) {{ powershell.exe -NoProfile -Command {install}; if ($LASTEXITCODE) {{ exit $LASTEXITCODE }} }}; Set-Content $qaStamp $qaKey }}; {dev}"#
                )
            } else {
                format!("powershell.exe -NoProfile -Command {install}; if ($?) {{ {dev} }}")
            }
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
    fn primary_install_reuses_success_and_invalidates_on_dependency_changes() {
        let root = std::env::temp_dir().join(format!("qa-install-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            )
        };
        git(&["init", "--quiet"]);
        std::fs::write(root.join("package.json"), "{}").unwrap();
        git(&["add", "package.json"]);
        let config = QaConfig {
            primary_checkout: true,
            install_command: "echo installed >> .git/install-count".into(),
            dev_command: "true".into(),
            ..Default::default()
        };
        let run = |config: &QaConfig| {
            let command = server_command(config);
            std::process::Command::new(command.shell.unwrap())
                .args(["-c", &command.script])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        };
        assert!(run(&config));
        assert!(run(&config));
        assert_eq!(
            std::fs::read_to_string(root.join(".git/install-count"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        std::fs::write(root.join("package.json"), "{\"changed\":true}").unwrap();
        git(&["add", "package.json"]);
        assert!(run(&config));
        assert_eq!(
            std::fs::read_to_string(root.join(".git/install-count"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        std::fs::write(root.join("clock.rs"), "source change").unwrap();
        git(&["add", "clock.rs"]);
        assert!(run(&config));
        assert_eq!(
            std::fs::read_to_string(root.join(".git/install-count"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        std::fs::write(root.join("bun.lock"), "changed dependency").unwrap();
        git(&["add", "bun.lock"]);
        assert!(run(&config));
        assert_eq!(
            std::fs::read_to_string(root.join(".git/install-count"))
                .unwrap()
                .lines()
                .count(),
            3
        );
        let forced = server_command_with_install(&config, true);
        assert!(
            std::process::Command::new(forced.shell.unwrap())
                .args(["-c", &forced.script])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(run(&config));
        assert_eq!(
            std::fs::read_to_string(root.join(".git/install-count"))
                .unwrap()
                .lines()
                .count(),
            4
        );
        let failed = QaConfig {
            install_command: "exit 17".into(),
            dev_command: "touch .git/served-after-failure".into(),
            ..config.clone()
        };
        assert!(!run(&failed));
        assert!(!root.join(".git/served-after-failure").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn qa_server_waits_for_install_and_preserves_shell_input() {
        let mut config = QaConfig {
            install_command: "printf 'installed\\n'".into(),
            dev_command: "printf '%s\\n' \"server's ready\"".into(),
            branch: "dev".into(),
            ..Default::default()
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
