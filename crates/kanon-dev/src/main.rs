//! kanon-dev CLI tool skeleton

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "kanon-dev")]
#[command(about = "CLI tool for Kanon plugin project management, scaffolding, and testing")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Plugin related commands
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Start dev server with hot reload
    Dev,
    /// Run offline sandbox tests
    Test {
        path: Option<String>,
    },
}

#[derive(Subcommand)]
enum PluginAction {
    /// Create a new plugin from template
    Create {
        name: String,
        #[arg(long, default_value = "python")]
        lang: String,
    },
    /// Lint a plugin manifest
    Lint {
        path: Option<String>,
    },
    /// Pack a plugin into a distribution package
    Pack {
        path: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _cli = Cli::parse();
    println!("kanon-dev skeleton initialized");
    Ok(())
}
