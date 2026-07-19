//! Mach-O 64-bit object file writer for ARM64 macOS.
//!
//! Writes relocatable object files (.o) that can be linked with Apple's `ld`.
//! Implements the minimum viable subset: header, segment with supported Mach-O
//! sections, symbol table, dynamic symbol table, build version, and relocations.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::process::Command;
use std::sync::OnceLock;

// ---- Mach-O Constants ----

const MH_MAGIC_64: u32 = 0xFEEDFACF;
const CPU_TYPE_ARM64: u32 = 0x0100000C;
const CPU_SUBTYPE_ARM64_ALL: u32 = 0x00000000;
const MH_OBJECT: u32 = 1;
pub const MH_SUBSECTIONS_VIA_SYMBOLS: u32 = 0x2000;

const LC_SEGMENT_64: u32 = 0x19;
const LC_SYMTAB: u32 = 0x02;
const LC_DYSYMTAB: u32 = 0x0B;
const LC_BUILD_VERSION: u32 = 0x32;
const LC_LINKER_OPTIMIZATION_HINT: u32 = 0x2E;

const S_REGULAR: u32 = 0x0;
const S_ZEROFILL: u32 = 0x1;
const S_CSTRING_LITERALS: u32 = 0x2;
const S_16BYTE_LITERALS: u32 = 0xE;
const S_COALESCED: u32 = 0x0B;
const S_THREAD_LOCAL_REGULAR: u32 = 0x11;
const S_THREAD_LOCAL_ZEROFILL: u32 = 0x12;
const S_THREAD_LOCAL_VARIABLES: u32 = 0x13;
const S_ATTR_DEBUG: u32 = 0x02000000;
const S_ATTR_LIVE_SUPPORT: u32 = 0x08000000;
const S_ATTR_STRIP_STATIC_SYMS: u32 = 0x20000000;
const S_ATTR_NO_TOC: u32 = 0x40000000;
const S_ATTR_PURE_INSTRUCTIONS: u32 = 0x80000000;
const S_ATTR_SOME_INSTRUCTIONS: u32 = 0x00000400;

pub const PLATFORM_MACOS: u32 = 1;

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
pub const ARM64_RELOC_GOT_LOAD_PAGE21: u32 = 5;
pub const ARM64_RELOC_GOT_LOAD_PAGEOFF12: u32 = 6;
pub const ARM64_RELOC_POINTER_TO_GOT: u32 = 7;
pub const ARM64_RELOC_TLVP_LOAD_PAGE21: u32 = 8;
pub const ARM64_RELOC_TLVP_LOAD_PAGEOFF12: u32 = 9;
pub const ARM64_RELOC_ADDEND: u32 = 10;

// Struct sizes
const HEADER_SIZE: u32 = 32;
const SEGMENT_CMD_SIZE: u32 = 72;
const SECTION_SIZE: u32 = 80;
const SYMTAB_CMD_SIZE: u32 = 24;
const DYSYMTAB_CMD_SIZE: u32 = 80;
const BUILD_VERSION_CMD_SIZE: u32 = 24;
const LINKEDIT_DATA_CMD_SIZE: u32 = 16;
const NLIST_SIZE: u32 = 16;
const RELOC_SIZE: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildVersion {
    pub platform: u32,
    pub minos: u32,
    pub sdk: u32,
}

impl Default for BuildVersion {
    fn default() -> Self {
        Self {
            platform: PLATFORM_MACOS,
            minos: default_host_minos(),
            sdk: 0,
        }
    }
}

fn default_host_minos() -> u32 {
    static HOST_MINOS: OnceLock<u32> = OnceLock::new();
    *HOST_MINOS.get_or_init(|| {
        Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|out| out.status.success().then_some(out.stdout))
            .and_then(|stdout| {
                let version = String::from_utf8(stdout).ok()?;
                version
                    .trim()
                    .split('.')
                    .next()
                    .and_then(|major| major.parse::<u32>().ok())
            })
            .map(|major| pack_version(major, 0, 0))
            .unwrap_or_else(|| pack_version(15, 0, 0))
    })
}

/// A symbol in the object file.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub section: u8,     // 1-based section index, or 0 for N_UNDF
    pub value: u64,      // offset within section
    pub global: bool,    // N_EXT flag
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
    pub offset: u32,     // byte offset in section
    pub symbol_idx: u32, // index into symbol table
    pub pcrel: bool,     // PC-relative?
    pub length: u8,      // 2 = 4 bytes (32-bit)
    pub extern_: bool,   // true = symbol index, false = section number
    pub reloc_type: u32, // ARM64_RELOC_*
}

