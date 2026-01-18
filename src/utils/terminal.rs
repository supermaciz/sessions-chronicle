use anyhow::{Context, Result};
use std::env;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").exists() || env::var("FLATPAK_ID").is_ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    Auto,
    Ptyxis,
    Ghostty,
    Foot,
    Alacritty,
    Kitty,
}

impl Terminal {
    const ALL: &'static [Terminal] = &[
        Terminal::Ptyxis,
        Terminal::Ghostty,
        Terminal::Foot,
        Terminal::Alacritty,
        Terminal::Kitty,
    ];

    pub fn to_str(self) -> &'static str {
        match self {
            Terminal::Auto => "auto",
            Terminal::Ptyxis => "ptyxis",
            Terminal::Ghostty => "ghostty",
            Terminal::Foot => "foot",
            Terminal::Alacritty => "alacritty",
            Terminal::Kitty => "kitty",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Terminal::Auto => "Automatic",
            Terminal::Ptyxis => "Ptyxis",
            Terminal::Ghostty => "Ghostty",
            Terminal::Foot => "Foot",
            Terminal::Alacritty => "Alacritty",
            Terminal::Kitty => "Kitty",
        }
    }

    pub fn executable(&self) -> Option<&'static str> {
        match self {
            Terminal::Auto => None,
            Terminal::Ptyxis => Some("ptyxis"),
            Terminal::Ghostty => Some("ghostty"),
            Terminal::Foot => Some("foot"),
            Terminal::Alacritty => Some("alacritty"),
            Terminal::Kitty => Some("kitty"),
        }
    }

    fn is_available(&self) -> bool {
        match self.executable() {
            Some(exe) => {
                if is_flatpak() {
                    Command::new("flatpak-spawn")
                        .arg("--host")
                        .arg("which")
                        .arg(exe)
                        .status()
                        .map(|status| status.success())
                        .unwrap_or(false)
                } else {
                    which::which(exe).is_ok()
                }
            }
            None => false,
        }
    }

    pub fn resolve_auto(&self) -> Result<Self> {
        if *self != Terminal::Auto {
            return Ok(*self);
        }

        for terminal in Self::ALL {
            if terminal.is_available() {
                return Ok(*terminal);
            }
        }

        Err(anyhow::anyhow!("No terminal emulator found"))
    }
}

impl FromStr for Terminal {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Terminal::Auto),
            "ptyxis" => Ok(Terminal::Ptyxis),
            "ghostty" => Ok(Terminal::Ghostty),
            "foot" => Ok(Terminal::Foot),
            "alacritty" => Ok(Terminal::Alacritty),
            "kitty" => Ok(Terminal::Kitty),
            _ => Err(()),
        }
    }
}

pub fn build_resume_command(session_id: &str, workdir: &Path) -> Result<Vec<String>> {
    let workdir = workdir
        .canonicalize()
        .context("Failed to canonicalize workdir")?;

    let shell_cmd = "cd \"$1\" && claude -r \"$2\"; exec bash".to_string();

    Ok(vec![
        "bash".to_string(),
        "-lc".to_string(),
        shell_cmd,
        "--".to_string(),
        workdir.to_string_lossy().to_string(),
        session_id.to_string(),
    ])
}

pub fn spawn_terminal(terminal: Terminal, args: &[String]) -> Result<()> {
    let resolved = terminal
        .resolve_auto()
        .context("Failed to resolve terminal")?;

    let executable = resolved
        .executable()
        .ok_or_else(|| anyhow::anyhow!("Terminal has no executable"))?;

    let mut command = if is_flatpak() {
        let mut cmd = Command::new("flatpak-spawn");
        cmd.arg("--host").arg(executable);
        cmd
    } else {
        Command::new(executable)
    };

    let mut final_args: Vec<String> = Vec::new();

    // Each terminal has different syntax for specifying the command to run
    match resolved {
        Terminal::Ghostty | Terminal::Alacritty | Terminal::Kitty => {
            final_args.push("-e".to_string());
        }
        Terminal::Ptyxis => {
            final_args.push("--".to_string());
        }
        Terminal::Foot => {
            // Foot takes the command directly without a separator
        }
        Terminal::Auto => unreachable!("Auto should be resolved"),
    }

    final_args.extend(args.iter().cloned());

    for arg in &final_args {
        command.arg(arg);
    }

    command
        .spawn()
        .context("Failed to spawn terminal process")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_terminal_from_str() {
        assert_eq!(Terminal::from_str("auto"), Ok(Terminal::Auto));
        assert_eq!(Terminal::from_str("ptyxis"), Ok(Terminal::Ptyxis));
        assert_eq!(Terminal::from_str("ghostty"), Ok(Terminal::Ghostty));
        assert_eq!(Terminal::from_str("foot"), Ok(Terminal::Foot));
        assert_eq!(Terminal::from_str("alacritty"), Ok(Terminal::Alacritty));
        assert_eq!(Terminal::from_str("kitty"), Ok(Terminal::Kitty));
        assert_eq!(Terminal::from_str("invalid"), Err(()));
    }

    #[test]
    fn test_terminal_to_str() {
        assert_eq!(Terminal::Auto.to_str(), "auto");
        assert_eq!(Terminal::Ptyxis.to_str(), "ptyxis");
        assert_eq!(Terminal::Ghostty.to_str(), "ghostty");
        assert_eq!(Terminal::Foot.to_str(), "foot");
        assert_eq!(Terminal::Alacritty.to_str(), "alacritty");
        assert_eq!(Terminal::Kitty.to_str(), "kitty");
    }

    #[test]
    fn test_build_resume_command() {
        let temp_dir = std::env::temp_dir();
        let project_dir = temp_dir.join("test-project");

        if !project_dir.exists() {
            std::fs::create_dir(&project_dir).ok();
        }

        let cmd = build_resume_command("test-session-id", &project_dir).unwrap();
        assert_eq!(cmd.len(), 6);
        assert_eq!(cmd[0], "bash");
        assert_eq!(cmd[1], "-lc");
        assert!(cmd[2].contains("claude -r"));
        assert_eq!(cmd[3], "--");
        assert!(cmd[4].ends_with("test-project"));
        assert_eq!(cmd[5], "test-session-id");
    }
}
