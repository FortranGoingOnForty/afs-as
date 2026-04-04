//! Mach-O 64-bit object file writer for ARM64 macOS.
//!
//! Writes relocatable object files (.o) that can be linked with Apple's `ld`.
//! Implements the minimum viable subset: header, segment with __text and __data
//! sections, symbol table, dynamic symbol table, build version, and relocations.

use std::io::{self, Write};

// ---- Mach-O Constants ----

const MH_MAGIC_64: u32 = 0xFEEDFACF;
const CPU_TYPE_ARM64: u32 = 0x0100000C;
const CPU_SUBTYPE_ARM64_ALL: u32 = 0x00000000;
const MH_OBJECT: u32 = 1;
const MH_SUBSECTIONS_VIA_SYMBOLS: u32 = 0x2000;

const LC_SEGMENT_64: u32 = 0x19;
const LC_SYMTAB: u32 = 0x02;
const LC_DYSYMTAB: u32 = 0x0B;
const LC_BUILD_VERSION: u32 = 0x32;

const S_REGULAR: u32 = 0x0;
const S_ATTR_PURE_INSTRUCTIONS: u32 = 0x80000000;
const S_ATTR_SOME_INSTRUCTIONS: u32 = 0x00000400;

const PLATFORM_MACOS: u32 = 1;

// nlist_64 type bits
const N_UNDF: u8 = 0x00;
const N_SECT: u8 = 0x0E;
const N_EXT: u8 = 0x01;

// Relocation types
pub const ARM64_RELOC_UNSIGNED: u32 = 0;
pub const ARM64_RELOC_BRANCH26: u32 = 2;
pub const ARM64_RELOC_PAGE21: u32 = 3;
pub const ARM64_RELOC_PAGEOFF12: u32 = 4;

// Struct sizes
const HEADER_SIZE: u32 = 32;
const SEGMENT_CMD_SIZE: u32 = 72;
const SECTION_SIZE: u32 = 80;
const SYMTAB_CMD_SIZE: u32 = 24;
const DYSYMTAB_CMD_SIZE: u32 = 80;
const BUILD_VERSION_CMD_SIZE: u32 = 24;
const NLIST_SIZE: u32 = 16;
const RELOC_SIZE: u32 = 8;

/// A symbol in the object file.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub section: u8,    // 1-based section index, or 0 for N_UNDF
    pub value: u64,     // offset within section
    pub global: bool,   // N_EXT flag
    pub undefined: bool, // true for external references
}

/// A relocation entry.
#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: u32,       // byte offset in section
    pub symbol_idx: u32,   // index into symbol table
    pub pcrel: bool,       // PC-relative?
    pub length: u8,        // 2 = 4 bytes (32-bit)
    pub extern_: bool,     // true = symbol index, false = section number
    pub reloc_type: u32,   // ARM64_RELOC_*
}

/// Assembled object file ready for Mach-O emission.
#[derive(Debug, Clone)]
pub struct ObjectFile {
    pub text: Vec<u8>,
    pub data: Vec<u8>,
    pub symbols: Vec<Symbol>,
    pub text_relocs: Vec<Relocation>,
    pub text_align: u32,   // power of 2
}

impl Default for ObjectFile {
    fn default() -> Self {
        Self {
            text: Vec::new(),
            data: Vec::new(),
            symbols: Vec::new(),
            text_relocs: Vec::new(),
            text_align: 0,
        }
    }
}