/// Supported Mach-O section kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionKind {
    Text,
    Data,
    CStringLiterals,
    Literal16,
    ConstData,
    ThreadLocalData,
    ThreadLocalZeroFill,
    ThreadLocalVariables,
    CompactUnwind,
    EhFrame,
    ZeroFill,
}

impl SectionKind {
    fn flags(&self, size: u64, has_instructions: bool) -> u32 {
        match self {
            Self::Text if has_instructions => {
                S_REGULAR | S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS
            }
            Self::Text if size == 0 => S_ATTR_PURE_INSTRUCTIONS,
            Self::Text => S_ATTR_PURE_INSTRUCTIONS,
            Self::CStringLiterals => S_CSTRING_LITERALS,
            Self::Literal16 => S_16BYTE_LITERALS,
            Self::CompactUnwind => S_REGULAR | S_ATTR_DEBUG,
            Self::EhFrame => {
                S_COALESCED | S_ATTR_NO_TOC | S_ATTR_STRIP_STATIC_SYMS | S_ATTR_LIVE_SUPPORT
            }
            Self::ZeroFill => S_ZEROFILL,
            Self::ThreadLocalData => S_THREAD_LOCAL_REGULAR,
            Self::ThreadLocalZeroFill => S_THREAD_LOCAL_ZEROFILL,
            Self::ThreadLocalVariables => S_THREAD_LOCAL_VARIABLES,
            Self::Data | Self::ConstData => S_REGULAR,
        }
    }

    pub(crate) fn is_zerofill(&self) -> bool {
        matches!(self, Self::ZeroFill | Self::ThreadLocalZeroFill)
    }
}

/// A Mach-O section in a relocatable object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub segment: String,
    pub name: String,
    pub kind: SectionKind,
    pub align_pow2: u32,
    pub has_instructions: bool,
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
            has_instructions: false,
            data: Vec::new(),
            size: 0,
            relocations: Vec::new(),
        }
    }

    pub fn text() -> Self {
        Self::new("__TEXT", "__text", SectionKind::Text)
    }

    pub fn file_size(&self) -> u64 {
        if self.kind.is_zerofill() {
            0
        } else {
            self.size
        }
    }
}

