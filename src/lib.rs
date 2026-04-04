//! afs-as: Standalone ARM64 assembler for macOS
//!
//! Encodes ARM64 instructions, parses assembly text, and emits Mach-O object files.
//! Usable as a library (called by the ARMFORTAS compiler) or as a standalone CLI tool.

pub mod encode;
pub mod parse;
pub mod macho;

/// ARM64 register definitions.
pub mod reg;
