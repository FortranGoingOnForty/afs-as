//! ELF64 relocatable object model, writer, and reader (x13).
//!
//! Parallel to `macho.rs` by design, sharing nothing: the Mach-O model
//! bakes segment names, `S_*` flags, and scattered-bit relocations into
//! every field, and a shared trait would be lossy on both sides. Per the
//! afs-ld house rules adopted for this module: constants are duplicated
//! locally (numerically mirroring the system headers), every wire
//! structure has a paired `parse`/`write`, and the writer is a pure
//! function of the model — no clock, no host queries.
//!
//! Scope (x13): `ET_REL` objects only. The relocation namespace is
//! per-arch (`reloc::x86_64` now; `reloc::aarch64` reserved for x15);
//! `r_type` stays a raw `u32` and validation dispatches on
//! `ObjectFile.machine`.

use std::collections::HashMap;
use std::fmt;
use std::io::Write;

// ---------------------------------------------------------------------
// Constants (duplicated locally per afs-ld discipline)
// ---------------------------------------------------------------------

pub const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
pub const ELFCLASS64: u8 = 2;
pub const ELFDATA2LSB: u8 = 1;
pub const EV_CURRENT: u8 = 1;

pub const ELFOSABI_NONE: u8 = 0;
pub const ELFOSABI_FREEBSD: u8 = 9;

pub const ET_REL: u16 = 1;

pub const EM_X86_64: u16 = 62;
/// Reserved for x15 (arm64-linux); no `R_AARCH64_*` constants until then.
pub const EM_AARCH64: u16 = 183;

pub const SHT_NULL: u32 = 0;
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_NOBITS: u32 = 8;

pub const SHF_WRITE: u64 = 0x1;
pub const SHF_ALLOC: u64 = 0x2;
pub const SHF_EXECINSTR: u64 = 0x4;

pub const SHN_UNDEF: u16 = 0;
pub const SHN_ABS: u16 = 0xfff1;
pub const SHN_COMMON: u16 = 0xfff2;

pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;

pub const STT_NOTYPE: u8 = 0;
pub const STT_OBJECT: u8 = 1;
pub const STT_FUNC: u8 = 2;
pub const STT_SECTION: u8 = 3;

pub const STV_DEFAULT: u8 = 0;
pub const STV_HIDDEN: u8 = 2;

/// x86_64 relocation types (binutils include/elf/x86-64.h).
pub mod reloc {
    pub mod x86_64 {
        pub const R_X86_64_NONE: u32 = 0;
        pub const R_X86_64_64: u32 = 1;
        pub const R_X86_64_PC32: u32 = 2;
        pub const R_X86_64_PLT32: u32 = 4;
        pub const R_X86_64_GOTPCREL: u32 = 9;
        pub const R_X86_64_32: u32 = 10;
        pub const R_X86_64_32S: u32 = 11;
        pub const R_X86_64_GOTPCRELX: u32 = 41;
        pub const R_X86_64_REX_GOTPCRELX: u32 = 42;

        /// Field width in bytes patched by a relocation type. Used by
        /// offset-in-bounds validation.
        pub fn width(r_type: u32) -> Option<u64> {
            match r_type {
                R_X86_64_NONE => Some(0),
                R_X86_64_64 => Some(8),
                R_X86_64_PC32
                | R_X86_64_PLT32
                | R_X86_64_GOTPCREL
                | R_X86_64_32
                | R_X86_64_32S
                | R_X86_64_GOTPCRELX
                | R_X86_64_REX_GOTPCRELX => Some(4),
                _ => None,
            }
        }
    }
}

// ---------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------

/// Error for ELF parse/write/validate, in the AsmError style: an
/// offset (when parsing) plus a reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfError {
    pub offset: Option<u64>,
    pub message: String,
}

impl ElfError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            offset: None,
            message: message.into(),
        }
    }

    fn at(offset: u64, message: impl Into<String>) -> Self {
        Self {
            offset: Some(offset),
            message: message.into(),
        }
    }
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.offset {
            Some(off) => write!(f, "elf: offset {:#x}: {}", off, self.message),
            None => write!(f, "elf: {}", self.message),
        }
    }
}

impl std::error::Error for ElfError {}

// ---------------------------------------------------------------------
// Wire structures — each with paired parse/write
// ---------------------------------------------------------------------