/// Assembled object file ready for Mach-O emission.
#[derive(Debug, Clone)]
pub struct ObjectFile {
    pub sections: Vec<Section>,
    pub symbols: Vec<Symbol>,
    pub flags: u32,
    pub build_version: BuildVersion,
    pub linker_optimization_hints: Vec<u8>,
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
            flags: 0,
            build_version: BuildVersion::default(),
            linker_optimization_hints: Vec::new(),
        }
    }

    pub fn section(&self, segment: &str, name: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|section| section.segment == segment && section.name == name)
    }

    pub fn section_mut(&mut self, segment: &str, name: &str) -> Option<&mut Section> {
        self.sections
            .iter_mut()
            .find(|section| section.segment == segment && section.name == name)
    }

    pub fn text_section(&self) -> &Section {
        self.section("__TEXT", "__text")
            .expect("missing __TEXT,__text section")
    }

    pub fn text_section_mut(&mut self) -> &mut Section {
        self.section_mut("__TEXT", "__text")
            .expect("missing __TEXT,__text section")
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
    let has_loh = !obj.linker_optimization_hints.is_empty();
    let ncmds: u32 = 4 + has_loh as u32; // LC_SEGMENT_64, LC_BUILD_VERSION, optional LOH, LC_SYMTAB, LC_DYSYMTAB
    let sizeofcmds = segment_cmdsize
        + BUILD_VERSION_CMD_SIZE
        + if has_loh { LINKEDIT_DATA_CMD_SIZE } else { 0 }
        + SYMTAB_CMD_SIZE
        + DYSYMTAB_CMD_SIZE;

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

        vm_cursor = checked_align_value(vm_cursor, section.align_pow2).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "section layout alignment overflows u64",
            )
        })?;
        let addr = vm_cursor;
        vm_cursor = vm_cursor.checked_add(section.size).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "section layout size overflows u64",
            )
        })?;

        let offset = if section.kind.is_zerofill() {
            0
        } else {
            let offset = content_offset.saturating_add(addr as u32);
            file_cursor = file_cursor.max(offset.saturating_add(section.file_size() as u32));
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

    // Linker optimization hints, when present, follow relocations.
    let lohoff = reloc_cursor;
    let lohsize = obj.linker_optimization_hints.len() as u32;

    // Symbol table follows linker optimization hints.
    let symoff = lohoff + lohsize;
    let nsyms = obj.symbols.len() as u32;
    let sym_size = nsyms * NLIST_SIZE;

    // String table follows symbol table.
    let stroff = symoff + sym_size;
    let strtab = build_string_table(&obj.symbols);
    let strsize = strtab.bytes.len() as u32;

    // Classify symbols for LC_DYSYMTAB.
    let nlocalsym = obj
        .symbols
        .iter()
        .filter(|s| !s.global && !s.undefined)
        .count() as u32;
    let nextdefsym = obj
        .symbols
        .iter()
        .filter(|s| s.global && !s.undefined)
        .count() as u32;
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
    let vmsize = vm_cursor;

    // ---- Write header ----
    write_u32(w, MH_MAGIC_64)?;
    write_u32(w, CPU_TYPE_ARM64)?;
    write_u32(w, CPU_SUBTYPE_ARM64_ALL)?;
    write_u32(w, MH_OBJECT)?;
    write_u32(w, ncmds)?;
    write_u32(w, sizeofcmds)?;
    write_u32(w, obj.flags)?; // flags
    write_u32(w, 0)?; // reserved

    // ---- LC_SEGMENT_64 ----
    write_u32(w, LC_SEGMENT_64)?;
    write_u32(w, segment_cmdsize)?;
    write_pad16(w, b"")?; // segname (empty for object files)
    write_u64(w, 0)?; // vmaddr
    write_u64(w, vmsize)?; // vmsize
    write_u64(w, segment_fileoff as u64)?; // fileoff
    write_u64(w, filesize as u64)?; // filesize
    write_u32(w, 7)?; // maxprot (rwx)
    write_u32(w, 7)?; // initprot (rwx)
    write_u32(w, nsects)?;
    write_u32(w, 0)?; // flags

    for (section, layout) in obj.sections.iter().zip(&layouts) {
        write_pad16(w, section.name.as_bytes())?;
        write_pad16(w, section.segment.as_bytes())?;
        write_u64(w, layout.addr)?;
        write_u64(w, section.size)?;
        write_u32(w, layout.offset)?;
        write_u32(w, section.align_pow2)?;
        write_u32(w, layout.reloff)?;
        write_u32(w, layout.nreloc)?;
        write_u32(
            w,
            section.kind.flags(section.size, section.has_instructions),
        )?;
        write_u32(w, 0)?;
        write_u32(w, 0)?;
        write_u32(w, 0)?;
    }

    // ---- LC_BUILD_VERSION ----
    write_u32(w, LC_BUILD_VERSION)?;
    write_u32(w, BUILD_VERSION_CMD_SIZE)?;
    write_u32(w, obj.build_version.platform)?;
    write_u32(w, obj.build_version.minos)?;
    write_u32(w, obj.build_version.sdk)?;
    write_u32(w, 0)?; // ntools

    if has_loh {
        write_u32(w, LC_LINKER_OPTIMIZATION_HINT)?;
        write_u32(w, LINKEDIT_DATA_CMD_SIZE)?;
        write_u32(w, lohoff)?;
        write_u32(w, lohsize)?;
    }

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
    write_u32(w, 0)?; // ilocalsym
    write_u32(w, nlocalsym)?;
    write_u32(w, nlocalsym)?; // iextdefsym
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
        sorted_relocs.sort_by_key(|rel| std::cmp::Reverse(rel.offset));
        for rel in &sorted_relocs {
            write_reloc(w, rel)?;
        }
    }

    if has_loh {
        w.write_all(&obj.linker_optimization_hints)?;
    }

    // ---- Symbol table ----
    for (i, sym) in obj.symbols.iter().enumerate() {
        let str_offset = *strtab
            .offsets
            .get(&sym.name)
            .expect("string table offset for symbol");
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
        write_u32(w, str_offset)?; // n_strx
        w.write_all(&[n_type])?; // n_type
        w.write_all(&[sym.section])?; // n_sect
        write_u16(w, n_desc)?; // n_desc
        write_u64(w, sym.value)?; // n_value
        let _ = i;
    }

    // ---- String table ----
    w.write_all(&strtab.bytes)?;

    Ok(())
}

// ---- Helpers ----

struct StringTable {
    bytes: Vec<u8>,
    offsets: BTreeMap<String, u32>,
}

