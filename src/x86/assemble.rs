//! x86_64 two-pass assembler (x14): parsed statements → elf::ObjectFile.
//!
//! Pass 1 builds per-section item streams (encoded bytes, relaxable
//! branches, alignment marks, data) and the symbol bookkeeping
//! (.globl/.local/.weak/.type/.size/.comm/.file). The relaxation pass then
//! runs a fixed-point over each text section: every intra-section
//! jmp/jcc starts optimistically at rel8 and grows to rel32 until no
//! displacement overflows — growth is monotonic, so it terminates.
//! This matches gas, keeping the whole-file differential byte-exact.
//!
//! References to defined local symbols (`.L*` labels, non-.globl
//! labels, `.local` commons) follow the gas rules: PC-relative
//! fixups inside one section resolve at assembly time with no
//! relocation; cross-section ones become relocations against the
//! target section's STT_SECTION symbol with the offset folded into
//! the addend. Global and weak symbols always keep a symbol
//! relocation (they can be preempted).

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::assemble::AsmError;

use super::super::elf::{
    self, reloc::x86_64::*, ObjectFile, Rela, Section, Symbol, SymbolPlace, EM_X86_64, SHF_ALLOC,
    SHF_EXECINSTR, SHF_WRITE, SHT_NOBITS, SHT_NOTE, SHT_PROGBITS, STB_GLOBAL, STB_LOCAL, STB_WEAK,
    STT_FILE, STT_FUNC, STT_NOTYPE, STT_OBJECT, STT_SECTION, STV_DEFAULT,
};
use super::encode::{encode, InsnReloc};
use super::parse::{
    parse_bytes, DataAtom, DataItem, Directive, SectionType, SizeArg, Stmt, SymKind,
};
use super::Operand;

pub type AsmX86Error = AsmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchKind {
    Jmp,
    Jcc(u8),
}

#[derive(Debug)]
enum Item {
    /// Encoded bytes with item-relative relocations.
    Bytes(Vec<u8>, Vec<InsnReloc>),
    /// Data expressions are materialized only after relaxation fixes every
    /// label offset, allowing forward same-section differences.
    Data { width: usize, items: Vec<DataItem> },
    /// Zero-filled storage that advances the section size. In NOBITS
    /// sections this must not materialize bytes in memory.
    Zero(u64),
    /// Repeated-byte storage for `.space N,F` / `.skip N,F`.
    Fill { size: u64, byte: u8 },
    /// Relaxable branch to a section-local label.
    Branch { kind: BranchKind, label: String },
    /// `.p2align`: pad to `1 << pow`, subject to an optional maximum skip.
    /// An omitted fill selects text NOPs or data zeros.
    Align {
        pow: u32,
        fill: Option<u8>,
        max_skip: Option<u64>,
    },
    /// `.size sym, .-base`: records the dot position at the
    /// directive so the size is exact even with padding or local
    /// labels after the body. Zero width.
    SizeDot(String),
}

#[derive(Debug)]
struct LocatedItem {
    line: u32,
    col: u32,
    item: Item,
}

#[derive(Debug)]
struct LocatedReloc {
    line: u32,
    col: u32,
    reloc: InsnReloc,
}

#[derive(Debug, Default)]
struct SecBuild {
    items: Vec<LocatedItem>,
    /// label -> item index it precedes.
    labels: HashMap<String, usize>,
    label_order: Vec<String>,
    max_align: u64,
    sh_type: u32,
    sh_flags: u64,
}

impl SecBuild {
    fn new(sh_type: u32, sh_flags: u64) -> Self {
        Self {
            sh_type,
            sh_flags,
            ..Self::default()
        }
    }

    fn push(&mut self, line: u32, col: u32, item: Item) {
        self.items.push(LocatedItem { line, col, item });
    }
}

#[derive(Debug)]
struct SubsectionBuild {
    section: String,
    subsection: u32,
    build: SecBuild,
}

/// Concatenate each section's subsection streams in numeric order before
/// relaxation and layout. Subsections are an assembler-only organization:
/// labels, relocations, alignment, and `.` must all observe one final section
/// address space.
fn merge_subsections(streams: Vec<SubsectionBuild>) -> (Vec<(String, SecBuild)>, Vec<usize>) {
    let mut section_order = Vec::new();
    let mut merged_index = HashMap::new();
    for stream in &streams {
        if !merged_index.contains_key(&stream.section) {
            let index = section_order.len();
            section_order.push(stream.section.clone());
            merged_index.insert(stream.section.clone(), index);
        }
    }

    let section_metadata: HashMap<_, _> = streams
        .iter()
        .map(|stream| {
            (
                stream.section.clone(),
                (stream.build.sh_type, stream.build.sh_flags),
            )
        })
        .collect();
    let mut indexed_streams: Vec<_> = streams.into_iter().enumerate().collect();
    indexed_streams.sort_by_key(|(_, stream)| (merged_index[&stream.section], stream.subsection));

    let mut sections: Vec<_> = section_order
        .into_iter()
        .map(|name| {
            let (sh_type, sh_flags) = section_metadata[&name];
            (name, SecBuild::new(sh_type, sh_flags))
        })
        .collect();
    let mut stream_to_section = vec![0; indexed_streams.len()];
    for (stream_index, stream) in indexed_streams {
        let section_index = merged_index[&stream.section];
        stream_to_section[stream_index] = section_index;

        let target = &mut sections[section_index].1;
        debug_assert_eq!(target.sh_type, stream.build.sh_type);
        debug_assert_eq!(target.sh_flags, stream.build.sh_flags);
        let item_base = target.items.len();
        target.max_align = target.max_align.max(stream.build.max_align);
        target.items.extend(stream.build.items);
        target.label_order.extend(stream.build.label_order);
        for (label, item_index) in stream.build.labels {
            let previous = target.labels.insert(label, item_base + item_index);
            debug_assert!(previous.is_none(), "duplicate label survived pass 1");
        }
    }
    (sections, stream_to_section)
}

#[derive(Debug, Default, Clone)]
struct SymInfo {
    globl: bool,
    weak: bool,
    local: bool,
    typ: Option<u8>,
    size: Option<SizeArg>,
    size_line: u32,
    size_col: u32,
    size_section: Option<usize>,
}

/// An assembled x86 object plus the exact `.text` byte ranges emitted as
/// implicit NOP padding for `.p2align` directives.
#[derive(Debug)]
pub struct X86Assembly {
    pub object: ObjectFile,
    pub text_nop_padding: Vec<Range<usize>>,
}

pub fn assemble_x86(src: &str, osabi: u8) -> Result<ObjectFile, AsmX86Error> {
    assemble_x86_bytes(src.as_bytes(), osabi)
}

pub fn assemble_x86_bytes(src: &[u8], osabi: u8) -> Result<ObjectFile, AsmX86Error> {
    assemble_x86_bytes_with_provenance(src, osabi).map(|assembly| assembly.object)
}

