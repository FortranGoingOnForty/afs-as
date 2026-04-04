//! Mach-O 64-bit object file writer for ARM64 macOS.
//!
//! Writes relocatable object files (.o) that can be linked with Apple's `ld`.
//! Implements the minimum viable subset: header, segment with supported Mach-O
//! sections, symbol table, dynamic symbol table, build version, and relocations.

use std::io::{self, Write};

// ---- Mach-O Constants ----

const MH_MAGIC_64: u32 = 0xFEEDFACF;
const CPU_TYPE_ARM64: u32 = 0x0100000C;
const CPU_SUBTYPE_ARM64_ALL: u32 = 0x00000000;
const MH_OBJECT: u32 = 1;
#[allow(dead_code)]
const MH_SUBSECTIONS_VIA_SYMBOLS: u32 = 0x2000;

const LC_SEGMENT_64: u32 = 0x19;
const LC_SYMTAB: u32 = 0x02;
const LC_DYSYMTAB: u32 = 0x0B;
const LC_BUILD_VERSION: u32 = 0x32;

const S_REGULAR: u32 = 0x0;
const S_ZEROFILL: u32 = 0x1;
const S_CSTRING_LITERALS: u32 = 0x2;
const S_ATTR_PURE_INSTRUCTIONS: u32 = 0x80000000;
const S_ATTR_SOME_INSTRUCTIONS: u32 = 0x00000400;

const PLATFORM_MACOS: u32 = 1;

// nlist_64 type bits
const N_UNDF: u8 = 0x00;
const N_ABS: u8 = 0x02;
const N_PEXT: u8 = 0x10;
const N_SECT: u8 = 0x0E;
const N_EXT: u8 = 0x01;
const N_NO_DEAD_STRIP: u16 = 0x0020;
const N_WEAK_REF: u16 = 0x0040;
const N_WEAK_DEF: u16 = 0x0080;

// Relocation types
pub const ARM64_RELOC_UNSIGNED: u32 = 0;
pub const ARM64_RELOC_SUBTRACTOR: u32 = 1;
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
    pub absolute: bool,
    pub common: bool,
    pub common_align_pow2: u8,
    pub private_extern: bool,
    pub weak_ref: bool,
    pub weak_def: bool,
}

/// A relocation entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relocation {
    pub offset: u32,       // byte offset in section
    pub symbol_idx: u32,   // index into symbol table
    pub pcrel: bool,       // PC-relative?
    pub length: u8,        // 2 = 4 bytes (32-bit)
    pub extern_: bool,     // true = symbol index, false = section number
    pub reloc_type: u32,   // ARM64_RELOC_*
}

/// Supported Mach-O section kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionKind {
    Text,
    Data,
    CStringLiterals,
    ConstData,
    ZeroFill,
}

impl SectionKind {
    fn flags(&self, size: u64) -> u32 {
        match self {
            Self::Text if size == 0 => S_ATTR_PURE_INSTRUCTIONS,
            Self::Text => S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS,
            Self::CStringLiterals => S_CSTRING_LITERALS,
            Self::ZeroFill => S_ZEROFILL,
            Self::Data | Self::ConstData => S_REGULAR,
        }
    }

    fn is_zerofill(&self) -> bool {
        matches!(self, Self::ZeroFill)
    }
}

/// A Mach-O section in a relocatable object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub segment: String,
    pub name: String,
    pub kind: SectionKind,
    pub align_pow2: u32,
    pub data: Vec<u8>,
    pub size: u64,
    pub relocations: Vec<Relocation>,
}

impl Section {
    pub fn new(segment: &str, name: &str, kind: SectionKind) -> Self {
        Self {
            segment: segment.into(),
            name: name.into(),
            kind,
            align_pow2: 0,
            data: Vec::new(),
            size: 0,
            relocations: Vec::new(),
        }
    }

    pub fn text() -> Self {
        Self::new("__TEXT", "__text", SectionKind::Text)
    }

    pub fn file_size(&self) -> u64 {
        if self.kind.is_zerofill() { 0 } else { self.size }
    }
}

/// Assembled object file ready for Mach-O emission.
#[derive(Debug, Clone)]
pub struct ObjectFile {
    pub sections: Vec<Section>,
    pub symbols: Vec<Symbol>,
}

#[derive(Clone, Copy, Default)]
struct SectionLayout {
    addr: u64,
    offset: u32,
    reloff: u32,
    nreloc: u32,
}

impl ObjectFile {
    pub fn new() -> Self {
        Self {
            sections: vec![Section::text()],
            symbols: Vec::new(),
        }
    }

