mod cli;

use std::io;

const INPUT_MASK: &str = "● ";

fn main() -> io::Result<()> {
    cli::encrypt::encrypt(INPUT_MASK)?;
    Ok(())
}