/// Little-endian field readers. All ELF64 we emit or accept is 2LSB.
fn rd_u16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}
fn rd_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
fn rd_u64(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes([
        b[off],
        b[off + 1],
        b[off + 2],
        b[off + 3],
        b[off + 4],
        b[off + 5],
        b[off + 6],
        b[off + 7],
    ])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elf64Ehdr {
    pub osabi: u8,
    pub abiversion: u8,
    pub e_type: u16,
    pub e_machine: u16,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

pub const EHDR_SIZE: usize = 64;
pub const SHDR_SIZE: usize = 64;
pub const SYM_SIZE: usize = 24;
pub const RELA_SIZE: usize = 24;

impl Elf64Ehdr {
    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&ELFMAG);
        out.push(ELFCLASS64);
        out.push(ELFDATA2LSB);
        out.push(EV_CURRENT);
        out.push(self.osabi);
        out.push(self.abiversion);
        out.extend_from_slice(&[0u8; 7]); // e_ident padding
        out.extend_from_slice(&self.e_type.to_le_bytes());
        out.extend_from_slice(&self.e_machine.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // e_version
        out.extend_from_slice(&self.e_entry.to_le_bytes());
        out.extend_from_slice(&self.e_phoff.to_le_bytes());
        out.extend_from_slice(&self.e_shoff.to_le_bytes());
        out.extend_from_slice(&self.e_flags.to_le_bytes());
        out.extend_from_slice(&(EHDR_SIZE as u16).to_le_bytes());
        out.extend_from_slice(&self.e_phentsize.to_le_bytes());
        out.extend_from_slice(&self.e_phnum.to_le_bytes());
        out.extend_from_slice(&(SHDR_SIZE as u16).to_le_bytes());
        out.extend_from_slice(&self.e_shnum.to_le_bytes());
        out.extend_from_slice(&self.e_shstrndx.to_le_bytes());
    }

    pub fn parse(b: &[u8]) -> Result<Self, ElfError> {
        if b.len() < EHDR_SIZE {
            return Err(ElfError::at(0, "file shorter than ELF header"));
        }
        if b[0..4] != ELFMAG {
            return Err(ElfError::at(0, "bad ELF magic"));
        }
        if b[4] != ELFCLASS64 {
            return Err(ElfError::at(4, format!("not ELFCLASS64 (got {})", b[4])));
        }
        if b[5] != ELFDATA2LSB {
            return Err(ElfError::at(5, format!("not little-endian (got {})", b[5])));
        }
        if b[6] != EV_CURRENT {
            return Err(ElfError::at(6, format!("bad EI_VERSION {}", b[6])));
        }
        Ok(Self {
            osabi: b[7],
            abiversion: b[8],
            e_type: rd_u16(b, 16),
            e_machine: rd_u16(b, 18),
            e_entry: rd_u64(b, 24),
            e_phoff: rd_u64(b, 32),
            e_shoff: rd_u64(b, 40),
            e_flags: rd_u32(b, 48),
            e_phentsize: rd_u16(b, 54),
            e_phnum: rd_u16(b, 56),
            e_shentsize: rd_u16(b, 58),
            e_shnum: rd_u16(b, 60),
            e_shstrndx: rd_u16(b, 62),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elf64Shdr {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u64,
    pub sh_entsize: u64,
}

impl Elf64Shdr {
    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.sh_name.to_le_bytes());
        out.extend_from_slice(&self.sh_type.to_le_bytes());
        out.extend_from_slice(&self.sh_flags.to_le_bytes());
        out.extend_from_slice(&self.sh_addr.to_le_bytes());
        out.extend_from_slice(&self.sh_offset.to_le_bytes());
        out.extend_from_slice(&self.sh_size.to_le_bytes());
        out.extend_from_slice(&self.sh_link.to_le_bytes());
        out.extend_from_slice(&self.sh_info.to_le_bytes());
        out.extend_from_slice(&self.sh_addralign.to_le_bytes());
        out.extend_from_slice(&self.sh_entsize.to_le_bytes());
    }

    pub fn parse(b: &[u8], off: usize) -> Result<Self, ElfError> {
        if b.len() < off + SHDR_SIZE {
            return Err(ElfError::at(off as u64, "section header out of bounds"));
        }
        Ok(Self {
            sh_name: rd_u32(b, off),
            sh_type: rd_u32(b, off + 4),
            sh_flags: rd_u64(b, off + 8),
            sh_addr: rd_u64(b, off + 16),
            sh_offset: rd_u64(b, off + 24),
            sh_size: rd_u64(b, off + 32),
            sh_link: rd_u32(b, off + 40),
            sh_info: rd_u32(b, off + 44),
            sh_addralign: rd_u64(b, off + 48),
            sh_entsize: rd_u64(b, off + 56),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elf64Sym {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

impl Elf64Sym {
    pub fn bind(&self) -> u8 {
        self.st_info >> 4
    }
    pub fn typ(&self) -> u8 {
        self.st_info & 0xf
    }
    pub fn info(bind: u8, typ: u8) -> u8 {
        (bind << 4) | (typ & 0xf)
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.st_name.to_le_bytes());
        out.push(self.st_info);
        out.push(self.st_other);
        out.extend_from_slice(&self.st_shndx.to_le_bytes());
        out.extend_from_slice(&self.st_value.to_le_bytes());
        out.extend_from_slice(&self.st_size.to_le_bytes());
    }

    pub fn parse(b: &[u8], off: usize) -> Result<Self, ElfError> {
        if b.len() < off + SYM_SIZE {
            return Err(ElfError::at(off as u64, "symbol entry out of bounds"));
        }
        Ok(Self {
            st_name: rd_u32(b, off),
            st_info: b[off + 4],
            st_other: b[off + 5],
            st_shndx: rd_u16(b, off + 6),
            st_value: rd_u64(b, off + 8),
            st_size: rd_u64(b, off + 16),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}

impl Elf64Rela {
    pub fn sym(&self) -> u32 {
        (self.r_info >> 32) as u32
    }
    pub fn r_type(&self) -> u32 {
        self.r_info as u32
    }
    pub fn info(sym: u32, r_type: u32) -> u64 {
        ((sym as u64) << 32) | (r_type as u64)
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.r_offset.to_le_bytes());
        out.extend_from_slice(&self.r_info.to_le_bytes());
        out.extend_from_slice(&self.r_addend.to_le_bytes());
    }

    pub fn parse(b: &[u8], off: usize) -> Result<Self, ElfError> {
        if b.len() < off + RELA_SIZE {
            return Err(ElfError::at(off as u64, "rela entry out of bounds"));
        }
        Ok(Self {
            r_offset: rd_u64(b, off),
            r_info: rd_u64(b, off + 8),
            r_addend: rd_u64(b, off + 16) as i64,
        })
    }
}

/// A string table under construction: dedupes exact repeats, always
/// starts with the mandatory NUL.
#[derive(Debug, Default)]
pub struct StringTable {
    data: Vec<u8>,
    index: HashMap<String, u32>,
}

impl StringTable {
    pub fn new() -> Self {
        Self {
            data: vec![0],
            index: HashMap::new(),
        }
    }

    pub fn intern(&mut self, s: &str) -> u32 {
        if s.is_empty() {
            return 0;
        }
        if let Some(&off) = self.index.get(s) {
            return off;
        }
        let off = self.data.len() as u32;
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0);
        self.index.insert(s.to_string(), off);
        off
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Read a NUL-terminated string at `off` from raw strtab bytes.
    pub fn read(bytes: &[u8], off: u32) -> Result<String, ElfError> {
        let start = off as usize;
        if start >= bytes.len() {
            return Err(ElfError::at(off as u64, "string offset out of bounds"));
        }
        let end = bytes[start..]
            .iter()
            .position(|&c| c == 0)
            .map(|p| start + p)
            .ok_or_else(|| ElfError::at(off as u64, "unterminated string"))?;
        String::from_utf8(bytes[start..end].to_vec())
            .map_err(|_| ElfError::at(off as u64, "non-UTF-8 string"))
    }
}

// ---------------------------------------------------------------------
// Object model
// ---------------------------------------------------------------------

/// Where a symbol lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolPlace {
    /// Undefined (external reference).
    Undef,
    /// Absolute value.
    Abs,
    /// COMMON block: `value` holds the alignment, `size` the byte size.
    Common,
    /// Defined in `sections[idx]` (model index, not file shndx).
    Section(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub bind: u8, // STB_*
    pub typ: u8,  // STT_*
    pub vis: u8,  // STV_*
    pub place: SymbolPlace,
    pub value: u64,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rela {
    pub offset: u64,
    /// Index into `ObjectFile.symbols` (model index).
    pub symbol: usize,
    pub r_type: u32,
    pub addend: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addralign: u64,
    /// Content bytes. Empty for SHT_NOBITS; `nobits_size` carries the
    /// size then.
    pub data: Vec<u8>,
    /// Size for SHT_NOBITS sections (`.bss`). Ignored otherwise.
    pub nobits_size: u64,
    pub relas: Vec<Rela>,
}

impl Section {
    pub fn progbits(name: &str, sh_flags: u64, align: u64) -> Self {
        Self {
            name: name.into(),
            sh_type: SHT_PROGBITS,
            sh_flags,
            sh_addralign: align,
            data: Vec::new(),
            nobits_size: 0,
            relas: Vec::new(),
        }
    }

    pub fn text() -> Self {
        Self::progbits(".text", SHF_ALLOC | SHF_EXECINSTR, 16)
    }

    pub fn data() -> Self {
        Self::progbits(".data", SHF_ALLOC | SHF_WRITE, 8)
    }

    pub fn rodata() -> Self {
        Self::progbits(".rodata", SHF_ALLOC, 8)
    }

    pub fn bss() -> Self {
        Self {
            name: ".bss".into(),
            sh_type: SHT_NOBITS,
            sh_flags: SHF_ALLOC | SHF_WRITE,
            sh_addralign: 8,
            data: Vec::new(),
            nobits_size: 0,
            relas: Vec::new(),
        }
    }

    pub fn size(&self) -> u64 {
        if self.sh_type == SHT_NOBITS {
            self.nobits_size
        } else {
            self.data.len() as u64
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectFile {
    /// EM_X86_64 (EM_AARCH64 reserved for x15).
    pub machine: u16,
    /// ELFOSABI_FREEBSD on FreeBSD targets, ELFOSABI_NONE on Linux —
    /// gas brands relocatables per OS and the differential compares it.
    pub osabi: u8,
    pub sections: Vec<Section>,
    pub symbols: Vec<Symbol>,
}

impl ObjectFile {
    pub fn new(machine: u16, osabi: u8) -> Self {
        Self {
            machine,
            osabi,
            sections: Vec::new(),
            symbols: Vec::new(),
        }
    }

    pub fn section_by_name(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }
}

// ---------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------

/// Validate relocations against the object's machine: known type,
/// offset+width in bounds, symbol index in range. Loud errors over
/// silently emitting a bad object.
pub fn validate(obj: &ObjectFile) -> Result<(), ElfError> {
    for sec in &obj.sections {
        for r in &sec.relas {
            if r.symbol >= obj.symbols.len() {
                return Err(ElfError::new(format!(
                    "{}: rela at {:#x}: symbol index {} out of range ({} symbols)",
                    sec.name,
                    r.offset,
                    r.symbol,
                    obj.symbols.len()
                )));
            }
            let width = match obj.machine {
                EM_X86_64 => reloc::x86_64::width(r.r_type),
                m => {
                    return Err(ElfError::new(format!(
                        "unsupported machine {} for relocation validation",
                        m
                    )))
                }
            };
            let Some(width) = width else {
                return Err(ElfError::new(format!(
                    "{}: rela at {:#x}: unknown relocation type {} for machine {}",
                    sec.name, r.offset, r.r_type, obj.machine
                )));
            };
            if sec.sh_type == SHT_NOBITS {
                return Err(ElfError::new(format!(
                    "{}: relocation in SHT_NOBITS section",
                    sec.name
                )));
            }
            if r.offset + width > sec.data.len() as u64 {
                return Err(ElfError::new(format!(
                    "{}: rela at {:#x} (width {}) out of section bounds ({})",
                    sec.name,
                    r.offset,
                    width,
                    sec.data.len()
                )));
            }
        }
    }
    for (i, sym) in obj.symbols.iter().enumerate() {
        if let SymbolPlace::Section(idx) = sym.place {
            if idx >= obj.sections.len() {
                return Err(ElfError::new(format!(
                    "symbol {} '{}': section index {} out of range",
                    i, sym.name, idx
                )));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------

fn align_up(v: u64, align: u64) -> u64 {
    if align <= 1 {
        return v;
    }
    v.div_ceil(align) * align
}

/// Serialize an `ObjectFile` to ELF64 `ET_REL` bytes.
///
/// Deterministic: output is a pure function of the model. Symbols are
/// emitted locals-first (stable within each partition) and rela symbol
/// indexes are remapped accordingly; `.symtab.sh_info` = index of the
/// first global.
///
/// File layout: ehdr, section contents (model order, 8-byte aligned,
/// NOBITS consuming no bytes but taking the current offset like gas),
/// symtab, strtab, rela bodies, shstrtab, then the section header
/// table: [null, contents..., .note.GNU-stack, .symtab, .strtab,
/// .rela.X..., .shstrtab].
pub fn write_elf(obj: &ObjectFile) -> Result<Vec<u8>, ElfError> {
    validate(obj)?;

    // --- Symbol ordering: locals first, stable. Model idx -> file idx
    // (1-based; file idx 0 is the null symbol).
    let n_model = obj.symbols.len();
    let mut order: Vec<usize> = Vec::with_capacity(n_model);
    order.extend((0..n_model).filter(|&i| obj.symbols[i].bind == STB_LOCAL));
    let first_global_file_idx = 1 + order.len() as u32;
    order.extend((0..n_model).filter(|&i| obj.symbols[i].bind != STB_LOCAL));
    let mut model_to_file = vec![0u32; n_model];
    for (file_minus_1, &model_idx) in order.iter().enumerate() {
        model_to_file[model_idx] = 1 + file_minus_1 as u32;
    }

    // --- String tables.
    let mut strtab = StringTable::new();
    let sym_names: Vec<u32> = order
        .iter()
        .map(|&i| strtab.intern(&obj.symbols[i].name))
        .collect();

    let mut shstrtab = StringTable::new();

    // --- Build symtab body.
    let mut symtab_body: Vec<u8> = Vec::with_capacity((1 + n_model) * SYM_SIZE);
    Elf64Sym {
        st_name: 0,
        st_info: 0,
        st_other: 0,
        st_shndx: SHN_UNDEF,
        st_value: 0,
        st_size: 0,
    }
    .write(&mut symtab_body);
    // Model section idx -> file shndx: null(0), contents follow in
    // model order starting at 1.
    let file_shndx = |model_idx: usize| -> u16 { (1 + model_idx) as u16 };
    for (k, &i) in order.iter().enumerate() {
        let s = &obj.symbols[i];
        let shndx = match s.place {
            SymbolPlace::Undef => SHN_UNDEF,
            SymbolPlace::Abs => SHN_ABS,
            SymbolPlace::Common => SHN_COMMON,
            SymbolPlace::Section(idx) => file_shndx(idx),
        };
        Elf64Sym {
            st_name: sym_names[k],
            st_info: Elf64Sym::info(s.bind, s.typ),
            st_other: s.vis,
            st_shndx: shndx,
            st_value: s.value,
            st_size: s.size,
        }
        .write(&mut symtab_body);
    }

    // --- Section header list assembly. Order:
    // [0] null
    // [1..=n] content sections (model order)
    // [n+1] .note.GNU-stack
    // [n+2] .symtab, [n+3] .strtab
    // then one .rela.X per relocated content section, then .shstrtab.
    let n_contents = obj.sections.len();
    let note_idx = 1 + n_contents;
    let symtab_idx = note_idx + 1;
    let strtab_idx = symtab_idx + 1;
    let relocated: Vec<usize> = (0..n_contents)
        .filter(|&i| !obj.sections[i].relas.is_empty())
        .collect();
    let first_rela_idx = strtab_idx + 1;
    let shstrtab_idx = first_rela_idx + relocated.len();
    let e_shnum = (shstrtab_idx + 1) as u16;

    // --- Lay out file contents.
    let mut body: Vec<u8> = Vec::new(); // everything after the ehdr
    let base = EHDR_SIZE as u64;
    let mut shdrs: Vec<Elf64Shdr> = Vec::with_capacity(e_shnum as usize);
    shdrs.push(Elf64Shdr {
        sh_name: 0,
        sh_type: SHT_NULL,
        sh_flags: 0,
        sh_addr: 0,
        sh_offset: 0,
        sh_size: 0,
        sh_link: 0,
        sh_info: 0,
        sh_addralign: 0,
        sh_entsize: 0,
    });

    let place = |body: &mut Vec<u8>, align: u64, bytes: &[u8]| -> u64 {
        let here = base + body.len() as u64;
        let aligned = align_up(here, align.max(1));
        body.resize(body.len() + (aligned - here) as usize, 0);
        let off = base + body.len() as u64;
        body.extend_from_slice(bytes);
        off
    };

    for sec in &obj.sections {
        let name_off = shstrtab.intern(&sec.name);
        let (offset, size) = if sec.sh_type == SHT_NOBITS {
            // gas convention: NOBITS still takes the current file
            // offset; it just contributes no bytes.
            let here = align_up(base + body.len() as u64, sec.sh_addralign.max(1));
            (here, sec.nobits_size)
        } else {
            let off = place(&mut body, sec.sh_addralign, &sec.data);
            (off, sec.data.len() as u64)
        };
        shdrs.push(Elf64Shdr {
            sh_name: name_off,
            sh_type: sec.sh_type,
            sh_flags: sec.sh_flags,
            sh_addr: 0,
            sh_offset: offset,
            sh_size: size,
            sh_link: 0,
            sh_info: 0,
            sh_addralign: sec.sh_addralign,
            sh_entsize: 0,
        });
    }

    // .note.GNU-stack: empty PROGBITS, no flags — absence makes GNU ld
    // warn about executable stacks.
    let note_name = shstrtab.intern(".note.GNU-stack");
    let note_off = base + body.len() as u64;
    shdrs.push(Elf64Shdr {
        sh_name: note_name,
        sh_type: SHT_PROGBITS,
        sh_flags: 0,
        sh_addr: 0,
        sh_offset: note_off,
        sh_size: 0,
        sh_link: 0,
        sh_info: 0,
        sh_addralign: 1,
        sh_entsize: 0,
    });

    let symtab_name = shstrtab.intern(".symtab");
    let symtab_off = place(&mut body, 8, &symtab_body);
    shdrs.push(Elf64Shdr {
        sh_name: symtab_name,
        sh_type: SHT_SYMTAB,
        sh_flags: 0,
        sh_addr: 0,
        sh_offset: symtab_off,
        sh_size: symtab_body.len() as u64,
        sh_link: strtab_idx as u32,
        sh_info: first_global_file_idx,
        sh_addralign: 8,
        sh_entsize: SYM_SIZE as u64,
    });

    let strtab_name = shstrtab.intern(".strtab");
    let strtab_off = place(&mut body, 1, strtab.bytes());
    shdrs.push(Elf64Shdr {
        sh_name: strtab_name,
        sh_type: SHT_STRTAB,
        sh_flags: 0,
        sh_addr: 0,
        sh_offset: strtab_off,
        sh_size: strtab.bytes().len() as u64,
        sh_link: 0,
        sh_info: 0,
        sh_addralign: 1,
        sh_entsize: 0,
    });

    for &sec_idx in &relocated {
        let sec = &obj.sections[sec_idx];
        let rela_name = shstrtab.intern(&format!(".rela{}", sec.name));
        let mut rela_body: Vec<u8> = Vec::with_capacity(sec.relas.len() * RELA_SIZE);
        for r in &sec.relas {
            Elf64Rela {
                r_offset: r.offset,
                r_info: Elf64Rela::info(model_to_file[r.symbol], r.r_type),
                r_addend: r.addend,
            }
            .write(&mut rela_body);
        }
        let off = place(&mut body, 8, &rela_body);
        shdrs.push(Elf64Shdr {
            sh_name: rela_name,
            sh_type: SHT_RELA,
            sh_flags: 0,
            sh_addr: 0,
            sh_offset: off,
            sh_size: rela_body.len() as u64,
            sh_link: symtab_idx as u32,
            sh_info: file_shndx(sec_idx) as u32,
            sh_addralign: 8,
            sh_entsize: RELA_SIZE as u64,
        });
    }

    let shstr_name = shstrtab.intern(".shstrtab");
    let shstr_bytes = shstrtab.bytes().to_vec();
    let shstr_off = place(&mut body, 1, &shstr_bytes);
    shdrs.push(Elf64Shdr {
        sh_name: shstr_name,
        sh_type: SHT_STRTAB,
        sh_flags: 0,
        sh_addr: 0,
        sh_offset: shstr_off,
        sh_size: shstr_bytes.len() as u64,
        sh_link: 0,
        sh_info: 0,
        sh_addralign: 1,
        sh_entsize: 0,
    });

    // Section header table, 8-aligned.
    let here = base + body.len() as u64;
    let sh_off = align_up(here, 8);
    body.resize(body.len() + (sh_off - here) as usize, 0);
    let e_shoff = base + body.len() as u64;
    debug_assert_eq!(e_shoff, sh_off);
    for sh in &shdrs {
        let mut tmp = Vec::with_capacity(SHDR_SIZE);
        sh.write(&mut tmp);
        body.extend_from_slice(&tmp);
    }
    debug_assert_eq!(shdrs.len(), e_shnum as usize);

    let mut out = Vec::with_capacity(EHDR_SIZE + body.len());
    Elf64Ehdr {
        osabi: obj.osabi,
        abiversion: 0,
        e_type: ET_REL,
        e_machine: obj.machine,
        e_entry: 0,
        e_phoff: 0,
        e_shoff,
        e_flags: 0,
        e_phentsize: 0,
        e_phnum: 0,
        e_shentsize: SHDR_SIZE as u16,
        e_shnum,
        e_shstrndx: shstrtab_idx as u16,
    }
    .write(&mut out);
    out.extend_from_slice(&body);
    Ok(out)
}

/// Convenience: write to an io sink.
pub fn write_elf_to(obj: &ObjectFile, w: &mut impl Write) -> Result<(), ElfError> {
    let bytes = write_elf(obj)?;
    w.write_all(&bytes)
        .map_err(|e| ElfError::new(format!("write: {}", e)))
}

// ---------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------

/// Parse an ELF64 `ET_REL` object back into the model. Round-trip
/// partner of `write_elf`; also consumed by the differential harness
/// to lift system-assembler output.
pub fn parse_elf(bytes: &[u8]) -> Result<ObjectFile, ElfError> {
    let ehdr = Elf64Ehdr::parse(bytes)?;
    if ehdr.e_type != ET_REL {
        return Err(ElfError::at(
            16,
            format!("not ET_REL (e_type {})", ehdr.e_type),
        ));
    }
    if ehdr.e_shentsize as usize != SHDR_SIZE {
        return Err(ElfError::at(
            58,
            format!("unexpected e_shentsize {}", ehdr.e_shentsize),
        ));
    }
    let shoff = ehdr.e_shoff as usize;
    let shnum = ehdr.e_shnum as usize;
    let mut shdrs = Vec::with_capacity(shnum);
    for i in 0..shnum {
        shdrs.push(Elf64Shdr::parse(bytes, shoff + i * SHDR_SIZE)?);
    }
    if shdrs.is_empty() {
        return Err(ElfError::new("no sections"));
    }

    let shstr = &shdrs
        .get(ehdr.e_shstrndx as usize)
        .ok_or_else(|| ElfError::new("e_shstrndx out of range"))?;
    let shstr_bytes = section_bytes(bytes, shstr)?;
    let sec_name =
        |sh: &Elf64Shdr| -> Result<String, ElfError> { StringTable::read(shstr_bytes, sh.sh_name) };

    // First pass: identify symtab/strtab and content sections. Content
    // = everything that is not NULL/SYMTAB/STRTAB/RELA and not
    // .note.GNU-stack (synthesized by the writer).
    let mut symtab: Option<(usize, &Elf64Shdr)> = None;
    for (i, sh) in shdrs.iter().enumerate() {
        if sh.sh_type == SHT_SYMTAB {
            if symtab.is_some() {
                return Err(ElfError::new("multiple SHT_SYMTAB sections"));
            }
            symtab = Some((i, sh));
        }
    }
    // gas omits .symtab entirely for objects that define no symbols;
    // treat that as an empty symbol table.
    let (symtab_idx, strtab_bytes): (usize, &[u8]) = match symtab {
        Some((idx, sh)) => {
            let strtab_sh = shdrs
                .get(sh.sh_link as usize)
                .ok_or_else(|| ElfError::new(".symtab sh_link out of range"))?;
            (idx, section_bytes(bytes, strtab_sh)?)
        }
        None => (usize::MAX, &[]),
    };
    let symtab_sh = symtab.map(|(_, sh)| sh);

    // Map file section index -> model content index.
    let mut file_to_model: HashMap<usize, usize> = HashMap::new();
    let mut sections: Vec<Section> = Vec::new();
    for (i, sh) in shdrs.iter().enumerate() {
        let keep = !matches!(sh.sh_type, SHT_NULL | SHT_SYMTAB | SHT_STRTAB | SHT_RELA);
        if !keep {
            continue;
        }
        let name = sec_name(sh)?;
        if name == ".note.GNU-stack" || name == ".comment" || name.starts_with(".note.gnu") {
            continue;
        }
        let data = if sh.sh_type == SHT_NOBITS {
            Vec::new()
        } else {
            section_bytes(bytes, sh)?.to_vec()
        };
        file_to_model.insert(i, sections.len());
        sections.push(Section {
            name,
            sh_type: sh.sh_type,
            sh_flags: sh.sh_flags,
            sh_addralign: sh.sh_addralign,
            nobits_size: if sh.sh_type == SHT_NOBITS {
                sh.sh_size
            } else {
                0
            },
            data,
            relas: Vec::new(),
        });
    }

    // Symbols. File idx -> model idx (skipping the null and SECTION
    // symbols, which the writer synthesizes/omits — but keep a map so
    // relas against section symbols can be re-pointed).
    let symtab_bytes: &[u8] = match symtab_sh {
        Some(sh) => section_bytes(bytes, sh)?,
        None => &[],
    };
    if !symtab_bytes.len().is_multiple_of(SYM_SIZE) {
        return Err(ElfError::new(".symtab size not a multiple of 24"));
    }
    let nsyms = symtab_bytes.len() / SYM_SIZE;
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut sym_file_to_model: HashMap<usize, usize> = HashMap::new();
    // For SECTION symbols: file sym idx -> file section idx.
    let mut section_syms: HashMap<usize, usize> = HashMap::new();
    for i in 1..nsyms {
        let raw = Elf64Sym::parse(symtab_bytes, i * SYM_SIZE)?;
        if raw.typ() == STT_SECTION {
            section_syms.insert(i, raw.st_shndx as usize);
            continue;
        }
        let name = StringTable::read(strtab_bytes, raw.st_name)?;
        let place = match raw.st_shndx {
            SHN_UNDEF => SymbolPlace::Undef,
            SHN_ABS => SymbolPlace::Abs,
            SHN_COMMON => SymbolPlace::Common,
            shndx => {
                let model = file_to_model
                    .get(&(shndx as usize))
                    .copied()
                    .ok_or_else(|| {
                        ElfError::new(format!(
                            "symbol '{}' references untracked section {}",
                            name, shndx
                        ))
                    })?;
                SymbolPlace::Section(model)
            }
        };
        sym_file_to_model.insert(i, symbols.len());
        symbols.push(Symbol {
            name,
            bind: raw.bind(),
            typ: raw.typ(),
            vis: raw.st_other & 0x3,
            place,
            value: raw.st_value,
            size: raw.st_size,
        });
    }

    // Relocations. Relas against SECTION symbols are re-pointed at a
    // synthesized local symbol for that section so the model stays
    // closed. (gas emits section-relative relocations for local data.)
    let mut section_sym_model: HashMap<usize, usize> = HashMap::new();
    for (i, sh) in shdrs.iter().enumerate() {
        if sh.sh_type != SHT_RELA {
            continue;
        }
        if sh.sh_link as usize != symtab_idx {
            return Err(ElfError::new(format!(
                "rela section {} links symtab {} (expected {})",
                i, sh.sh_link, symtab_idx
            )));
        }
        let Some(&target_model) = file_to_model.get(&(sh.sh_info as usize)) else {
            continue; // relocations for a skipped section (e.g. .eh_frame filtered later)
        };
        let body = section_bytes(bytes, sh)?;
        if !body.len().is_multiple_of(RELA_SIZE) {
            return Err(ElfError::new("rela size not a multiple of 24"));
        }
        let mut relas = Vec::with_capacity(body.len() / RELA_SIZE);
        for k in 0..body.len() / RELA_SIZE {
            let raw = Elf64Rela::parse(body, k * RELA_SIZE)?;
            let file_sym = raw.sym() as usize;
            let model_sym = if let Some(&m) = sym_file_to_model.get(&file_sym) {
                m
            } else if let Some(&file_sec) = section_syms.get(&file_sym) {
                *section_sym_model.entry(file_sec).or_insert_with(|| {
                    let target = file_to_model.get(&file_sec).copied();
                    let name = target
                        .map(|t| sections[t].name.clone())
                        .unwrap_or_else(|| format!("<section {}>", file_sec));
                    symbols.push(Symbol {
                        name,
                        bind: STB_LOCAL,
                        typ: STT_SECTION,
                        vis: STV_DEFAULT,
                        place: target
                            .map(SymbolPlace::Section)
                            .unwrap_or(SymbolPlace::Undef),
                        value: 0,
                        size: 0,
                    });
                    symbols.len() - 1
                })
            } else {
                return Err(ElfError::new(format!(
                    "rela references unknown symbol index {}",
                    file_sym
                )));
            };
            relas.push(Rela {
                offset: raw.r_offset,
                symbol: model_sym,
                r_type: raw.r_type(),
                addend: raw.r_addend,
            });
        }
        sections[target_model].relas = relas;
    }

    Ok(ObjectFile {
        machine: ehdr.e_machine,
        osabi: ehdr.osabi,
        sections,
        symbols,
    })
}

fn section_bytes<'a>(bytes: &'a [u8], sh: &Elf64Shdr) -> Result<&'a [u8], ElfError> {
    if sh.sh_type == SHT_NOBITS {
        return Ok(&[]);
    }
    let start = sh.sh_offset as usize;
    let end = start + sh.sh_size as usize;
    if end > bytes.len() {
        return Err(ElfError::at(
            sh.sh_offset,
            format!("section extends past EOF (size {:#x})", sh.sh_size),
        ));
    }
    Ok(&bytes[start..end])
}

// ---------------------------------------------------------------------
// Unit tests: parse(write(x)) == x per wire struct, field-boundary
// values, plus writer round-trip and determinism.
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::reloc::x86_64::*;
    use super::*;

    #[test]
    fn ehdr_roundtrip() {
        let h = Elf64Ehdr {
            osabi: ELFOSABI_FREEBSD,
            abiversion: 0,
            e_type: ET_REL,
            e_machine: EM_X86_64,
            e_entry: 0,
            e_phoff: 0,
            e_shoff: 0x1234_5678_9abc,
            e_flags: 0,
            e_phentsize: 0,
            e_phnum: 0,
            e_shentsize: SHDR_SIZE as u16,
            e_shnum: 0xffff,
            e_shstrndx: 0xfffe,
        };
        let mut b = Vec::new();
        h.write(&mut b);
        assert_eq!(b.len(), EHDR_SIZE);
        assert_eq!(Elf64Ehdr::parse(&b).unwrap(), h);
    }

    #[test]
    fn shdr_roundtrip() {
        let sh = Elf64Shdr {
            sh_name: u32::MAX,
            sh_type: SHT_RELA,
            sh_flags: u64::MAX,
            sh_addr: 1,
            sh_offset: u64::MAX - 1,
            sh_size: 7,
            sh_link: 3,
            sh_info: 4,
            sh_addralign: 16,
            sh_entsize: RELA_SIZE as u64,
        };
        let mut b = Vec::new();
        sh.write(&mut b);
        assert_eq!(b.len(), SHDR_SIZE);
        assert_eq!(Elf64Shdr::parse(&b, 0).unwrap(), sh);
    }

    #[test]
    fn sym_roundtrip_and_info_packing() {
        let s = Elf64Sym {
            st_name: 5,
            st_info: Elf64Sym::info(STB_GLOBAL, STT_FUNC),
            st_other: STV_HIDDEN,
            st_shndx: SHN_COMMON,
            st_value: u64::MAX,
            st_size: 0,
        };
        assert_eq!(s.bind(), STB_GLOBAL);
        assert_eq!(s.typ(), STT_FUNC);
        let mut b = Vec::new();
        s.write(&mut b);
        assert_eq!(b.len(), SYM_SIZE);
        assert_eq!(Elf64Sym::parse(&b, 0).unwrap(), s);
    }

    #[test]
    fn rela_roundtrip_negative_addend() {
        let r = Elf64Rela {
            r_offset: 0x40,
            r_info: Elf64Rela::info(7, R_X86_64_PLT32),
            r_addend: -4,
        };
        assert_eq!(r.sym(), 7);
        assert_eq!(r.r_type(), R_X86_64_PLT32);
        let mut b = Vec::new();
        r.write(&mut b);
        assert_eq!(b.len(), RELA_SIZE);
        assert_eq!(Elf64Rela::parse(&b, 0).unwrap(), r);
    }

    #[test]
    fn strtab_interns_and_reads() {
        let mut t = StringTable::new();
        let a = t.intern("alpha");
        let b = t.intern("beta");
        let a2 = t.intern("alpha");
        assert_eq!(a, a2);
        assert_ne!(a, b);
        assert_eq!(t.intern(""), 0);
        assert_eq!(StringTable::read(t.bytes(), a).unwrap(), "alpha");
        assert_eq!(StringTable::read(t.bytes(), b).unwrap(), "beta");
        assert_eq!(StringTable::read(t.bytes(), 0).unwrap(), "");
    }

    fn sample_object() -> ObjectFile {
        let mut obj = ObjectFile::new(EM_X86_64, ELFOSABI_FREEBSD);
        let mut text = Section::text();
        // call ext  (E8 + rel32; gas writes the -4 addend bytes too)
        text.data = vec![0xe8, 0xfc, 0xff, 0xff, 0xff, 0xc3];
        text.relas.push(Rela {
            offset: 1,
            symbol: 1, // ext
            r_type: R_X86_64_PLT32,
            addend: -4,
        });
        let mut data = Section::data();
        data.data = vec![0; 8];
        data.relas.push(Rela {
            offset: 0,
            symbol: 0, // f
            r_type: R_X86_64_64,
            addend: 0,
        });
        let mut bss = Section::bss();
        bss.nobits_size = 32;
        obj.sections.push(text);
        obj.sections.push(data);
        obj.sections.push(bss);
        obj.symbols.push(Symbol {
            name: "f".into(),
            bind: STB_GLOBAL,
            typ: STT_FUNC,
            vis: STV_DEFAULT,
            place: SymbolPlace::Section(0),
            value: 0,
            size: 6,
        });
        obj.symbols.push(Symbol {
            name: "ext".into(),
            bind: STB_GLOBAL,
            typ: STT_NOTYPE,
            vis: STV_DEFAULT,
            place: SymbolPlace::Undef,
            value: 0,
            size: 0,
        });
        obj.symbols.push(Symbol {
            name: "lcl".into(),
            bind: STB_LOCAL,
            typ: STT_OBJECT,
            vis: STV_DEFAULT,
            place: SymbolPlace::Section(1),
            value: 0,
            size: 8,
        });
        obj.symbols.push(Symbol {
            name: "cmn".into(),
            bind: STB_GLOBAL,
            typ: STT_OBJECT,
            vis: STV_DEFAULT,
            place: SymbolPlace::Common,
            value: 8, // alignment
            size: 128,
        });
        obj
    }

    #[test]
    fn writer_is_deterministic() {
        let obj = sample_object();
        assert_eq!(write_elf(&obj).unwrap(), write_elf(&obj).unwrap());
    }

    #[test]
    fn whole_file_model_roundtrip() {
        let obj = sample_object();
        let bytes = write_elf(&obj).unwrap();
        let back = parse_elf(&bytes).unwrap();
        assert_eq!(back.machine, obj.machine);
        assert_eq!(back.osabi, obj.osabi);
        // Section content survives.
        assert_eq!(
            back.section_by_name(".text").unwrap().data,
            obj.section_by_name(".text").unwrap().data
        );
        assert_eq!(back.section_by_name(".bss").unwrap().nobits_size, 32);
        // Symbols survive as a set with binding partition; local-first
        // reordering means indexes may differ.
        let names: Vec<&str> = back.symbols.iter().map(|s| s.name.as_str()).collect();
        for expect in ["f", "ext", "lcl", "cmn"] {
            assert!(names.contains(&expect), "missing {}: {:?}", expect, names);
        }
        let lcl_pos = back.symbols.iter().position(|s| s.name == "lcl").unwrap();
        let f_pos = back.symbols.iter().position(|s| s.name == "f").unwrap();
        assert!(lcl_pos < f_pos, "locals must precede globals");
        // Relocation survives with the right symbol linkage.
        let text = back.section_by_name(".text").unwrap();
        assert_eq!(text.relas.len(), 1);
        let r = &text.relas[0];
        assert_eq!(r.r_type, R_X86_64_PLT32);
        assert_eq!(r.addend, -4);
        assert_eq!(back.symbols[r.symbol].name, "ext");
        // Common symbol keeps alignment-in-value convention.
        let cmn = back.symbols.iter().find(|s| s.name == "cmn").unwrap();
        assert_eq!(cmn.place, SymbolPlace::Common);
        assert_eq!(cmn.value, 8);
        assert_eq!(cmn.size, 128);
    }

    #[test]
    fn validate_rejects_unknown_reloc_and_oob() {
        let mut obj = sample_object();
        obj.sections[0].relas[0].r_type = 9999;
        assert!(write_elf(&obj).is_err());
        let mut obj = sample_object();
        obj.sections[0].relas[0].offset = 1000;
        assert!(write_elf(&obj).is_err());
        let mut obj = sample_object();
        obj.sections[0].relas[0].symbol = 99;
        assert!(write_elf(&obj).is_err());
    }

    #[test]
    fn symtab_sh_info_is_first_global() {
        let obj = sample_object();
        let bytes = write_elf(&obj).unwrap();
        let ehdr = Elf64Ehdr::parse(&bytes).unwrap();
        let mut first_global = None;
        for i in 0..ehdr.e_shnum as usize {
            let sh = Elf64Shdr::parse(&bytes, ehdr.e_shoff as usize + i * SHDR_SIZE).unwrap();
            if sh.sh_type == SHT_SYMTAB {
                first_global = Some(sh.sh_info);
                let body = &bytes[sh.sh_offset as usize..(sh.sh_offset + sh.sh_size) as usize];
                let n = body.len() / SYM_SIZE;
                for k in 0..n {
                    let sym = Elf64Sym::parse(body, k * SYM_SIZE).unwrap();
                    let is_local = sym.bind() == STB_LOCAL;
                    assert_eq!(
                        (k as u32) < sh.sh_info,
                        is_local || k == 0,
                        "symbol {} violates local/global partition",
                        k
                    );
                }
            }
        }
        // sample has 1 local + null => first global at index 2.
        assert_eq!(first_global, Some(2));
    }
}
