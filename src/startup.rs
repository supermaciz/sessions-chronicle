use std::path::{Path, PathBuf};

use clap::Parser;

#[derive(Debug, Parser, PartialEq, Eq)]
pub(crate) struct LocalArgs {
    /// Override session source root directory.
    #[arg(long, value_name = "DIR")]
    pub(crate) sessions_dir: Option<PathBuf>,

    /// Print the resolved SQLite database path and exit.
    #[arg(long)]
    pub(crate) print_db_path: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub(crate) local: LocalArgs,
    pub(crate) gapplication_args: Vec<String>,
}

pub(crate) fn parse_invocation(
    args: impl IntoIterator<Item = String>,
) -> Result<Invocation, clap::Error> {
    let mut args = args.into_iter();
    let program = args
        .next()
        .unwrap_or_else(|| "sessions-chronicle".to_string());
    let mut local_args = vec![program.clone()];
    let mut gapplication_args = vec![program];
    let mut args = args.peekable();
    let mut after_delimiter = false;

    while let Some(arg) = args.next() {
        if after_delimiter {
            gapplication_args.push(arg);
            continue;
        }

        match arg.as_str() {
            "--" => {
                after_delimiter = true;
                gapplication_args.push(arg);
            }
            "--sessions-dir" => {
                local_args.push(arg);
                if args
                    .peek()
                    .is_some_and(|value| value != "--" && !value.starts_with('-'))
                {
                    local_args.push(args.next().unwrap());
                }
            }
            "--print-db-path" | "-h" | "--help" => local_args.push(arg),
            _ if arg.starts_with("--sessions-dir=") => local_args.push(arg),
            _ => gapplication_args.push(arg),
        }
    }

    Ok(Invocation {
        local: LocalArgs::try_parse_from(local_args)?,
        gapplication_args,
    })
}

pub(crate) fn allow_multiple_instances(sessions_dir: Option<&Path>) -> bool {
    sessions_dir.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn gapplication_service_is_forwarded() {
        let invocation =
            parse_invocation(strings(&["sessions-chronicle", "--gapplication-service"])).unwrap();

        assert_eq!(
            invocation.gapplication_args,
            strings(&["sessions-chronicle", "--gapplication-service"])
        );
        assert_eq!(invocation.local.sessions_dir, None);
        assert!(!invocation.local.print_db_path);
    }

    #[test]
    fn sessions_dir_after_gtk_option_is_extracted() {
        let invocation = parse_invocation(strings(&[
            "sessions-chronicle",
            "--display=:1",
            "--sessions-dir",
            "tests/fixtures",
        ]))
        .unwrap();

        assert_eq!(
            invocation.local.sessions_dir,
            Some(PathBuf::from("tests/fixtures"))
        );
        assert_eq!(
            invocation.gapplication_args,
            strings(&["sessions-chronicle", "--display=:1"])
        );
    }

    #[test]
    fn sessions_dir_equals_form_is_extracted() {
        let invocation = parse_invocation(strings(&[
            "sessions-chronicle",
            "--sessions-dir=tests/fixtures",
        ]))
        .unwrap();

        assert_eq!(
            invocation.local.sessions_dir,
            Some(PathBuf::from("tests/fixtures"))
        );
        assert_eq!(
            invocation.gapplication_args,
            strings(&["sessions-chronicle"])
        );
    }

    #[test]
    fn print_db_path_after_service_option_stays_local() {
        let invocation = parse_invocation(strings(&[
            "sessions-chronicle",
            "--gapplication-service",
            "--print-db-path",
        ]))
        .unwrap();

        assert!(invocation.local.print_db_path);
        assert_eq!(
            invocation.gapplication_args,
            strings(&["sessions-chronicle", "--gapplication-service"])
        );
    }

    #[test]
    fn delimiter_forwards_every_following_argument_unchanged() {
        let invocation = parse_invocation(strings(&[
            "sessions-chronicle",
            "--",
            "--sessions-dir",
            "tests/fixtures",
            "--print-db-path",
        ]))
        .unwrap();

        assert_eq!(invocation.local.sessions_dir, None);
        assert!(!invocation.local.print_db_path);
        assert_eq!(
            invocation.gapplication_args,
            strings(&[
                "sessions-chronicle",
                "--",
                "--sessions-dir",
                "tests/fixtures",
                "--print-db-path",
            ])
        );
    }

    #[test]
    fn missing_and_duplicate_local_options_remain_clap_errors() {
        assert!(parse_invocation(strings(&["sessions-chronicle", "--sessions-dir"])).is_err());
        assert!(
            parse_invocation(strings(&[
                "sessions-chronicle",
                "--sessions-dir=one",
                "--sessions-dir=two",
            ]))
            .is_err()
        );
        assert!(
            parse_invocation(strings(&[
                "sessions-chronicle",
                "--print-db-path",
                "--print-db-path",
            ]))
            .is_err()
        );
    }

    #[test]
    fn only_override_mode_allows_multiple_instances() {
        assert!(!allow_multiple_instances(None));
        assert!(allow_multiple_instances(Some(Path::new("tests/fixtures"))));
    }
}
