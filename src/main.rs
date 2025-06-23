use clap::{Parser, Subcommand};
use flake_inputs::{FlakeInputsArgs, replace_inputs};

use flake_home::FlakeHomeArgs;
use rnix::ast::AttrSet;

use crate::flake_home::replace_flake_home;

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
    }
    
}

#[cfg(test)]
mod test {

    #[test]
    fn flake_inputs() {
        use crate::flake_inputs::{replace_inputs, FlakeInputsArgs};
        use std::path::PathBuf;

        let args = FlakeInputsArgs {
            expression_path: PathBuf::from("/Users/kemengjian/workspace/git/nixos-config/lib/select_device_build_inputs.nix"),
            flake_path: PathBuf::from("/Users/kemengjian/workspace/git/nixos-config/flake.nix"),
        };
        let _ = replace_inputs(args.expression_path, args.flake_path);
    }

    #[test]
    fn flake_home() {
        use crate::flake_home::{replace_flake_home, FlakeHomeArgs};
        use std::path::PathBuf;

        let args = FlakeHomeArgs {
            self_var_path: PathBuf::from("/Users/kemengjian/workspace/git/nixos-config/self.nix"),
            property: "flakeHome".to_string(),
            flake_home_dir: Some("/Users/kemengjian/workspace/git/nixos-config/".to_string().into()),
        };
        let _ = replace_flake_home(args.self_var_path, args.property, args.flake_home_dir);
    }

}
