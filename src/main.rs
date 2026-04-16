mod cli;

use std::io;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Encrypt a BIP39 mnemonic
    Encrypt,
    /// Decrypt a mnemonic encrypted with mnemolock
    Decrypt,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Encrypt => cli::encrypt::encrypt(),
        Commands::Decrypt => cli::decrypt::decrypt(),
    }
}
