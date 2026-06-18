use clap::{Parser, Subcommand};
use export_env::{ExportEnvArgs, export_env};
use flake_inputs::{FlakeInputsArgs, replace_inputs};

use flake_home::FlakeHomeArgs;

use crate::flake_home::replace_flake_home;

mod export_env;
mod flake_home;
mod flake_inputs;
mod util;

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
    /// Replace self attribute set file flake home dir
    FlakeHome(FlakeHomeArgs),
    /// Export environment variables from a file.
    ExportEnv(ExportEnvArgs),
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
        Commands::FlakeHome(FlakeHomeArgs {
            self_var_path,
            property,
            flake_home_dir,
        }) => {
            let _ = replace_flake_home(self_var_path, property, flake_home_dir);
        }
        Commands::ExportEnv(ExportEnvArgs { files, prefix }) => {
            let _ = export_env(files, prefix);
        }
    }
    
}