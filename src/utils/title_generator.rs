use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_TITLE_CHARS: usize = 50;
pub const TITLE_TIMEOUT_SECS: u64 = 30;

const OPENCODE_DEFAULT_MODEL: &str = "opencode/gpt-5-nano";
const CLAUDE_DEFAULT_MODEL: &str = "claude-3-5-haiku-latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleGenerationConfig {
    pub enabled: bool,
    pub provider: TitleProvider,
    pub model_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleProvider {
    Auto,
    Claude,
    OpenCode,
}

impl TitleProvider {
    pub fn parse(value: &str) -> Self {
        match value {
            "claude" => Self::Claude,
            "opencode" => Self::OpenCode,
            _ => Self::Auto,
        }
    }

    fn executable(self) -> &'static str {
        match self {
            Self::Auto => "",
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandSpec {
    program: String,
    args: Vec<String>,
}

fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists() || std::env::var("FLATPAK_ID").is_ok()
}

fn command_exists(command: &str, flatpak: bool) -> bool {
    if flatpak {
        Command::new("flatpak-spawn")
            .arg("--host")
            .arg("which")
            .arg(command)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    } else {
        which::which(command).is_ok()
    }
}

fn provider_available(provider: TitleProvider) -> bool {
    match provider {
        TitleProvider::Auto => false,
        TitleProvider::Claude | TitleProvider::OpenCode => {
            command_exists(provider.executable(), is_flatpak())
        }
    }
}

pub fn detect_available_provider() -> Option<TitleProvider> {
    [TitleProvider::OpenCode, TitleProvider::Claude]
        .into_iter()
        .find(|provider| provider_available(*provider))
}

fn resolve_provider_chain_with<F>(
    config: &TitleGenerationConfig,
    is_available: F,
) -> Vec<TitleProvider>
where
    F: Fn(TitleProvider) -> bool,
{
    match config.provider {
        TitleProvider::Auto => [TitleProvider::OpenCode, TitleProvider::Claude]
            .into_iter()
            .filter(|provider| is_available(*provider))
            .collect(),
        provider => {
            if is_available(provider) {
                vec![provider]
            } else {
                Vec::new()
            }
        }
    }
}

pub fn resolve_provider_chain(config: &TitleGenerationConfig) -> Vec<TitleProvider> {
    resolve_provider_chain_with(config, provider_available)
}

pub fn default_model_for(provider: TitleProvider) -> Option<&'static str> {
    match provider {
        TitleProvider::Auto => None,
        TitleProvider::OpenCode => Some(OPENCODE_DEFAULT_MODEL),
        TitleProvider::Claude => Some(CLAUDE_DEFAULT_MODEL),
    }
}

fn wrap_for_host_execution(base_program: &str, base_args: &[String], flatpak: bool) -> CommandSpec {
    if flatpak {
        let mut args = vec!["--host".to_string(), base_program.to_string()];
        args.extend(base_args.iter().cloned());
        CommandSpec {
            program: "flatpak-spawn".to_string(),
            args,
        }
    } else {
        CommandSpec {
            program: base_program.to_string(),
            args: base_args.to_vec(),
        }
    }
}

fn build_command(
    provider: TitleProvider,
    prompt: &str,
    model_override: Option<&str>,
    auto_mode: bool,
    flatpak: bool,
) -> CommandSpec {
    match provider {
        TitleProvider::OpenCode => {
            let mut args = vec![
                "run".to_string(),
                prompt.to_string(),
                "--format".to_string(),
                "default".to_string(),
            ];

            let model = if auto_mode {
                Some(model_override.unwrap_or(OPENCODE_DEFAULT_MODEL))
            } else {
                model_override
            };

            if let Some(model) = model {
                args.push("--model".to_string());
                args.push(model.to_string());
            }

            wrap_for_host_execution("opencode", &args, flatpak)
        }
        TitleProvider::Claude => {
            let model = model_override.unwrap_or(CLAUDE_DEFAULT_MODEL);
            let args = vec![
                "-p".to_string(),
                prompt.to_string(),
                "--output-format".to_string(),
                "text".to_string(),
                "--permission-mode".to_string(),
                "plan".to_string(),
                "--tools".to_string(),
                "".to_string(),
                "--model".to_string(),
                model.to_string(),
            ];

            wrap_for_host_execution("claude", &args, flatpak)
        }
        TitleProvider::Auto => wrap_for_host_execution("", &[], flatpak),
    }
}

fn run_command_with_timeout(spec: &CommandSpec, timeout: Duration) -> Option<String> {
    let mut child = Command::new(&spec.program)
        .args(&spec.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }

                let mut stdout = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_string(&mut stdout);
                }

                if stdout.trim().is_empty() {
                    return None;
                } else {
                    return Some(stdout);
                }
            }
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
}

