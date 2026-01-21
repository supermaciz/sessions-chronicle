use anyhow::{Context, Result};
use std::env;
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").exists() || env::var("FLATPAK_ID").is_ok()
}

/// Error type for terminal spawning operations
#[derive(Debug)]
pub enum TerminalSpawnError {
    /// No terminal emulator was found on the system
    NoTerminalFound,
    /// The specified terminal is not available
    NotAvailable(String),
    /// Other error occurred during terminal spawn
    Other(anyhow::Error),
}

impl std::fmt::Display for TerminalSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminalSpawnError::NoTerminalFound => write!(f, "No terminal emulator found"),
            TerminalSpawnError::NotAvailable(name) => write!(f, "{} is not available", name),
            TerminalSpawnError::Other(err) => write!(f, "{}", err),
        }
    }
}

impl std::error::Error for TerminalSpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TerminalSpawnError::Other(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

impl TerminalSpawnError {
    /// Returns true if this error should show a preferences button
    pub fn should_show_preferences(&self) -> bool {
        matches!(
            self,
            TerminalSpawnError::NoTerminalFound | TerminalSpawnError::NotAvailable(_)
        )
    }
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

    pub fn resolve_auto(&self) -> Result<Self, TerminalSpawnError> {
        if *self != Terminal::Auto {
            return Ok(*self);
        }

        for terminal in Self::ALL {
            if terminal.is_available() {
                return Ok(*terminal);
            }
        }

        Err(TerminalSpawnError::NoTerminalFound)
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

pub fn spawn_terminal(terminal: Terminal, args: &[String]) -> Result<(), TerminalSpawnError> {
    let resolved = terminal.resolve_auto()?;

    if !resolved.is_available() {
        return Err(TerminalSpawnError::NotAvailable(
            resolved.display_name().to_string(),
        ));
    }

    let executable = resolved
        .executable()
        .ok_or_else(|| TerminalSpawnError::Other(anyhow::anyhow!("Terminal has no executable")))?;

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

    command.spawn().map_err(|e| {
        TerminalSpawnError::Other(
            anyhow::Error::from(e).context("Failed to spawn terminal process"),
        )
    })?;

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

    #[test]
    fn test_terminal_spawn_error_display() {
        let err = TerminalSpawnError::NoTerminalFound;
        assert_eq!(err.to_string(), "No terminal emulator found");

        let err = TerminalSpawnError::NotAvailable("Ptyxis".to_string());
        assert_eq!(err.to_string(), "Ptyxis is not available");

        let err = TerminalSpawnError::Other(anyhow::anyhow!("Custom error"));
        assert_eq!(err.to_string(), "Custom error");
    }

    #[test]
    fn test_terminal_spawn_error_should_show_preferences() {
        let err = TerminalSpawnError::NoTerminalFound;
        assert!(err.should_show_preferences());

        let err = TerminalSpawnError::NotAvailable("Ptyxis".to_string());
        assert!(err.should_show_preferences());

        let err = TerminalSpawnError::Other(anyhow::anyhow!("Custom error"));
        assert!(!err.should_show_preferences());
    }

    #[test]
    fn test_resolve_auto_no_terminal_found() {
        // This test assumes no terminals are available, which may not always be true
        // It's more of a documentation of expected behavior
        let result = Terminal::Auto.resolve_auto();
        // Result depends on system state, but error should be NoTerminalFound if none available
        if result.is_err() {
            match result.unwrap_err() {
                TerminalSpawnError::NoTerminalFound => {
                    // Expected when no terminals are available
                }
                _ => panic!("Expected NoTerminalFound error"),
            }
        }
    }
}
