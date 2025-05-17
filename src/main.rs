use clap::{Parser, Subcommand};
use flake_inputs::{FlakeInputsArgs, replace_inputs};

mod flake_inputs;

#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate inputs in flake.nix through Nix expressions.
    FlakeInputs(FlakeInputsArgs),
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::FlakeInputs(FlakeInputsArgs {
            expression_path,
            flake_path,
        }) => {
            let _ = replace_inputs(expression_path, flake_path);
        }
    }
}