pub fn assemble_x86_with_provenance(src: &str, osabi: u8) -> Result<X86Assembly, AsmX86Error> {
    assemble_x86_bytes_with_provenance(src.as_bytes(), osabi)
}

pub fn assemble_x86_bytes_with_provenance(
    src: &[u8],
    osabi: u8,
) -> Result<X86Assembly, AsmX86Error> {
    let stmts = parse_bytes(src).map_err(|e| AsmError::at(e.line, e.col, e.msg))?;

    // ---- Pass 1: build sections -----------------------------------
    let mut streams: Vec<SubsectionBuild> = Vec::new();
    let mut stream_index: HashMap<(String, u32), usize> = HashMap::new();
    let mut current: usize = usize::MAX;
    let mut syminfo: HashMap<String, SymInfo> = HashMap::new();
    let mut gnu_stack_flags: Option<u64> = None;
    // The first directive or definition that makes a named ELF symbol exist.
    // GNU as uses this order to interleave local symbols with `.file` groups.
    let mut symbol_creation_order: HashMap<String, usize> = HashMap::new();
    let mut file_symbols: Vec<(usize, String)> = Vec::new();
    // (sym, size, explicit alignment, line, column)
    let mut commons: Vec<(String, u64, Option<u64>, u32, u32)> = Vec::new();
    let mut common_names: HashSet<String> = HashSet::new();
    let mut global_common_names: HashSet<String> = HashSet::new();
    let mut local_common_names: HashSet<String> = HashSet::new();
    // label -> subsection stream index; remapped to the merged section index
    // before relaxation and relocation processing.
    let mut label_section: HashMap<String, usize> = HashMap::new();

    let ensure_stream = |name: &str,
                         subsection: u32,
                         streams: &mut Vec<SubsectionBuild>,
                         stream_index: &mut HashMap<(String, u32), usize>|
     -> usize {
        let key = (name.to_string(), subsection);
        if let Some(&i) = stream_index.get(&key) {
            return i;
        }
        let index = streams.len();
        let (sh_type, sh_flags) = default_section_metadata(name);
        streams.push(SubsectionBuild {
            section: name.to_string(),
            subsection,
            build: SecBuild::new(sh_type, sh_flags),
        });
        stream_index.insert(key, index);
        index
    };

    let err = |line: u32, col: u32, msg: String| AsmError::at(line, col, msg);

    // gas always creates .text/.data/.bss even when empty; match it so
    // objects diff clean against the system assembler.
    ensure_stream(".text", 0, &mut streams, &mut stream_index);
    ensure_stream(".data", 0, &mut streams, &mut stream_index);
    ensure_stream(".bss", 0, &mut streams, &mut stream_index);

    for (statement_order, located) in stmts.iter().enumerate() {
        let line = located.line;
        let col = located.col;
        match &located.stmt {
            Stmt::Label(name) => {
                symbol_creation_order
                    .entry(name.clone())
                    .or_insert(statement_order);
                if common_names.contains(name) {
                    return Err(err(
                        line,
                        col,
                        format!("symbol '{}' is already defined", name),
                    ));
                }
                if current == usize::MAX {
                    current = ensure_stream(".text", 0, &mut streams, &mut stream_index);
                }
                let sb = &mut streams[current].build;
                if label_section.insert(name.clone(), current).is_some() {
                    return Err(err(line, col, format!("duplicate label '{}'", name)));
                }
                sb.labels.insert(name.clone(), sb.items.len());
                sb.label_order.push(name.clone());
            }
            Stmt::Directive(d) => match d {
                Directive::Section {
                    name,
                    subsection,
                    flags,
                    section_type,
                } => {
                    let key = (name.clone(), *subsection);
                    let existed = stream_index.contains_key(&key);
                    current = ensure_stream(name, *subsection, &mut streams, &mut stream_index);
                    let sb = &mut streams[current].build;
                    let declared_type = section_type.map(|kind| match kind {
                        SectionType::Progbits => SHT_PROGBITS,
                        SectionType::Note => SHT_NOTE,
                        SectionType::Nobits => SHT_NOBITS,
                    });
                    let declared_flags = flags
                        .as_deref()
                        .map(parse_section_flags)
                        .transpose()
                        .map_err(|msg| err(line, col, msg))?;
                    if let Some(sh_type) = declared_type {
                        if existed && sb.sh_type != sh_type {
                            return Err(err(
                                line,
                                col,
                                format!("section '{}' redeclared with a different type", name),
                            ));
                        }
                        sb.sh_type = sh_type;
                    }
                    if let Some(sh_flags) = declared_flags {
                        if existed && sb.sh_flags != sh_flags {
                            return Err(err(
                                line,
                                col,
                                format!("section '{}' redeclared with different flags", name),
                            ));
                        }
                        sb.sh_flags = sh_flags;
                    }
                }
                Directive::NoteGnuStack { executable } => {
                    gnu_stack_flags.get_or_insert(if *executable { SHF_EXECINSTR } else { 0 });
                }
                Directive::Globl(s) => {
                    symbol_creation_order
                        .entry(s.clone())
                        .or_insert(statement_order);
                    syminfo.entry(s.clone()).or_default().globl = true;
                }
                Directive::Extern(s) => {
                    symbol_creation_order
                        .entry(s.clone())
                        .or_insert(statement_order);
                    syminfo.entry(s.clone()).or_default();
                }
                Directive::Weak(s) => {
                    symbol_creation_order
                        .entry(s.clone())
                        .or_insert(statement_order);
                    if global_common_names.contains(s) {
                        return Err(err(
                            line,
                            col,
                            format!("symbol '{}' can not be both weak and common", s),
                        ));
                    }
                    syminfo.entry(s.clone()).or_default().weak = true;
                }
                Directive::Local(s) => {
                    symbol_creation_order
                        .entry(s.clone())
                        .or_insert(statement_order);
                    syminfo.entry(s.clone()).or_default().local = true;
                }
                Directive::Type { sym, kind } => {
                    symbol_creation_order
                        .entry(sym.clone())
                        .or_insert(statement_order);
                    syminfo.entry(sym.clone()).or_default().typ = Some(match kind {
                        SymKind::Function => STT_FUNC,
                        SymKind::Object => STT_OBJECT,
                    })
                }
                Directive::Size { sym, arg } => {
                    symbol_creation_order
                        .entry(sym.clone())
                        .or_insert(statement_order);
                    if matches!(arg, SizeArg::DotMinus(_)) && current == usize::MAX {
                        return Err(err(
                            line,
                            col,
                            format!(".size {}, .-... before any section", sym),
                        ));
                    }
                    let e = syminfo.entry(sym.clone()).or_default();
                    e.size = Some(arg.clone());
                    e.size_line = line;
                    e.size_col = col;
                    e.size_section = matches!(arg, SizeArg::DotMinus(_)).then_some(current);
                    if matches!(arg, SizeArg::DotMinus(_)) {
                        streams[current]
                            .build
                            .push(line, col, Item::SizeDot(sym.clone()));
                    }
                }
                Directive::Comm { sym, size, align } => {
                    symbol_creation_order
                        .entry(sym.clone())
                        .or_insert(statement_order);
                    if label_section.contains_key(sym) {
                        return Err(err(
                            line,
                            col,
                            format!("symbol '{}' is already defined", sym),
                        ));
                    }
                    let info = syminfo.get(sym);
                    if info.is_some_and(|info| info.weak && !info.local) {
                        return Err(err(
                            line,
                            col,
                            format!("symbol '{}' can not be both weak and common", sym),
                        ));
                    }
                    let is_local = info.is_some_and(|info| info.local);
                    if is_local && !local_common_names.insert(sym.clone()) {
                        return Err(err(
                            line,
                            col,
                            format!("symbol '{}' is already defined", sym),
                        ));
                    }
                    if !is_local {
                        global_common_names.insert(sym.clone());
                    }
                    common_names.insert(sym.clone());
                    commons.push((sym.clone(), *size, *align, line, col))
                }
                Directive::File(name) => file_symbols.push((statement_order, name.clone())),
                Directive::P2Align {
                    pow,
                    fill,
                    max_skip,
                } => {
                    if current == usize::MAX {
                        current = ensure_stream(".text", 0, &mut streams, &mut stream_index);
                    }
                    let sb = &mut streams[current].build;
                    sb.max_align = sb.max_align.max(1u64 << pow);
                    sb.push(
                        line,
                        col,
                        Item::Align {
                            pow: *pow,
                            fill: *fill,
                            max_skip: *max_skip,
                        },
                    );
                }
                Directive::Byte(items)
                | Directive::Short(items)
                | Directive::Long(items)
                | Directive::Quad(items) => {
                    let width = match d {
                        Directive::Byte(_) => 1usize,
                        Directive::Short(_) => 2,
                        Directive::Long(_) => 4,
                        _ => 8,
                    };
                    if current == usize::MAX {
                        current = ensure_stream(".text", 0, &mut streams, &mut stream_index);
                    }
                    let is_nobits = streams[current].build.sh_type == SHT_NOBITS;
                    let has_nonzero_or_expr = items.iter().any(|item| match item {
                        DataItem::Num(value) => *value != 0,
                        _ => true,
                    });
                    if is_nobits && has_nonzero_or_expr {
                        return Err(err(
                            line,
                            col,
                            "attempt to store non-zero value in section `.bss'".into(),
                        ));
                    }
                    if is_nobits {
                        streams[current].build.push(
                            line,
                            col,
                            Item::Zero((width * items.len()) as u64),
                        );
                    } else {
                        streams[current].build.push(
                            line,
                            col,
                            Item::Data {
                                width,
                                items: items.clone(),
                            },
                        );
                    }
                }
                Directive::Ascii(strings) => {
                    if current == usize::MAX {
                        current = ensure_stream(".text", 0, &mut streams, &mut stream_index);
                    }
                    let bytes = strings.concat();
                    let is_nobits = streams[current].build.sh_type == SHT_NOBITS;
                    if is_nobits && bytes.iter().any(|&byte| byte != 0) {
                        return Err(err(
                            line,
                            col,
                            "attempt to store non-empty string in section `.bss'".into(),
                        ));
                    }
                    if is_nobits {
                        streams[current]
                            .build
                            .push(line, col, Item::Zero(bytes.len() as u64));
                    } else {
                        streams[current]
                            .build
                            .push(line, col, Item::Bytes(bytes, vec![]));
                    }
                }
                Directive::Asciz(strings) => {
                    if current == usize::MAX {
                        current = ensure_stream(".text", 0, &mut streams, &mut stream_index);
                    }
                    let mut bytes = Vec::new();
                    for string in strings {
                        bytes.extend_from_slice(string);
                        bytes.push(0);
                    }
                    let is_nobits = streams[current].build.sh_type == SHT_NOBITS;
                    if is_nobits && bytes.iter().any(|&byte| byte != 0) {
                        return Err(err(
                            line,
                            col,
                            "attempt to store non-empty string in section `.bss'".into(),
                        ));
                    }
                    if is_nobits {
                        streams[current]
                            .build
                            .push(line, col, Item::Zero(bytes.len() as u64));
                    } else {
                        streams[current]
                            .build
                            .push(line, col, Item::Bytes(bytes, vec![]));
                    }
                }
                Directive::Space { size, fill } => {
                    if current == usize::MAX {
                        current = ensure_stream(".text", 0, &mut streams, &mut stream_index);
                    }
                    if streams[current].build.sh_type == SHT_NOBITS && *fill != 0 {
                        return Err(err(
                            line,
                            col,
                            "attempt to store non-zero value in section `.bss'".into(),
                        ));
                    }
                    streams[current].build.push(
                        line,
                        col,
                        Item::Fill {
                            size: *size,
                            byte: *fill,
                        },
                    );
                }
                Directive::Zero(n) => {
                    if current == usize::MAX {
                        current = ensure_stream(".text", 0, &mut streams, &mut stream_index);
                    }
                    streams[current].build.push(line, col, Item::Zero(*n));
                }
            },
            Stmt::Insn { mnemonic, operands } => {
                if current == usize::MAX {
                    current = ensure_stream(".text", 0, &mut streams, &mut stream_index);
                }
                // Relaxable branch? jmp/jcc to a symbol that is a
                // file-local label (decided in pass 2 — here we defer
                // by recording the branch when the target LOOKS like
                // a label; unresolved ones are re-encoded as external
                // in layout when the label never appears).
                let branch = match (mnemonic.as_str(), operands.as_slice()) {
                    ("jmp", [Operand::Sym(l)]) => Some((BranchKind::Jmp, l.clone())),
                    (m, [Operand::Sym(l)]) if m != "jmp" => m
                        .strip_prefix('j')
                        .and_then(super::encode::cond_code_pub)
                        .map(|cc| (BranchKind::Jcc(cc), l.clone())),
                    _ => None,
                };
                if let Some((kind, label)) = branch {
                    streams[current]
                        .build
                        .push(line, col, Item::Branch { kind, label });
                    continue;
                }
                let enc = encode(mnemonic, operands)
                    .map_err(|e| err(line, col, format!("{}: {}", mnemonic, e)))?;
                if enc.label_fix.is_some() {
                    return Err(err(
                        line,
                        col,
                        format!("unexpected label fix for {}", mnemonic),
                    ));
                }
                streams[current].build.push(
                    line,
                    col,
                    Item::Bytes(enc.bytes, enc.reloc.into_iter().collect()),
                );
            }
        }
    }

    let (secs, stream_to_section) = merge_subsections(streams);
    for section in label_section.values_mut() {
        *section = stream_to_section[*section];
    }
    for info in syminfo.values_mut() {
        if let Some(section) = &mut info.size_section {
            *section = stream_to_section[*section];
        }
    }

    // ---- Relaxation + layout per section ---------------------------
    struct Laid {
        name: String,
        size: u64,
        bytes: Vec<u8>,
        relocs: Vec<LocatedReloc>, // section-relative offsets
        labels: HashMap<String, u64>,
        /// sym -> dot position at its `.size sym, .-base` directive.
        size_dot: HashMap<String, u64>,
        max_align: u64,
        text_nop_padding: Vec<Range<usize>>,
        sh_type: u32,
        sh_flags: u64,
    }
    let mut laid: Vec<Laid> = Vec::new();
    // Labels whose final post-relaxation offsets are known. Section order
    // places the built-in text/data/bss sections before debug/unwind sections,
    // so expressions emitted there can fold differences between text labels.
    let mut resolved_labels: HashMap<String, (usize, u64)> = HashMap::new();

    for (section_index, (name, sb)) in secs.iter().enumerate() {
        let is_text = name == ".text";
        // Branch sizing state: index into items -> long?
        let mut long: Vec<bool> = sb.items.iter().map(|_| false).collect();
        let branch_len = |kind: BranchKind, is_long: bool| -> u64 {
            match (kind, is_long) {
                (BranchKind::Jmp, false) => 2,
                (BranchKind::Jmp, true) => 5,
                (BranchKind::Jcc(_), false) => 2,
                (BranchKind::Jcc(_), true) => 6,
            }
        };
        let is_local_branch = |label: &str| {
            sb.labels.contains_key(label)
                && !syminfo.get(label).map(|info| info.weak).unwrap_or(false)
        };
        // Fixed-point.
        loop {
            // Compute offsets under current sizing.
            let mut offsets: Vec<u64> = Vec::with_capacity(sb.items.len() + 1);
            let mut pos: u64 = 0;
            for (i, located) in sb.items.iter().enumerate() {
                offsets.push(pos);
                let item = &located.item;
                let size = match item {
                    Item::Bytes(b, _) => b.len() as u64,
                    Item::Data { width, items } => (*width * items.len()) as u64,
                    Item::Zero(n) => *n,
                    Item::Fill { size, .. } => *size,
                    Item::Branch { kind, label } => {
                        if is_local_branch(label) {
                            branch_len(*kind, long[i])
                        } else {
                            branch_len(*kind, true)
                        }
                    }
                    Item::Align { pow, max_skip, .. } => {
                        align_pad(pos, *pow, *max_skip, located.line, located.col)?
                    }
                    Item::SizeDot(_) => 0,
                };
                pos = checked_layout_add(pos, size, located.line, located.col)?;
            }
            offsets.push(pos);
            // Grow any short branch whose displacement overflows i8.
            let mut grew = false;
            for (i, located) in sb.items.iter().enumerate() {
                let item = &located.item;
                if let Item::Branch { kind, label } = item {
                    if long[i] || !is_local_branch(label) {
                        continue;
                    }
                    let target_item = sb.labels[label];
                    let target_off = offsets[target_item];
                    let end = checked_layout_add(
                        offsets[i],
                        branch_len(*kind, false),
                        located.line,
                        located.col,
                    )?;
                    let disp = i128::from(target_off) - i128::from(end);
                    if i8::try_from(disp).is_err() {
                        long[i] = true;
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }

        // Materialize.
        let mut bytes: Vec<u8> = Vec::new();
        let mut relocs: Vec<LocatedReloc> = Vec::new();
        let mut text_nop_padding = Vec::new();
        let mut item_offsets: Vec<u64> = Vec::with_capacity(sb.items.len());
        // First recompute final offsets (same walk as above).
        {
            let mut pos = 0u64;
            for (i, located) in sb.items.iter().enumerate() {
                item_offsets.push(pos);
                let item = &located.item;
                let size = match item {
                    Item::Bytes(b, _) => b.len() as u64,
                    Item::Data { width, items } => (*width * items.len()) as u64,
                    Item::Zero(n) => *n,
                    Item::Fill { size, .. } => *size,
                    Item::Branch { kind, label } => {
                        if is_local_branch(label) {
                            branch_len(*kind, long[i])
                        } else {
                            branch_len(*kind, true)
                        }
                    }
                    Item::Align { pow, max_skip, .. } => {
                        align_pad(pos, *pow, *max_skip, located.line, located.col)?
                    }
                    Item::SizeDot(_) => 0,
                };
                pos = checked_layout_add(pos, size, located.line, located.col)?;
            }
            item_offsets.push(pos);
        }
        let final_size = item_offsets.last().copied().unwrap_or(0);
        let final_labels: HashMap<String, u64> = sb
            .labels
            .iter()
            .map(|(label, &item_idx)| {
                (
                    label.clone(),
                    item_offsets.get(item_idx).copied().unwrap_or(final_size),
                )
            })
            .collect();
        resolved_labels.extend(
            final_labels
                .iter()
                .map(|(name, &offset)| (name.clone(), (section_index, offset))),
        );
        let mut size_dot: HashMap<String, u64> = HashMap::new();
        let is_bss = sb.sh_type == SHT_NOBITS;
        let mut pos = 0u64;
        let reloc_offset = |offset: u64, line: u32, col: u32| -> Result<u32, AsmX86Error> {
            u32::try_from(offset).map_err(|_| {
                err(
                    line,
                    col,
                    format!("relocation offset {} exceeds u32", offset),
                )
            })
        };
        for (i, located) in sb.items.iter().enumerate() {
            let line = located.line;
            let col = located.col;
            let item = &located.item;
            match item {
                Item::Bytes(b, rs) => {
                    let base = reloc_offset(pos, line, col)?;
                    for r in rs {
                        let offset = base.checked_add(r.offset).ok_or_else(|| {
                            err(
                                line,
                                col,
                                format!("relocation offset {} + {} overflows u32", base, r.offset),
                            )
                        })?;
                        relocs.push(LocatedReloc {
                            line,
                            col,
                            reloc: InsnReloc {
                                offset,
                                ..r.clone()
                            },
                        });
                    }
                    if !is_bss {
                        reserve_materialized_bytes(&mut bytes, b.len() as u64, line, col)?;
                        bytes.extend_from_slice(b);
                    }
                    pos = checked_layout_add(pos, b.len() as u64, line, col)?;
                }
                Item::Data { width, items } => {
                    for data in items {
                        let field_pos = pos;
                        match data {
                            DataItem::Num(value) => {
                                bytes.extend_from_slice(&value.to_le_bytes()[..*width]);
                            }
                            DataItem::Sym { name, addend } => {
                                let r_type = data_reloc_type(*width, false).ok_or_else(|| {
                                    err(
                                        line,
                                        col,
                                        format!(
                                            "symbolic data item '{}' is unsupported at {} bytes",
                                            name, width
                                        ),
                                    )
                                })?;
                                relocs.push(LocatedReloc {
                                    line,
                                    col,
                                    reloc: InsnReloc {
                                        offset: reloc_offset(field_pos, line, col)?,
                                        sym: name.clone(),
                                        r_type,
                                        addend: *addend,
                                    },
                                });
                                bytes.resize(bytes.len() + *width, 0);
                            }
                            DataItem::Difference {
                                minuend,
                                subtrahend,
                                addend,
                            } => {
                                let atom_value = |atom: &DataAtom| -> Option<(usize, u64)> {
                                    match atom {
                                        DataAtom::Dot => Some((section_index, field_pos)),
                                        DataAtom::Sym(name) => resolved_labels.get(name).copied(),
                                    }
                                };
                                let resolved_difference =
                                    match (atom_value(minuend), atom_value(subtrahend)) {
                                        (Some((lhs_section, lhs)), Some((rhs_section, rhs)))
                                            if lhs_section == rhs_section =>
                                        {
                                            Some(
                                                (lhs as i64)
                                                    .wrapping_sub(rhs as i64)
                                                    .wrapping_add(*addend),
                                            )
                                        }
                                        _ => None,
                                    };
                                if let Some(value) = resolved_difference {
                                    bytes.extend_from_slice(&value.to_le_bytes()[..*width]);
                                } else if let DataAtom::Sym(name) = minuend {
                                    let (rhs_section, rhs) = atom_value(subtrahend).ok_or_else(|| {
                                        err(
                                            line,
                                            col,
                                            "subtrahend of a relocatable difference must be in the current section"
                                                .into(),
                                        )
                                    })?;
                                    if rhs_section != section_index {
                                        return Err(err(
                                            line,
                                            col,
                                            "subtrahend of a relocatable difference must be in the current section"
                                                .into(),
                                        ));
                                    }
                                    let r_type =
                                        data_reloc_type(*width, true).ok_or_else(|| {
                                            err(
                                                line,
                                                col,
                                                format!(
                                                    "PC-relative data item is unsupported at {} bytes",
                                                    width
                                                ),
                                            )
                                        })?;
                                    let reloc_addend = (field_pos as i64)
                                        .wrapping_sub(rhs as i64)
                                        .wrapping_add(*addend);
                                    relocs.push(LocatedReloc {
                                        line,
                                        col,
                                        reloc: InsnReloc {
                                            offset: reloc_offset(field_pos, line, col)?,
                                            sym: name.clone(),
                                            r_type,
                                            addend: reloc_addend,
                                        },
                                    });
                                    bytes.resize(bytes.len() + *width, 0);
                                } else {
                                    return Err(err(
                                        line,
                                        col,
                                        "current location cannot be the minuend of a cross-section difference"
                                            .into(),
                                    ));
                                }
                            }
                        }
                        pos = checked_layout_add(pos, *width as u64, line, col)?;
                    }
                }
                Item::Zero(n) => {
                    if !is_bss {
                        let new_len = reserve_materialized_bytes(&mut bytes, *n, line, col)?;
                        bytes.resize(new_len, 0);
                    }
                    pos = checked_layout_add(pos, *n, line, col)?;
                }
                Item::Fill { size, byte } => {
                    if !is_bss {
                        let new_len = reserve_materialized_bytes(&mut bytes, *size, line, col)?;
                        bytes.resize(new_len, *byte);
                    }
                    pos = checked_layout_add(pos, *size, line, col)?;
                }
                Item::Branch { kind, label } => {
                    if is_local_branch(label) {
                        let target_item = sb.labels[label];
                        let target = item_offsets.get(target_item).copied().unwrap_or(pos);
                        let here = pos;
                        if long[i] {
                            let (len, mut head) = match kind {
                                BranchKind::Jmp => (5u64, vec![0xe9]),
                                BranchKind::Jcc(cc) => (6u64, vec![0x0f, 0x80 + cc]),
                            };
                            let end = checked_layout_add(here, len, line, col)?;
                            let disp = i128::from(target) - i128::from(end);
                            let encoded = i32::try_from(disp).map_err(|_| {
                                err(
                                    line,
                                    col,
                                    format!("branch displacement to '{}' exceeds i32", label),
                                )
                            })?;
                            if !is_bss {
                                reserve_materialized_bytes(&mut bytes, len, line, col)?;
                                bytes.append(&mut head);
                                bytes.extend_from_slice(&encoded.to_le_bytes());
                            }
                            pos = end;
                        } else {
                            let end = checked_layout_add(here, 2, line, col)?;
                            let disp = i128::from(target) - i128::from(end);
                            let d8 = i8::try_from(disp).map_err(|_| {
                                err(line, col, "relaxed branch displacement exceeds i8".into())
                            })?;
                            if !is_bss {
                                reserve_materialized_bytes(&mut bytes, 2, line, col)?;
                                match kind {
                                    BranchKind::Jmp => bytes.extend_from_slice(&[0xeb, d8 as u8]),
                                    BranchKind::Jcc(cc) => {
                                        bytes.extend_from_slice(&[0x70 + cc, d8 as u8])
                                    }
                                }
                            }
                            pos = end;
                        }
                    } else {
                        let (head, disp_offset): (&[u8], u64) = match kind {
                            BranchKind::Jmp => (&[0xe9], 1),
                            BranchKind::Jcc(cc) => (&[0x0f, 0x80 + cc], 2),
                        };
                        let relocation_pos = checked_layout_add(pos, disp_offset, line, col)?;
                        relocs.push(LocatedReloc {
                            line,
                            col,
                            reloc: InsnReloc {
                                offset: reloc_offset(relocation_pos, line, col)?,
                                sym: label.clone(),
                                r_type: R_X86_64_PLT32,
                                addend: -4,
                            },
                        });
                        if !is_bss {
                            reserve_materialized_bytes(
                                &mut bytes,
                                head.len() as u64 + 4,
                                line,
                                col,
                            )?;
                            bytes.extend_from_slice(head);
                            bytes.extend_from_slice(&0i32.to_le_bytes());
                        }
                        pos = checked_layout_add(pos, head.len() as u64 + 4, line, col)?;
                    }
                }
                Item::Align {
                    pow,
                    fill,
                    max_skip,
                } => {
                    let pad = align_pad(pos, *pow, *max_skip, line, col)?;
                    let section_end = checked_layout_add(pos, pad, line, col)?;
                    if !is_bss {
                        let start = bytes.len();
                        let end = reserve_materialized_bytes(&mut bytes, pad, line, col)?;
                        let len = end - start;
                        if let Some(byte) = fill {
                            bytes.resize(end, *byte);
                        } else if is_text {
                            fill_nops(&mut bytes, len);
                            if start != end {
                                text_nop_padding.push(start..end);
                            }
                        } else {
                            bytes.resize(end, 0);
                        }
                    }
                    pos = section_end;
                }
                Item::SizeDot(sym) => {
                    size_dot.insert(sym.clone(), pos);
                }
            }
        }
        let section_size = pos;
        // Label -> final offset map.
        laid.push(Laid {
            name: name.clone(),
            size: section_size,
            size_dot,
            bytes,
            relocs,
            labels: final_labels,
            max_align: sb.max_align,
            text_nop_padding,
            sh_type: sb.sh_type,
            sh_flags: sb.sh_flags,
        });
    }

    // ---- Build the ELF model ---------------------------------------
    let mut obj = ObjectFile::new(EM_X86_64, osabi);
    obj.gnu_stack_flags = gnu_stack_flags;
    let mut model_sec_index: HashMap<String, usize> = HashMap::new();
    for l in &mut laid {
        let sh_flags = l.sh_flags;
        let sh_type = l.sh_type;
        let align_default = 1;
        obj.sections.push(Section {
            name: l.name.clone(),
            sh_type,
            sh_flags,
            sh_addralign: l.max_align.max(align_default),
            nobits_size: if sh_type == SHT_NOBITS { l.size } else { 0 },
            data: if sh_type == SHT_NOBITS {
                Vec::new()
            } else {
                std::mem::take(&mut l.bytes)
            },
            relas: Vec::new(),
        });
        model_sec_index.insert(l.name.clone(), obj.sections.len() - 1);
    }

    // .local + .comm pairs allocate in .bss (gas behavior); plain
    // .comm becomes a COMMON symbol.
    let mut local_bss: HashMap<String, (u64, u64)> = HashMap::new(); // sym -> (offset, size)
    {
        let mut bss_needed = false;
        for (sym, _, _, _, _) in &commons {
            if syminfo.get(sym).is_some_and(|i| i.local) {
                bss_needed = true;
            }
        }
        if bss_needed && !model_sec_index.contains_key(".bss") {
            obj.sections.push(Section::bss());
            model_sec_index.insert(".bss".into(), obj.sections.len() - 1);
        }
        for (sym, size, align, line, col) in &commons {
            if syminfo.get(sym).is_some_and(|i| i.local) {
                // Local COMMON storage is allocated directly in `.bss`, whose
                // omitted alignment default is one byte. This differs from
                // the size-derived default carried by global SHN_COMMON.
                let align = align.unwrap_or(1);
                if !align.is_power_of_two() {
                    return Err(err(
                        *line,
                        *col,
                        format!(
                            "local COMMON '{}' alignment {} is not a power of two",
                            sym, align
                        ),
                    ));
                }
                let bss = &mut obj.sections[model_sec_index[".bss"]];
                let off = checked_align_up(bss.nobits_size, align).ok_or_else(|| {
                    err(*line, *col, "local COMMON alignment overflows u64".into())
                })?;
                bss.nobits_size = off
                    .checked_add(*size)
                    .ok_or_else(|| err(*line, *col, "local COMMON size overflows u64".into()))?;
                bss.sh_addralign = bss.sh_addralign.max(align);
                local_bss.insert(sym.clone(), (off, *size));
            }
        }
    }

    // Symbols: unexported file-local labels (.L*) stay out of the symtab;
    // explicitly global or weak definitions remain visible. All other
    // defined labels carry their recorded binding/type/size. Section symbols
    // are synthesized on demand for local-label relocations.
    let mut model_sym_index: HashMap<String, usize> = HashMap::new();
    let mut section_sym: HashMap<usize, usize> = HashMap::new();
    let symbol_size = |symbol: &str,
                       info: &SymInfo,
                       defined_section: Option<usize>|
     -> Result<u64, AsmX86Error> {
        match &info.size {
            None => Ok(0),
            Some(SizeArg::Const(size)) => Ok(*size),
            Some(SizeArg::DotMinus(base)) => {
                let size_section = info.size_section.ok_or_else(|| {
                    err(
                        info.size_line,
                        info.size_col,
                        format!(".size {}: directive has no section", symbol),
                    )
                })?;
                if defined_section.is_some_and(|section| section != size_section) {
                    return Err(err(
                        info.size_line,
                        info.size_col,
                        format!(".size {}: directive not in the symbol's section", symbol),
                    ));
                }
                let section = &laid[size_section];
                let dot = section.size_dot.get(symbol).copied().ok_or_else(|| {
                    err(
                        info.size_line,
                        info.size_col,
                        format!(".size {}: directive has no recorded position", symbol),
                    )
                })?;
                let start = section.labels.get(base).copied().ok_or_else(|| {
                    err(
                        info.size_line,
                        info.size_col,
                        format!(
                            ".size {}: base '{}' not defined in this section",
                            symbol, base
                        ),
                    )
                })?;
                Ok(dot.wrapping_sub(start))
            }
        }
    };

    for (li, l) in laid.iter().enumerate() {
        // Deterministic: label_order from the build.
        for label in secs[li].1.label_order.iter() {
            let info = syminfo.get(label).cloned().unwrap_or_default();
            // Validate metadata even when the temporary itself is omitted
            // from the symbol table.
            let size = symbol_size(label, &info, Some(li))?;
            if label.starts_with(".L") && !info.globl && !info.weak {
                continue;
            }
            let off = l.labels[label];
            let bind = if info.weak {
                STB_WEAK
            } else if info.globl {
                STB_GLOBAL
            } else {
                STB_LOCAL
            };
            model_sym_index.insert(label.clone(), obj.symbols.len());
            obj.symbols.push(Symbol {
                name: label.clone(),
                bind,
                typ: info.typ.unwrap_or(STT_NOTYPE),
                vis: STV_DEFAULT,
                place: SymbolPlace::Section(model_sec_index[&l.name]),
                value: off,
                size,
            });
        }
    }
    // Local .comm allocations.
    for located in &stmts {
        let sym = match &located.stmt {
            Stmt::Directive(Directive::Globl(sym))
            | Stmt::Directive(Directive::Local(sym))
            | Stmt::Directive(Directive::Weak(sym)) => sym,
            Stmt::Directive(Directive::Type { sym, .. })
            | Stmt::Directive(Directive::Size { sym, .. })
            | Stmt::Directive(Directive::Comm { sym, .. }) => sym,
            _ => continue,
        };
        let Some(&(off, size)) = local_bss.get(sym) else {
            continue;
        };
        if model_sym_index.contains_key(sym) {
            continue;
        }
        let info = syminfo.get(sym).cloned().unwrap_or_default();
        model_sym_index.insert(sym.clone(), obj.symbols.len());
        obj.symbols.push(Symbol {
            name: sym.clone(),
            bind: STB_LOCAL,
            typ: info.typ.unwrap_or(STT_OBJECT),
            vis: STV_DEFAULT,
            place: SymbolPlace::Section(model_sec_index[".bss"]),
            value: off,
            size,
        });
    }
    // Global commons.
    for (sym, size, align, _, _) in &commons {
        if local_bss.contains_key(sym) {
            continue;
        }
        let align = align.unwrap_or_else(|| default_common_alignment(*size));
        if let Some(&symbol_index) = model_sym_index.get(sym) {
            let symbol = &mut obj.symbols[symbol_index];
            symbol.value = symbol.value.max(align);
            if symbol.size == 0 {
                symbol.size = *size;
            }
            continue;
        }
        model_sym_index.insert(sym.clone(), obj.symbols.len());
        obj.symbols.push(Symbol {
            name: sym.clone(),
            bind: STB_GLOBAL,
            typ: STT_OBJECT,
            vis: STV_DEFAULT,
            place: SymbolPlace::Common,
            value: align,
            size: *size,
        });
    }

    // GNU as retains strong undefined declarations even when no relocation
    // references them. Walk the parsed directives to preserve source order;
    // an unreferenced weak declaration is intentionally omitted.
    for located in &stmts {
        let symbol = match &located.stmt {
            Stmt::Directive(Directive::Globl(symbol))
            | Stmt::Directive(Directive::Extern(symbol))
            | Stmt::Directive(Directive::Local(symbol))
            | Stmt::Directive(Directive::Weak(symbol)) => symbol,
            Stmt::Directive(Directive::Type { sym, .. })
            | Stmt::Directive(Directive::Size { sym, .. }) => sym,
            _ => continue,
        };
        // A defined label either already has a symbol-table entry or is an
        // intentionally elided local temporary. Neither case is undefined.
        if model_sym_index.contains_key(symbol) || label_section.contains_key(symbol) {
            continue;
        }
        let info = &syminfo[symbol];
        if info.weak || !(info.globl || info.typ.is_some() || info.size.is_some()) {
            continue;
        }
        let size = symbol_size(symbol, info, None)?;
        model_sym_index.insert(symbol.clone(), obj.symbols.len());
        obj.symbols.push(Symbol {
            name: symbol.clone(),
            bind: STB_GLOBAL,
            typ: info.typ.unwrap_or(STT_NOTYPE),
            vis: STV_DEFAULT,
            place: SymbolPlace::Undef,
            value: 0,
            size,
        });
    }

    if !file_symbols.is_empty() {
        // An STT_FILE entry begins a group of subsequent local symbols. GNU as
        // preserves those creation boundaries, except that the first FILE is
        // promoted ahead of locals created before any `.file`. Keep globals in
        // their existing deterministic order; the ELF writer partitions them
        // after all locals and remaps relocation indexes later.
        let existing_symbols = std::mem::take(&mut obj.symbols);
        let mut local_symbols = Vec::new();
        let mut nonlocal_symbols = Vec::new();
        for (serial, symbol) in existing_symbols.into_iter().enumerate() {
            if symbol.bind == STB_LOCAL {
                let creation_order = symbol_creation_order
                    .get(&symbol.name)
                    .copied()
                    .unwrap_or(usize::MAX);
                local_symbols.push((creation_order, serial, None, symbol));
            } else {
                nonlocal_symbols.push(symbol);
            }
        }
        let serial_base = local_symbols.len();
        for (file_index, (creation_order, name)) in file_symbols.into_iter().enumerate() {
            local_symbols.push((
                creation_order,
                serial_base + file_index,
                Some(file_index),
                Symbol {
                    name,
                    bind: STB_LOCAL,
                    typ: STT_FILE,
                    vis: STV_DEFAULT,
                    place: SymbolPlace::Abs,
                    value: 0,
                    size: 0,
                },
            ));
        }
        local_symbols.sort_by_key(|(creation_order, serial, _, _)| (*creation_order, *serial));
        let first_file = local_symbols
            .iter()
            .position(|(_, _, file_index, _)| *file_index == Some(0))
            .expect("non-empty file symbol list contains its first entry");
        let first_file = local_symbols.remove(first_file);
        local_symbols.insert(0, first_file);

        obj.symbols
            .extend(local_symbols.into_iter().map(|(_, _, _, symbol)| symbol));
        obj.symbols.extend(nonlocal_symbols);
        model_sym_index.clear();
        for (symbol_index, symbol) in obj.symbols.iter().enumerate() {
            if symbol.typ != STT_FILE {
                model_sym_index.insert(symbol.name.clone(), symbol_index);
            }
        }
    }

    // Relocations. gas's rules for defined LOCAL targets (.L*,
    // non-.globl labels, .local commons): a PC-relative fixup whose
    // target lives in the same section resolves at assembly time and
    // emits no relocation; a cross-section one becomes a relocation
    // against the target section's STT_SECTION symbol with the
    // offset folded into the addend (PLT32 reduces to PC32 — a
    // section symbol can't be preempted). Global/weak targets always
    // keep their symbol relocation.
    for l in &laid {
        let sec_idx = model_sec_index[&l.name];
        for located in &l.relocs {
            let line = located.line;
            let col = located.col;
            let r = &located.reloc;
            // (model section, offset, local?) when defined here.
            let target = if let Some(&tsec) = label_section.get(&r.sym) {
                let tmodel = model_sec_index[&secs[tsec].0];
                let off = laid[tsec].labels[&r.sym];
                let local = !syminfo.get(&r.sym).is_some_and(|i| i.globl || i.weak);
                Some((tmodel, off, local))
            } else {
                local_bss
                    .get(&r.sym)
                    .map(|&(off, _)| (model_sec_index[".bss"], off, true))
            };
            let (sym_idx, r_type, addend) = match target {
                Some((tmodel, off, true)) => {
                    let pc_rel = r.r_type == R_X86_64_PC32 || r.r_type == R_X86_64_PLT32;
                    if pc_rel && tmodel == sec_idx {
                        // Same-section local PC-rel: patch in place.
                        let disp = i128::from(off) + i128::from(r.addend) - i128::from(r.offset);
                        let d = i32::try_from(disp).map_err(|_| {
                            err(
                                line,
                                col,
                                format!("displacement to '{}' overflows i32", r.sym),
                            )
                        })?;
                        let start = r.offset as usize;
                        if start + 4 > obj.sections[sec_idx].data.len() {
                            return Err(err(
                                line,
                                col,
                                format!(
                                    "cannot resolve PC-relative reference to '{}' inside NOBITS section '{}'",
                                    r.sym, obj.sections[sec_idx].name
                                ),
                            ));
                        }
                        obj.sections[sec_idx].data[start..start + 4]
                            .copy_from_slice(&d.to_le_bytes());
                        continue;
                    }
                    let ss = *section_sym.entry(tmodel).or_insert_with(|| {
                        obj.symbols.push(Symbol {
                            name: obj.sections[tmodel].name.clone(),
                            bind: STB_LOCAL,
                            typ: STT_SECTION,
                            vis: STV_DEFAULT,
                            place: SymbolPlace::Section(tmodel),
                            value: 0,
                            size: 0,
                        });
                        obj.symbols.len() - 1
                    });
                    let rt = if r.r_type == R_X86_64_PLT32 {
                        R_X86_64_PC32
                    } else {
                        r.r_type
                    };
                    // ELF64 RELA addends are signed, but relocation arithmetic
                    // preserves the section offset modulo 2^64. GNU as emits
                    // the same two's-complement representation above i64::MAX.
                    (ss, rt, (off as i64).wrapping_add(r.addend))
                }
                Some((_, _, false)) => {
                    let sym_idx = model_sym_index.get(&r.sym).copied().ok_or_else(|| {
                        err(
                            line,
                            col,
                            format!("missing symbol table entry for '{}'", r.sym),
                        )
                    })?;
                    (sym_idx, r.r_type, r.addend)
                }
                None => {
                    if let Some(&idx) = model_sym_index.get(&r.sym) {
                        // Defined elsewhere in the model (e.g. a
                        // global COMMON symbol).
                        (idx, r.r_type, r.addend)
                    } else {
                        // Undefined external.
                        let info = syminfo.get(&r.sym).cloned().unwrap_or_default();
                        let size = symbol_size(&r.sym, &info, None)?;
                        let idx = obj.symbols.len();
                        model_sym_index.insert(r.sym.clone(), idx);
                        obj.symbols.push(Symbol {
                            name: r.sym.clone(),
                            bind: if info.weak { STB_WEAK } else { STB_GLOBAL },
                            typ: info.typ.unwrap_or(STT_NOTYPE),
                            vis: STV_DEFAULT,
                            place: SymbolPlace::Undef,
                            value: 0,
                            size,
                        });
                        (idx, r.r_type, r.addend)
                    }
                }
            };
            if obj.sections[sec_idx].sh_type == SHT_NOBITS {
                return Err(err(
                    line,
                    col,
                    format!(
                        "cannot emit relocation to '{}' inside NOBITS section '{}'",
                        r.sym, obj.sections[sec_idx].name
                    ),
                ));
            }
            obj.sections[sec_idx].relas.push(Rela {
                offset: r.offset as u64,
                symbol: sym_idx,
                r_type,
                addend,
            });
        }
    }

    elf::validate(&obj).map_err(|e| AsmError::new(e.to_string()))?;
    let text_nop_padding = laid
        .iter()
        .find(|section| section.name == ".text")
        .map(|section| section.text_nop_padding.clone())
        .unwrap_or_default();
    Ok(X86Assembly {
        object: obj,
        text_nop_padding,
    })
}

fn default_section_metadata(name: &str) -> (u32, u64) {
    match name {
        ".text" => (SHT_PROGBITS, SHF_ALLOC | SHF_EXECINSTR),
        ".data" => (SHT_PROGBITS, SHF_ALLOC | SHF_WRITE),
        ".bss" => (SHT_NOBITS, SHF_ALLOC | SHF_WRITE),
        ".rodata" => (SHT_PROGBITS, SHF_ALLOC),
        _ => (SHT_PROGBITS, 0),
    }
}

fn parse_section_flags(flags: &str) -> Result<u64, String> {
    let mut bits = 0;
    for flag in flags.chars() {
        bits |= match flag {
            'a' => SHF_ALLOC,
            'w' => SHF_WRITE,
            'x' => SHF_EXECINSTR,
            other => return Err(format!("unsupported ELF section flag '{}'", other)),
        };
    }
    Ok(bits)
}

fn data_reloc_type(width: usize, pc_relative: bool) -> Option<u32> {
    match (width, pc_relative) {
        (4, false) => Some(R_X86_64_32),
        (8, false) => Some(R_X86_64_64),
        (4, true) => Some(R_X86_64_PC32),
        (8, true) => Some(R_X86_64_PC64),
        _ => None,
    }
}

fn checked_layout_add(pos: u64, size: u64, line: u32, col: u32) -> Result<u64, AsmX86Error> {
    pos.checked_add(size)
        .ok_or_else(|| AsmError::at(line, col, "section layout size overflows u64".into()))
}

fn reserve_materialized_bytes(
    bytes: &mut Vec<u8>,
    additional: u64,
    line: u32,
    col: u32,
) -> Result<usize, AsmX86Error> {
    let current = bytes.len();
    let too_large = || {
        AsmError::at(
            line,
            col,
            format!(
                "initialized section is too large to materialize ({} + {} bytes)",
                current, additional
            ),
        )
    };
    let additional = usize::try_from(additional).map_err(|_| too_large())?;
    let end = current.checked_add(additional).ok_or_else(&too_large)?;
    bytes.try_reserve(additional).map_err(|_| too_large())?;
    Ok(end)
}

fn checked_align_up(value: u64, alignment: u64) -> Option<u64> {
    let remainder = value % alignment;
    if remainder == 0 {
        Some(value)
    } else {
        value.checked_add(alignment - remainder)
    }
}

fn default_common_alignment(size: u64) -> u64 {
    size.clamp(1, 16).next_power_of_two()
}

/// Padding a `.p2align pow` inserts at `pos`, honoring a `.p2align N,,M`
/// max-skip: gas emits no padding at all when it would exceed `max_skip`.
fn align_pad(
    pos: u64,
    pow: u32,
    max_skip: Option<u64>,
    line: u32,
    col: u32,
) -> Result<u64, AsmX86Error> {
    let alignment = 1u64
        .checked_shl(pow)
        .ok_or_else(|| AsmError::at(line, col, "section alignment exceeds u64".into()))?;
    let remainder = pos & (alignment - 1);
    let pad = if remainder == 0 {
        0
    } else {
        alignment - remainder
    };
    Ok(if max_skip.is_some_and(|m| pad > m) {
        0
    } else {
        pad
    })
}

/// gas-style multi-byte NOP fill for text alignment. Single NOPs run
/// up to 11 bytes (66/2e-prefixed nopw forms); longer fills emit the
/// REMAINDER-sized NOP first, then 11-byte NOPs — the order gas 2.44
/// actually produces (measured for n = 12, 13, 14, 25; the cgfried
/// object differential caught the longest-first divergence).
fn fill_nops(out: &mut Vec<u8>, mut n: usize) {
    const NOPS: [&[u8]; 11] = [
        &[0x90],
        &[0x66, 0x90],
        &[0x0f, 0x1f, 0x00],
        &[0x0f, 0x1f, 0x40, 0x00],
        &[0x0f, 0x1f, 0x44, 0x00, 0x00],
        &[0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00],
        &[0x0f, 0x1f, 0x80, 0x00, 0x00, 0x00, 0x00],
        &[0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
        &[0x66, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
        &[0x66, 0x2e, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
        &[
            0x66, 0x66, 0x2e, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
    ];
    if n > 11 && !n.is_multiple_of(11) {
        out.extend_from_slice(NOPS[n % 11 - 1]);
        n -= n % 11;
    }
    while n > 0 {
        let take = n.min(11);
        out.extend_from_slice(NOPS[take - 1]);
        n -= take;
    }
}
