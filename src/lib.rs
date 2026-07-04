//! afs-as: Standalone ARM64 assembler for macOS
//!
//! Encodes ARM64 instructions, parses assembly text, and emits Mach-O object files.
//! Usable as a library (called by the ARMFORTAS compiler) or as a standalone CLI tool.

// Binary literal groupings follow ARM instruction field boundaries (e.g., sf|opc|fixed),
// not Rust's default byte groupings. This is intentional and more readable for ISA work.
#![allow(clippy::unusual_byte_groupings)]

pub mod assemble;
pub mod encode;
pub mod expr;
pub mod lex;
pub mod elf;
pub mod macho;
pub mod parse;

/// ARM64 register definitions.
pub mod reg;
