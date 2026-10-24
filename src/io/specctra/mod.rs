//! Specctra DSN / SES file I/O (port of `app.freerouting.io.specctra`).
//!
//! * [`read_dsn`] / [`read_dsn_str`]: reads a DSN design file into a [`Board`]
//!   (`DsnReader.readBoard`), building the board through the same sequence of
//!   insertions as the original, so that item ids are identical.
//! * [`write_ses`]: writes a Specctra session file (`SesWriter.write`).
//! * [`read_ses`]: imports the wires and vias of a session file (`SesReader.read`).
//! * [`read_rules`] / [`write_rules`]: Specctra `.rules` files (`RulesReader`, `RulesWriter`).

mod autoroute_settings;
mod java_fmt;
mod lexer;
mod library;
mod network;
mod part_library;
mod placement;
mod reader;
mod rules_file;
mod scopes;
mod ses_reader;
mod ses_writer;
mod shapes;
mod structure;
mod wiring;

#[cfg(test)]
mod tests;

pub use autoroute_settings::{DsnAutorouteSettings, DsnLayerAutorouteSettings};
pub use java_fmt::{java_double_to_string, java_plain_string};
pub use reader::{adjust_plane_autoroute_settings, read_dsn_str, DsnError, DsnReadResult};
pub use rules_file::{read_rules, read_rules_str, rules_string, write_rules, RulesReadResult};
pub use ses_reader::{read_ses, read_ses_str, SesImportSummary};
pub use ses_writer::{ses_string, write_ses, SesError};

use crate::board::Board;
use std::io::Read;
use std::path::Path;

/// Reads a DSN file.
pub fn read_dsn(path: impl AsRef<Path>) -> Result<DsnReadResult, DsnError> {
    let bytes = std::fs::read(path)?;
    Ok(read_dsn_bytes(&bytes)?)
}

/// Reads a DSN design from a reader (the input is decoded as UTF-8, malformed
/// sequences are replaced like Java's `InputStreamReader` does).
pub fn read_dsn_from_reader<R: Read>(mut reader: R) -> Result<DsnReadResult, DsnError> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    read_dsn_bytes(&bytes)
}

fn read_dsn_bytes(bytes: &[u8]) -> Result<DsnReadResult, DsnError> {
    let text = String::from_utf8_lossy(bytes);
    read_dsn_str(&text)
}

/// Convenience: writes the session file of `board` to `path`.
pub fn write_ses_file(board: &Board, design_name: &str, path: impl AsRef<Path>) -> Result<(), SesError> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    write_ses(board, design_name, &mut f)
}