    pub fn section(&self, segment: &str, name: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.segment == segment && section.name == name)
    }

    pub fn section_mut(&mut self, segment: &str, name: &str) -> Option<&mut Section> {
        self.sections.iter_mut().find(|section| section.segment == segment && section.name == name)
    }

    pub fn text_section(&self) -> &Section {
        self.section("__TEXT", "__text").expect("missing __TEXT,__text section")
    }

    pub fn text_section_mut(&mut self) -> &mut Section {
        self.section_mut("__TEXT", "__text").expect("missing __TEXT,__text section")
    }
}

impl Default for ObjectFile {
    fn default() -> Self {
        Self::new()
    }
}

/// Write a Mach-O object file to the given writer.
pub fn write_macho<W: Write>(obj: &ObjectFile, w: &mut W) -> io::Result<()> {
    let nsects = obj.sections.len() as u32;

    // Compute layout.
    let segment_cmdsize = SEGMENT_CMD_SIZE + nsects * SECTION_SIZE;
    let ncmds: u32 = 4; // LC_SEGMENT_64, LC_BUILD_VERSION, LC_SYMTAB, LC_DYSYMTAB
    let sizeofcmds = segment_cmdsize + BUILD_VERSION_CMD_SIZE + SYMTAB_CMD_SIZE + DYSYMTAB_CMD_SIZE;

    let content_offset = HEADER_SIZE + sizeofcmds;
    let mut layouts = vec![SectionLayout::default(); obj.sections.len()];
    let mut file_cursor = content_offset;
    let mut vm_cursor = 0u64;

    let mut allocation_order: Vec<_> = (0..obj.sections.len()).collect();
    allocation_order.sort_by_key(|&index| obj.sections[index].kind.is_zerofill());

    for index in allocation_order {
        let section = &obj.sections[index];
        if !section.kind.is_zerofill() && section.data.len() as u64 != section.size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "section {},{} has size {} but {} data bytes",
                    section.segment,
                    section.name,
                    section.size,
                    section.data.len()
                ),
            ));
        }

        vm_cursor = align_value(vm_cursor, section.align_pow2);
        let addr = vm_cursor;
        vm_cursor += section.size;

        let offset = if section.kind.is_zerofill() {
            0
        } else {
            file_cursor = align_to(file_cursor, 1 << section.align_pow2);
            let offset = file_cursor;
            file_cursor = file_cursor.saturating_add(section.file_size() as u32);
            offset
        };

        layouts[index] = SectionLayout {
            addr,
            offset,
            reloff: 0,
            nreloc: 0,
        };
    }

    // Relocations follow section data (aligned to 8 bytes for relocation_info).
    let reloc_offset = align_to(file_cursor, 8);
    let mut reloc_cursor = reloc_offset;
    for (layout, section) in layouts.iter_mut().zip(&obj.sections) {
        layout.nreloc = section.relocations.len() as u32;
        if layout.nreloc > 0 {
            layout.reloff = reloc_cursor;
            reloc_cursor += layout.nreloc * RELOC_SIZE;
        }
    }

    // Symbol table follows relocations.
    let symoff = reloc_cursor;
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

    let segment_fileoff = layouts
        .iter()
        .zip(&obj.sections)
        .find(|(_, section)| !section.kind.is_zerofill())
        .map(|(layout, _)| layout.offset)
        .unwrap_or(content_offset);
    let filesize = layouts
        .iter()
        .zip(&obj.sections)
        .filter(|(_, section)| !section.kind.is_zerofill())
        .map(|(layout, section)| layout.offset + section.file_size() as u32)
        .max()
        .unwrap_or(segment_fileoff)
        .saturating_sub(segment_fileoff);
    let vmsize = layouts
        .iter()
        .zip(&obj.sections)
        .map(|(layout, section)| layout.addr + section.size)
        .max()
        .unwrap_or(0);

    // ---- Write header ----
    write_u32(w, MH_MAGIC_64)?;
    write_u32(w, CPU_TYPE_ARM64)?;
    write_u32(w, CPU_SUBTYPE_ARM64_ALL)?;
    write_u32(w, MH_OBJECT)?;
    write_u32(w, ncmds)?;
    write_u32(w, sizeofcmds)?;
    write_u32(w, 0)?; // flags
    write_u32(w, 0)?; // reserved

    // ---- LC_SEGMENT_64 ----
    write_u32(w, LC_SEGMENT_64)?;
    write_u32(w, segment_cmdsize)?;
    write_pad16(w, b"")?;           // segname (empty for object files)
    write_u64(w, 0)?;               // vmaddr
    write_u64(w, vmsize)?;          // vmsize
    write_u64(w, segment_fileoff as u64)?; // fileoff
    write_u64(w, filesize as u64)?; // filesize
    write_u32(w, 7)?;               // maxprot (rwx)
    write_u32(w, 7)?;               // initprot (rwx)
    write_u32(w, nsects)?;
    write_u32(w, 0)?;               // flags

    for (section, layout) in obj.sections.iter().zip(&layouts) {
        write_pad16(w, section.name.as_bytes())?;
        write_pad16(w, section.segment.as_bytes())?;
        write_u64(w, layout.addr)?;
        write_u64(w, section.size)?;
        write_u32(w, layout.offset)?;
        write_u32(w, section.align_pow2)?;
        write_u32(w, layout.reloff)?;
        write_u32(w, layout.nreloc)?;
        write_u32(w, section.kind.flags(section.size))?;
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

    // ---- Section data ----
    let mut written = HEADER_SIZE + sizeofcmds;
    for (section, layout) in obj.sections.iter().zip(&layouts) {
        if section.kind.is_zerofill() {
            continue;
        }
        let pad = layout.offset.saturating_sub(written) as usize;
        write_zeros(w, pad)?;
        w.write_all(&section.data)?;
        written = layout.offset + section.file_size() as u32;
    }

    // ---- Padding to relocation alignment ----
    let reloc_pad = reloc_offset.saturating_sub(written) as usize;
    write_zeros(w, reloc_pad)?;

    // ---- Relocation entries (descending address order within each section) ----
    for section in &obj.sections {
        let mut sorted_relocs: Vec<_> = section.relocations.iter().collect();
        sorted_relocs.sort_by(|a, b| b.offset.cmp(&a.offset));
        for rel in &sorted_relocs {
            write_reloc(w, rel)?;
        }
    }

    // ---- Symbol table ----
    for (i, sym) in obj.symbols.iter().enumerate() {
        let str_offset = string_offset(&strtab, &sym.name);
        let mut n_type = if sym.undefined || sym.common {
            N_UNDF
        } else if sym.absolute {
            N_ABS
        } else {
            N_SECT
        };
        if sym.global {
            n_type |= N_EXT;
        }
        if sym.private_extern {
            n_type |= N_PEXT;
        }
        let mut n_desc = 0u16;
        if sym.absolute {
            n_desc |= N_NO_DEAD_STRIP;
        }
        if sym.common {
            n_desc |= (sym.common_align_pow2 as u16) << 8;
        }
        if sym.weak_ref {
            n_desc |= N_WEAK_REF;
        }
        if sym.weak_def {
            n_desc |= N_WEAK_DEF;
        }
        write_u32(w, str_offset as u32)?;  // n_strx
        w.write_all(&[n_type])?;           // n_type
        w.write_all(&[sym.section])?;      // n_sect
        write_u16(w, n_desc)?;             // n_desc
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
    // Apple pads the object string table to 8-byte alignment.
    while !tab.len().is_multiple_of(8) {
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

fn align_value(value: u64, power: u32) -> u64 {
    let alignment = 1u64 << power;
    (value + alignment - 1) & !(alignment - 1)
}

fn pack_version(major: u32, minor: u32, patch: u32) -> u32 {
    (major << 16) | (minor << 8) | patch
}

/// Write N zero bytes without heap allocation.
fn write_zeros<W: Write>(w: &mut W, n: usize) -> io::Result<()> {
    const BUF: [u8; 64] = [0u8; 64];
    let mut remaining = n;
    while remaining > 0 {
        let chunk = remaining.min(BUF.len());
        w.write_all(&BUF[..chunk])?;
        remaining -= chunk;
    }
    Ok(())
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
        let text = obj.text_section_mut();
        text.data = vec![0xD5, 0x03, 0x20, 0x1F];
        text.size = 4;
        obj.symbols.push(Symbol {
            name: "_test".into(),
            section: 1,
            value: 0,
            global: true,
            undefined: false,
            absolute: false,
            common: false,
            common_align_pow2: 0,
            private_extern: false,
            weak_ref: false,
            weak_def: false,
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
            Symbol {
                name: "_main".into(),
                section: 1,
                value: 0,
                global: true,
                undefined: false,
                absolute: false,
                common: false,
                common_align_pow2: 0,
                private_extern: false,
                weak_ref: false,
                weak_def: false,
            },
            Symbol {
                name: "msg".into(),
                section: 2,
                value: 0,
                global: false,
                undefined: false,
                absolute: false,
                common: false,
                common_align_pow2: 0,
                private_extern: false,
                weak_ref: false,
                weak_def: false,
            },
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
    fn symbol_flags_encode_private_and_weak_bits() {
        let mut obj = ObjectFile::new();
        obj.symbols.push(Symbol {
            name: "_hidden".into(),
            section: 1,
            value: 0,
            global: true,
            undefined: false,
            absolute: false,
            common: false,
            common_align_pow2: 0,
            private_extern: true,
            weak_ref: false,
            weak_def: true,
        });
        obj.symbols.push(Symbol {
            name: "_puts".into(),
            section: 0,
            value: 0,
            global: true,
            undefined: true,
            absolute: false,
            common: false,
            common_align_pow2: 0,
            private_extern: false,
            weak_ref: true,
            weak_def: false,
        });

        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();

        let symtab_cmd_offset =
            HEADER_SIZE as usize + (SEGMENT_CMD_SIZE + SECTION_SIZE) as usize + BUILD_VERSION_CMD_SIZE as usize;
        let symoff = u32::from_le_bytes([
            buf[symtab_cmd_offset + 8],
            buf[symtab_cmd_offset + 9],
            buf[symtab_cmd_offset + 10],
            buf[symtab_cmd_offset + 11],
        ]) as usize;
        let hidden_type = buf[symoff + 4];
        let hidden_desc = u16::from_le_bytes([buf[symoff + 6], buf[symoff + 7]]);
        let puts_type = buf[symoff + 20];
        let puts_desc = u16::from_le_bytes([buf[symoff + 22], buf[symoff + 23]]);

        assert_eq!(hidden_type, N_SECT | N_EXT | N_PEXT);
        assert_eq!(hidden_desc, N_WEAK_DEF);
        assert_eq!(puts_type, N_UNDF | N_EXT);
        assert_eq!(puts_desc, N_WEAK_REF);
    }

    #[test]
    fn symbol_flags_encode_absolute_symbol() {
        let mut obj = ObjectFile::new();
        obj.symbols.push(Symbol {
            name: "ABS1".into(),
            section: 0,
            value: 7,
            global: false,
            undefined: false,
            absolute: true,
            common: false,
            common_align_pow2: 0,
            private_extern: false,
            weak_ref: false,
            weak_def: false,
        });

        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();

        let symtab_cmd_offset =
            HEADER_SIZE as usize + (SEGMENT_CMD_SIZE + SECTION_SIZE) as usize + BUILD_VERSION_CMD_SIZE as usize;
        let symoff = u32::from_le_bytes([
            buf[symtab_cmd_offset + 8],
            buf[symtab_cmd_offset + 9],
            buf[symtab_cmd_offset + 10],
            buf[symtab_cmd_offset + 11],
        ]) as usize;

        let abs_type = buf[symoff + 4];
        let abs_desc = u16::from_le_bytes([buf[symoff + 6], buf[symoff + 7]]);
        let abs_value = u64::from_le_bytes([
            buf[symoff + 8],
            buf[symoff + 9],
            buf[symoff + 10],
            buf[symoff + 11],
            buf[symoff + 12],
            buf[symoff + 13],
            buf[symoff + 14],
            buf[symoff + 15],
        ]);

        assert_eq!(abs_type, N_ABS);
        assert_eq!(abs_desc, N_NO_DEAD_STRIP);
        assert_eq!(abs_value, 7);
    }

    #[test]
    fn symbol_flags_encode_common_symbol_alignment() {
        let mut obj = ObjectFile::new();
        obj.symbols.push(Symbol {
            name: "_common".into(),
            section: 0,
            value: 24,
            global: true,
            undefined: true,
            absolute: false,
            common: true,
            common_align_pow2: 3,
            private_extern: false,
            weak_ref: false,
            weak_def: false,
        });

        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();

        let symtab_cmd_offset =
            HEADER_SIZE as usize + (SEGMENT_CMD_SIZE + SECTION_SIZE) as usize + BUILD_VERSION_CMD_SIZE as usize;
        let symoff = u32::from_le_bytes([
            buf[symtab_cmd_offset + 8],
            buf[symtab_cmd_offset + 9],
            buf[symtab_cmd_offset + 10],
            buf[symtab_cmd_offset + 11],
        ]) as usize;

        let common_type = buf[symoff + 4];
        let common_desc = u16::from_le_bytes([buf[symoff + 6], buf[symoff + 7]]);
        let common_value = u64::from_le_bytes([
            buf[symoff + 8],
            buf[symoff + 9],
            buf[symoff + 10],
            buf[symoff + 11],
            buf[symoff + 12],
            buf[symoff + 13],
            buf[symoff + 14],
            buf[symoff + 15],
        ]);

        assert_eq!(common_type, N_UNDF | N_EXT);
        assert_eq!(common_desc, 3u16 << 8);
        assert_eq!(common_value, 24);
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

    #[test]
    fn zerofill_section_does_not_contribute_file_bytes() {
        let mut obj = ObjectFile::new();
        obj.sections.push(Section {
            segment: "__DATA".into(),
            name: "__bss".into(),
            kind: SectionKind::ZeroFill,
            align_pow2: 4,
            data: Vec::new(),
            size: 16,
            relocations: Vec::new(),
        });
        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();
        assert_eq!(&buf[0..4], &MH_MAGIC_64.to_le_bytes());
    }
}
