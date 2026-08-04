use std::path::PathBuf;

use clap::Parser;
use sessions_chronicle_core::session_sources::database_path;
use tracing_subscriber::EnvFilter;

use sessions_chronicle_search_provider::{config::APP_ID, run};

#[derive(Debug, Parser)]
#[command(name = "sessions-chronicle-search-provider")]
struct Args {
    /// Use an explicit database instead of the default application database.
    #[arg(long)]
    database: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let database = args
        .database
        .unwrap_or_else(|| database_path(&glib::user_data_dir(), APP_ID, false));
    async_io::block_on(run(database))
}