impl ObjectFile {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Write a Mach-O object file to the given writer.
pub fn write_macho<W: Write>(obj: &ObjectFile, w: &mut W) -> io::Result<()> {
    let has_data = !obj.data.is_empty();
    let nsects: u32 = if has_data { 2 } else { 1 };

    // Compute layout.
    let segment_cmdsize = SEGMENT_CMD_SIZE + nsects * SECTION_SIZE;
    let ncmds: u32 = 4; // LC_SEGMENT_64, LC_BUILD_VERSION, LC_SYMTAB, LC_DYSYMTAB
    let sizeofcmds = segment_cmdsize + BUILD_VERSION_CMD_SIZE + SYMTAB_CMD_SIZE + DYSYMTAB_CMD_SIZE;

    let content_offset = HEADER_SIZE + sizeofcmds;

    // Align text section start.
    let text_offset = align_to(content_offset, 1 << obj.text_align);
    let text_size = obj.text.len() as u32;

    let data_offset = text_offset + text_size;
    let data_size = obj.data.len() as u32;

    // Relocations follow section data.
    let reloc_offset = data_offset + data_size;
    let nrelocs = obj.text_relocs.len() as u32;
    let reloc_size = nrelocs * RELOC_SIZE;

    // Symbol table follows relocations.
    let symoff = reloc_offset + reloc_size;
    let nsyms = obj.symbols.len() as u32;
    let sym_size = nsyms * NLIST_SIZE;

    // String table follows symbol table.
    let stroff = symoff + sym_size;
    let strtab = build_string_table(&obj.symbols);
    let strsize = strtab.len() as u32;

    // Classify symbols for LC_DYSYMTAB.
    let nlocalsym = obj.symbols.iter().filter(|s| !s.global && !s.undefined).count() as u32;
    let nextdefsym = obj.symbols.iter().filter(|s| s.global && !s.undefined).count() as u32;
    let nundefsym = obj.symbols.iter().filter(|s| s.undefined).count() as u32;

    let vmsize = if has_data {
        (obj.data.len() as u64) + (data_offset - text_offset) as u64
    } else {
        text_size as u64
    };
    let filesize = text_size + data_size;

    // ---- Write header ----
    write_u32(w, MH_MAGIC_64)?;
    write_u32(w, CPU_TYPE_ARM64)?;
    write_u32(w, CPU_SUBTYPE_ARM64_ALL)?;
    write_u32(w, MH_OBJECT)?;
    write_u32(w, ncmds)?;
    write_u32(w, sizeofcmds)?;
    write_u32(w, MH_SUBSECTIONS_VIA_SYMBOLS)?;
    write_u32(w, 0)?; // reserved

    // ---- LC_SEGMENT_64 ----
    write_u32(w, LC_SEGMENT_64)?;
    write_u32(w, segment_cmdsize)?;
    write_pad16(w, b"")?;           // segname (empty for object files)
    write_u64(w, 0)?;               // vmaddr
    write_u64(w, vmsize)?;          // vmsize
    write_u64(w, text_offset as u64)?; // fileoff
    write_u64(w, filesize as u64)?; // filesize
    write_u32(w, 7)?;               // maxprot (rwx)
    write_u32(w, 7)?;               // initprot (rwx)
    write_u32(w, nsects)?;
    write_u32(w, 0)?;               // flags

    // Section: __TEXT,__text
    write_pad16(w, b"__text")?;     // sectname
    write_pad16(w, b"__TEXT")?;     // segname
    write_u64(w, 0)?;               // addr
    write_u64(w, text_size as u64)?;
    write_u32(w, text_offset)?;     // offset
    write_u32(w, obj.text_align)?;  // align (power of 2)
    write_u32(w, if nrelocs > 0 { reloc_offset } else { 0 })?; // reloff
    write_u32(w, nrelocs)?;         // nreloc
    write_u32(w, S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS)?;
    write_u32(w, 0)?;               // reserved1
    write_u32(w, 0)?;               // reserved2
    write_u32(w, 0)?;               // reserved3 (padding to 80 bytes)

    // Section: __DATA,__data (if non-empty)
    if has_data {
        let data_addr = data_offset - text_offset; // relative to segment start
        write_pad16(w, b"__data")?;
        write_pad16(w, b"__DATA")?;
        write_u64(w, data_addr as u64)?;
        write_u64(w, data_size as u64)?;
        write_u32(w, data_offset)?;
        write_u32(w, 0)?;           // align
        write_u32(w, 0)?;           // reloff
        write_u32(w, 0)?;           // nreloc
        write_u32(w, S_REGULAR)?;
        write_u32(w, 0)?;
        write_u32(w, 0)?;
        write_u32(w, 0)?;
    }

    // ---- LC_BUILD_VERSION ----
    write_u32(w, LC_BUILD_VERSION)?;
    write_u32(w, BUILD_VERSION_CMD_SIZE)?;
    write_u32(w, PLATFORM_MACOS)?;
    write_u32(w, pack_version(15, 0, 0))?; // minos 15.0.0
    write_u32(w, 0)?;               // sdk (0 = n/a)
    write_u32(w, 0)?;               // ntools

    // ---- LC_SYMTAB ----
    write_u32(w, LC_SYMTAB)?;
    write_u32(w, SYMTAB_CMD_SIZE)?;
    write_u32(w, symoff)?;
    write_u32(w, nsyms)?;
    write_u32(w, stroff)?;
    write_u32(w, strsize)?;

    // ---- LC_DYSYMTAB ----
    write_u32(w, LC_DYSYMTAB)?;
    write_u32(w, DYSYMTAB_CMD_SIZE)?;
    write_u32(w, 0)?;               // ilocalsym
    write_u32(w, nlocalsym)?;
    write_u32(w, nlocalsym)?;       // iextdefsym
    write_u32(w, nextdefsym)?;
    write_u32(w, nlocalsym + nextdefsym)?; // iundefsym
    write_u32(w, nundefsym)?;
    // Rest is zeros (12 more u32 fields).
    for _ in 0..12 {
        write_u32(w, 0)?;
    }

    // ---- Padding to text_offset ----
    let current = HEADER_SIZE + sizeofcmds;
    let pad = (text_offset - current) as usize;
    w.write_all(&vec![0u8; pad])?;

    // ---- Section data ----
    w.write_all(&obj.text)?;
    w.write_all(&obj.data)?;

    // ---- Relocation entries ----
    for rel in &obj.text_relocs {
        write_reloc(w, rel)?;
    }

    // ---- Symbol table ----
    for (i, sym) in obj.symbols.iter().enumerate() {
        let str_offset = string_offset(&strtab, &sym.name);
        let n_type = if sym.undefined {
            N_UNDF | N_EXT
        } else if sym.global {
            N_SECT | N_EXT
        } else {
            N_SECT
        };
        write_u32(w, str_offset as u32)?;  // n_strx
        w.write_all(&[n_type])?;           // n_type
        w.write_all(&[sym.section])?;      // n_sect
        write_u16(w, 0)?;                  // n_desc
        write_u64(w, sym.value)?;          // n_value
        let _ = i;
    }

    // ---- String table ----
    w.write_all(&strtab)?;

    Ok(())
}

// ---- Helpers ----

fn build_string_table(symbols: &[Symbol]) -> Vec<u8> {
    let mut tab = vec![0u8]; // string table starts with a null byte
    for sym in symbols {
        tab.extend_from_slice(sym.name.as_bytes());
        tab.push(0);
    }
    // Pad to 4-byte alignment.
    while tab.len() % 4 != 0 {
        tab.push(0);
    }
    tab
}

fn string_offset(strtab: &[u8], name: &str) -> usize {
    let name_bytes = name.as_bytes();
    // Search for the null-terminated name in the string table.
    let mut pos = 1; // skip initial null
    while pos < strtab.len() {
        let end = strtab[pos..].iter().position(|&b| b == 0).unwrap() + pos;
        if &strtab[pos..end] == name_bytes {
            return pos;
        }
        pos = end + 1;
    }
    0 // fallback to empty string
}

fn write_reloc<W: Write>(w: &mut W, rel: &Relocation) -> io::Result<()> {
    // Mach-O relocation_info:
    // r_address: i32 (offset in section)
    // r_symbolnum:24, r_pcrel:1, r_length:2, r_extern:1, r_type:4
    write_u32(w, rel.offset)?;
    let info = (rel.symbol_idx & 0x00FFFFFF)
        | ((rel.pcrel as u32) << 24)
        | ((rel.length as u32 & 0x3) << 25)
        | ((rel.extern_ as u32) << 27)
        | ((rel.reloc_type & 0xF) << 28);
    write_u32(w, info)?;
    Ok(())
}

fn align_to(value: u32, align: u32) -> u32 {
    if align <= 1 { return value; }
    (value + align - 1) & !(align - 1)
}

fn pack_version(major: u32, minor: u32, patch: u32) -> u32 {
    (major << 16) | (minor << 8) | patch
}

fn write_u16<W: Write>(w: &mut W, v: u16) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn write_u32<W: Write>(w: &mut W, v: u32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn write_u64<W: Write>(w: &mut W, v: u64) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

/// Write a 16-byte padded name field (sectname/segname).
fn write_pad16<W: Write>(w: &mut W, name: &[u8]) -> io::Result<()> {
    let mut buf = [0u8; 16];
    let len = name.len().min(16);
    buf[..len].copy_from_slice(&name[..len]);
    w.write_all(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_object_is_valid_macho() {
        let obj = ObjectFile::new();
        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();

        // Check magic.
        assert_eq!(&buf[0..4], &MH_MAGIC_64.to_le_bytes());
        // Check CPU type.
        assert_eq!(&buf[4..8], &CPU_TYPE_ARM64.to_le_bytes());
        // Check file type.
        assert_eq!(&buf[12..16], &MH_OBJECT.to_le_bytes());
    }

    #[test]
    fn object_with_code_has_text_section() {
        let mut obj = ObjectFile::new();
        obj.text = vec![0xD5, 0x03, 0x20, 0x1F]; // NOP
        obj.symbols.push(Symbol {
            name: "_test".into(),
            section: 1,
            value: 0,
            global: true,
            undefined: false,
        });
        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();

        // Should be a valid Mach-O. Basic check: magic is present.
        assert_eq!(&buf[0..4], &MH_MAGIC_64.to_le_bytes());
        // File should be larger than just the header.
        assert!(buf.len() > 100);
    }

    #[test]
    fn string_table_lookup() {
        let syms = vec![
            Symbol { name: "_main".into(), section: 1, value: 0, global: true, undefined: false },
            Symbol { name: "msg".into(), section: 2, value: 0, global: false, undefined: false },
        ];
        let strtab = build_string_table(&syms);

        assert_eq!(strtab[0], 0); // initial null
        assert_eq!(string_offset(&strtab, "_main"), 1);
        assert_eq!(string_offset(&strtab, "msg"), 7);
    }

    #[test]
    fn relocation_encoding() {
        let rel = Relocation {
            offset: 4,
            symbol_idx: 1,
            pcrel: true,
            length: 2,
            extern_: true,
            reloc_type: ARM64_RELOC_PAGE21,
        };
        let mut buf = Vec::new();
        write_reloc(&mut buf, &rel).unwrap();
        assert_eq!(buf.len(), 8);
        // offset
        assert_eq!(&buf[0..4], &4u32.to_le_bytes());
        // info: sym=1, pcrel=1, length=2, extern=1, type=3
        let info = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        assert_eq!(info & 0x00FFFFFF, 1);        // symbolnum
        assert_eq!((info >> 24) & 1, 1);         // pcrel
        assert_eq!((info >> 25) & 3, 2);         // length
        assert_eq!((info >> 27) & 1, 1);         // extern
        assert_eq!((info >> 28) & 0xF, 3);       // type = PAGE21
    }

    #[test]
    fn align_to_works() {
        assert_eq!(align_to(0, 4), 0);
        assert_eq!(align_to(1, 4), 4);
        assert_eq!(align_to(4, 4), 4);
        assert_eq!(align_to(5, 4), 8);
        assert_eq!(align_to(100, 1), 100);
    }

    #[test]
    fn version_packing() {
        assert_eq!(pack_version(15, 0, 0), 0x000F0000);
        assert_eq!(pack_version(14, 5, 1), 0x000E0501);
    }
}