fn build_string_table(symbols: &[Symbol]) -> StringTable {
    let mut names: Vec<&str> = symbols.iter().map(|sym| sym.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();

    // Match clang's integrated assembler: names are finalized in descending
    // reverse-lexicographic order so suffix-related symbols can share bytes.
    names.sort_by(|left, right| right.bytes().rev().cmp(left.bytes().rev()));

    let mut bytes = vec![0u8];
    let mut offsets = BTreeMap::new();
    for name in names {
        let offset = suffix_string_offset(&bytes, name).unwrap_or_else(|| {
            let offset = bytes.len() as u32;
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(0);
            offset
        });
        offsets.insert(name.to_string(), offset);
    }

    // Apple pads the object string table to 8-byte alignment.
    while !bytes.len().is_multiple_of(8) {
        bytes.push(0);
    }

    StringTable { bytes, offsets }
}

fn suffix_string_offset(strtab: &[u8], name: &str) -> Option<u32> {
    let name_bytes = name.as_bytes();
    if strtab.len() < name_bytes.len() + 1 {
        return None;
    }

    for pos in 1..=strtab.len() - name_bytes.len() - 1 {
        if &strtab[pos..pos + name_bytes.len()] == name_bytes && strtab[pos + name_bytes.len()] == 0
        {
            return Some(pos as u32);
        }
    }

    None
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
    if align <= 1 {
        return value;
    }
    (value + align - 1) & !(align - 1)
}

fn checked_align_value(value: u64, power: u32) -> Option<u64> {
    let alignment = 1u64.checked_shl(power)?;
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
}

pub fn pack_version(major: u32, minor: u32, patch: u32) -> u32 {
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

        assert_eq!(strtab.bytes[0], 0); // initial null
        assert_eq!(strtab.offsets["_main"], 1);
        assert_eq!(strtab.offsets["msg"], 7);
    }

    #[test]
    fn string_table_orders_names_by_reverse_lexicographic_suffix_order() {
        let syms = vec![
            Symbol {
                name: "_ext".into(),
                section: 0,
                value: 0,
                global: true,
                undefined: true,
                absolute: false,
                common: false,
                common_align_pow2: 0,
                private_extern: false,
                weak_ref: false,
                weak_def: false,
            },
            Symbol {
                name: "ccc".into(),
                section: 1,
                value: 4,
                global: false,
                undefined: false,
                absolute: false,
                common: false,
                common_align_pow2: 0,
                private_extern: false,
                weak_ref: false,
                weak_def: false,
            },
            Symbol {
                name: "_bbb".into(),
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
                name: "_aaa".into(),
                section: 1,
                value: 8,
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
                name: "ltmp0".into(),
                section: 1,
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

        assert_eq!(
            strtab.bytes,
            b"\0_ext\0ccc\0_bbb\0_aaa\0ltmp0\0\0\0\0\0\0\0"
        );
        assert_eq!(strtab.offsets["_ext"], 1);
        assert_eq!(strtab.offsets["ccc"], 6);
        assert_eq!(strtab.offsets["_bbb"], 10);
        assert_eq!(strtab.offsets["_aaa"], 15);
        assert_eq!(strtab.offsets["ltmp0"], 20);
    }

    #[test]
    fn string_table_reuses_suffix_bytes() {
        let syms = vec![
            Symbol {
                name: "_aaa".into(),
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
                name: "aaa".into(),
                section: 1,
                value: 4,
                global: false,
                undefined: false,
                absolute: false,
                common: false,
                common_align_pow2: 0,
                private_extern: false,
                weak_ref: false,
                weak_def: false,
            },
            Symbol {
                name: "ltmp0".into(),
                section: 1,
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

        assert_eq!(strtab.bytes, b"\0_aaa\0ltmp0\0\0\0\0\0");
        assert_eq!(strtab.offsets["_aaa"], 1);
        assert_eq!(strtab.offsets["aaa"], 2);
        assert_eq!(strtab.offsets["ltmp0"], 6);
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
        assert_eq!(info & 0x00FFFFFF, 1); // symbolnum
        assert_eq!((info >> 24) & 1, 1); // pcrel
        assert_eq!((info >> 25) & 3, 2); // length
        assert_eq!((info >> 27) & 1, 1); // extern
        assert_eq!((info >> 28) & 0xF, 3); // type = PAGE21
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

        let symtab_cmd_offset = HEADER_SIZE as usize
            + (SEGMENT_CMD_SIZE + SECTION_SIZE) as usize
            + BUILD_VERSION_CMD_SIZE as usize;
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

        let symtab_cmd_offset = HEADER_SIZE as usize
            + (SEGMENT_CMD_SIZE + SECTION_SIZE) as usize
            + BUILD_VERSION_CMD_SIZE as usize;
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

        let symtab_cmd_offset = HEADER_SIZE as usize
            + (SEGMENT_CMD_SIZE + SECTION_SIZE) as usize
            + BUILD_VERSION_CMD_SIZE as usize;
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
    fn dysymtab_command_counts_symbol_classes() {
        let mut obj = ObjectFile::new();
        obj.symbols.push(Symbol {
            name: "local_abs".into(),
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
        obj.symbols.push(Symbol {
            name: "local_text".into(),
            section: 1,
            value: 0,
            global: false,
            undefined: false,
            absolute: false,
            common: false,
            common_align_pow2: 0,
            private_extern: false,
            weak_ref: false,
            weak_def: false,
        });
        obj.symbols.push(Symbol {
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
        });
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

        let dysymtab_cmd_offset = HEADER_SIZE as usize
            + (SEGMENT_CMD_SIZE + SECTION_SIZE) as usize
            + BUILD_VERSION_CMD_SIZE as usize
            + SYMTAB_CMD_SIZE as usize;
        let ilocalsym = u32::from_le_bytes([
            buf[dysymtab_cmd_offset + 8],
            buf[dysymtab_cmd_offset + 9],
            buf[dysymtab_cmd_offset + 10],
            buf[dysymtab_cmd_offset + 11],
        ]);
        let nlocalsym = u32::from_le_bytes([
            buf[dysymtab_cmd_offset + 12],
            buf[dysymtab_cmd_offset + 13],
            buf[dysymtab_cmd_offset + 14],
            buf[dysymtab_cmd_offset + 15],
        ]);
        let iextdefsym = u32::from_le_bytes([
            buf[dysymtab_cmd_offset + 16],
            buf[dysymtab_cmd_offset + 17],
            buf[dysymtab_cmd_offset + 18],
            buf[dysymtab_cmd_offset + 19],
        ]);
        let nextdefsym = u32::from_le_bytes([
            buf[dysymtab_cmd_offset + 20],
            buf[dysymtab_cmd_offset + 21],
            buf[dysymtab_cmd_offset + 22],
            buf[dysymtab_cmd_offset + 23],
        ]);
        let iundefsym = u32::from_le_bytes([
            buf[dysymtab_cmd_offset + 24],
            buf[dysymtab_cmd_offset + 25],
            buf[dysymtab_cmd_offset + 26],
            buf[dysymtab_cmd_offset + 27],
        ]);
        let nundefsym = u32::from_le_bytes([
            buf[dysymtab_cmd_offset + 28],
            buf[dysymtab_cmd_offset + 29],
            buf[dysymtab_cmd_offset + 30],
            buf[dysymtab_cmd_offset + 31],
        ]);

        assert_eq!(ilocalsym, 0);
        assert_eq!(nlocalsym, 2);
        assert_eq!(iextdefsym, 2);
        assert_eq!(nextdefsym, 1);
        assert_eq!(iundefsym, 3);
        assert_eq!(nundefsym, 2);
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
    fn mach_header_uses_object_flags() {
        let mut obj = ObjectFile::new();
        obj.flags = MH_SUBSECTIONS_VIA_SYMBOLS;

        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();

        let flags = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]);
        assert_eq!(flags, MH_SUBSECTIONS_VIA_SYMBOLS);
    }

    #[test]
    fn build_version_command_uses_object_metadata() {
        let mut obj = ObjectFile::new();
        obj.build_version = BuildVersion {
            platform: PLATFORM_MACOS,
            minos: pack_version(11, 0, 0),
            sdk: pack_version(15, 5, 0),
        };

        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();

        let build_cmd_offset = HEADER_SIZE as usize + (SEGMENT_CMD_SIZE + SECTION_SIZE) as usize;
        let platform = u32::from_le_bytes([
            buf[build_cmd_offset + 8],
            buf[build_cmd_offset + 9],
            buf[build_cmd_offset + 10],
            buf[build_cmd_offset + 11],
        ]);
        let minos = u32::from_le_bytes([
            buf[build_cmd_offset + 12],
            buf[build_cmd_offset + 13],
            buf[build_cmd_offset + 14],
            buf[build_cmd_offset + 15],
        ]);
        let sdk = u32::from_le_bytes([
            buf[build_cmd_offset + 16],
            buf[build_cmd_offset + 17],
            buf[build_cmd_offset + 18],
            buf[build_cmd_offset + 19],
        ]);

        assert_eq!(platform, PLATFORM_MACOS);
        assert_eq!(minos, pack_version(11, 0, 0));
        assert_eq!(sdk, pack_version(15, 5, 0));
    }

    #[test]
    fn linker_optimization_hint_command_uses_object_metadata() {
        let mut obj = ObjectFile::new();
        obj.linker_optimization_hints = vec![7, 2, 0, 4, 0, 0, 0, 0];

        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();

        let ncmds = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]) as usize;
        let mut offset = HEADER_SIZE as usize;
        let mut found = None;
        for _ in 0..ncmds {
            let cmd = u32::from_le_bytes([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);
            let cmdsize = u32::from_le_bytes([
                buf[offset + 4],
                buf[offset + 5],
                buf[offset + 6],
                buf[offset + 7],
            ]) as usize;
            if cmd == LC_LINKER_OPTIMIZATION_HINT {
                let dataoff = u32::from_le_bytes([
                    buf[offset + 8],
                    buf[offset + 9],
                    buf[offset + 10],
                    buf[offset + 11],
                ]) as usize;
                let datasize = u32::from_le_bytes([
                    buf[offset + 12],
                    buf[offset + 13],
                    buf[offset + 14],
                    buf[offset + 15],
                ]) as usize;
                found = Some((dataoff, datasize));
                break;
            }
            offset += cmdsize;
        }

        let (dataoff, datasize) = found.expect("missing LC_LINKER_OPTIMIZATION_HINT");
        assert_eq!(datasize, 8);
        assert_eq!(&buf[dataoff..dataoff + datasize], &[7, 2, 0, 4, 0, 0, 0, 0]);
    }

    #[test]
    fn zerofill_section_does_not_contribute_file_bytes() {
        let mut obj = ObjectFile::new();
        obj.sections.push(Section {
            segment: "__DATA".into(),
            name: "__bss".into(),
            kind: SectionKind::ZeroFill,
            align_pow2: 4,
            has_instructions: false,
            data: Vec::new(),
            size: 16,
            relocations: Vec::new(),
        });
        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();
        assert_eq!(&buf[0..4], &MH_MAGIC_64.to_le_bytes());
    }

    #[test]
    fn zerofill_layout_reports_address_overflow() {
        let mut obj = ObjectFile::new();
        let mut bss = Section::new("__DATA", "__bss", SectionKind::ZeroFill);
        bss.size = u64::MAX - 1;
        obj.sections.push(bss);
        let mut thread_bss =
            Section::new("__DATA", "__thread_bss", SectionKind::ThreadLocalZeroFill);
        thread_bss.align_pow2 = 2;
        obj.sections.push(thread_bss);

        let error = write_macho(&obj, &mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "section layout alignment overflows u64");
    }

    #[test]
    fn thread_local_sections_use_thread_local_flags() {
        assert_eq!(
            SectionKind::ThreadLocalData.flags(4, false),
            S_THREAD_LOCAL_REGULAR
        );
        assert_eq!(
            SectionKind::ThreadLocalZeroFill.flags(4, false),
            S_THREAD_LOCAL_ZEROFILL
        );
        assert_eq!(
            SectionKind::ThreadLocalVariables.flags(24, false),
            S_THREAD_LOCAL_VARIABLES
        );
    }

    #[test]
    fn literal16_section_uses_literal_flags() {
        assert_eq!(SectionKind::Literal16.flags(16, false), S_16BYTE_LITERALS);
    }

    #[test]
    fn file_backed_section_offsets_preserve_vm_gaps() {
        let mut obj = ObjectFile::new();
        {
            let text = obj.text_section_mut();
            text.data = vec![0; 0x24];
            text.size = 0x24;
            text.has_instructions = true;
        }
        let mut literal = Section::new("__TEXT", "__literal16", SectionKind::Literal16);
        literal.align_pow2 = 4;
        literal.data = vec![0; 0x20];
        literal.size = 0x20;
        obj.sections.push(literal);

        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();

        let first_section = HEADER_SIZE as usize + SEGMENT_CMD_SIZE as usize;
        let second_section = first_section + SECTION_SIZE as usize;
        let text_offset = u32::from_le_bytes(
            buf[first_section + 48..first_section + 52]
                .try_into()
                .unwrap(),
        );
        let literal_offset = u32::from_le_bytes(
            buf[second_section + 48..second_section + 52]
                .try_into()
                .unwrap(),
        );
        assert_eq!(literal_offset - text_offset, 0x30);
    }
}