pub fn sanitize_generated_title(raw: &str) -> Option<String> {
    let line = raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");

    if line.is_empty() {
        return None;
    }

    let unquoted = line
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            line.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(line);

    let collapsed = unquoted.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = collapsed.chars().take(MAX_TITLE_CHARS).collect();
    let normalized = truncated.trim();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn build_generation_prompt(context: &str) -> String {
    format!(
        "Generate a concise session title (max {} characters). Return title only.\\n\\n{}",
        MAX_TITLE_CHARS, context
    )
}

pub fn generate_title(context: &str, config: &TitleGenerationConfig) -> Option<String> {
    if !config.enabled || context.trim().is_empty() {
        return None;
    }

    let prompt = build_generation_prompt(context);
    let chain = resolve_provider_chain(config);
    let flatpak = is_flatpak();
    let auto_mode = config.provider == TitleProvider::Auto;

    for provider in chain {
        let spec = build_command(
            provider,
            &prompt,
            config.model_override.as_deref(),
            auto_mode,
            flatpak,
        );

        if let Some(raw_output) =
            run_command_with_timeout(&spec, Duration::from_secs(TITLE_TIMEOUT_SECS))
            && let Some(title) = sanitize_generated_title(&raw_output)
        {
            return Some(title);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto_config() -> TitleGenerationConfig {
        TitleGenerationConfig {
            enabled: true,
            provider: TitleProvider::Auto,
            model_override: None,
        }
    }

    #[test]
    fn provider_parsing_and_auto_resolution_order_prioritizes_opencode() {
        assert_eq!(TitleProvider::parse("auto"), TitleProvider::Auto);
        assert_eq!(TitleProvider::parse("opencode"), TitleProvider::OpenCode);
        assert_eq!(TitleProvider::parse("claude"), TitleProvider::Claude);
        assert_eq!(TitleProvider::parse("invalid"), TitleProvider::Auto);

        let chain = resolve_provider_chain_with(&auto_config(), |provider| {
            matches!(provider, TitleProvider::OpenCode | TitleProvider::Claude)
        });
        assert_eq!(chain, vec![TitleProvider::OpenCode, TitleProvider::Claude]);
    }

    #[test]
    fn auto_mode_default_model_mapping_uses_expected_defaults() {
        assert_eq!(default_model_for(TitleProvider::OpenCode), Some(OPENCODE_DEFAULT_MODEL));
        assert_eq!(default_model_for(TitleProvider::Claude), Some(CLAUDE_DEFAULT_MODEL));
        assert_eq!(default_model_for(TitleProvider::Auto), None);
    }

    #[test]
    fn flatpak_host_wrapping_behavior_prepends_flatpak_spawn_host() {
        let spec = wrap_for_host_execution("opencode", &["run".to_string()], true);
        assert_eq!(spec.program, "flatpak-spawn");
        assert_eq!(spec.args[0], "--host");
        assert_eq!(spec.args[1], "opencode");
        assert_eq!(spec.args[2], "run");
    }

    #[test]
    fn command_argument_mapping_respects_auto_defaults_and_override() {
        let auto_opencode = build_command(TitleProvider::OpenCode, "Prompt", None, true, false);
        assert_eq!(auto_opencode.program, "opencode");
        assert!(auto_opencode.args.contains(&"--model".to_string()));
        assert!(auto_opencode
            .args
            .contains(&OPENCODE_DEFAULT_MODEL.to_string()));

        let explicit_opencode = build_command(
            TitleProvider::OpenCode,
            "Prompt",
            Some("custom-model"),
            false,
            false,
        );
        assert!(explicit_opencode.args.contains(&"--model".to_string()));
        assert!(explicit_opencode
            .args
            .contains(&"custom-model".to_string()));

        let explicit_opencode_no_model =
            build_command(TitleProvider::OpenCode, "Prompt", None, false, false);
        assert!(!explicit_opencode_no_model
            .args
            .contains(&"--model".to_string()));

        let claude = build_command(TitleProvider::Claude, "Prompt", None, true, false);
        assert_eq!(claude.program, "claude");
        assert!(claude.args.contains(&"--model".to_string()));
        assert!(claude.args.contains(&CLAUDE_DEFAULT_MODEL.to_string()));
    }

    #[test]
    fn sanitize_generated_title_takes_first_non_empty_line_and_truncates() {
        let raw = "\n  \"This is a very long generated title that should be truncated to fit the title limit\"\nignored";
        let title = sanitize_generated_title(raw).unwrap();
        assert!(title.chars().count() <= MAX_TITLE_CHARS);
        assert!(!title.starts_with('"'));
        assert!(!title.ends_with('"'));
    }

    #[test]
    fn timeout_path_returns_none() {
        let spec = CommandSpec {
            program: "bash".to_string(),
            args: vec!["-lc".to_string(), "sleep 1".to_string()],
        };

        let output = run_command_with_timeout(&spec, Duration::from_millis(50));
        assert!(output.is_none());
    }
}
