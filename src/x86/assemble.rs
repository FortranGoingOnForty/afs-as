//! x86_64 two-pass assembler (x14): parsed statements → elf::ObjectFile.
//!
//! Pass 1 builds per-section item streams (encoded bytes, relaxable
//! branches, alignment marks, data) and the symbol bookkeeping
//! (.globl/.local/.weak/.type/.size/.comm). The relaxation pass then
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

use super::super::elf::{
    self, reloc::x86_64::*, ObjectFile, Rela, Section, Symbol, SymbolPlace, EM_X86_64, SHF_ALLOC,
    SHF_EXECINSTR, SHF_WRITE, SHT_NOBITS, SHT_PROGBITS, STB_GLOBAL, STB_LOCAL, STB_WEAK, STT_FUNC,
    STT_NOTYPE, STT_OBJECT, STT_SECTION, STV_DEFAULT,
};
use super::encode::{encode, InsnReloc};
use super::parse::{parse, DataItem, Directive, SizeArg, Stmt, SymKind};
use super::Operand;

#[derive(Debug)]
pub struct AsmX86Error {
    pub line: u32,
    pub msg: String,
}

impl std::fmt::Display for AsmX86Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchKind {
    Jmp,
    Jcc(u8),
}

#[derive(Debug)]
enum Item {
    /// Encoded bytes with item-relative relocations.
    Bytes(Vec<u8>, Vec<InsnReloc>),
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

#[derive(Debug, Default)]
struct SecBuild {
    items: Vec<Item>,
    /// label -> item index it precedes.
    labels: HashMap<String, usize>,
    label_order: Vec<String>,
    max_align: u64,
}

#[derive(Debug, Default, Clone)]
struct SymInfo {
    globl: bool,
    weak: bool,
    local: bool,
    typ: Option<u8>,
    size: Option<SizeArg>,
    size_line: u32,
    size_section: Option<usize>,
}

pub fn assemble_x86(src: &str, osabi: u8) -> Result<ObjectFile, AsmX86Error> {
    let stmts = parse(src).map_err(|e| AsmX86Error {
        line: e.line,
        msg: e.render_with_source(src),
    })?;

    // ---- Pass 1: build sections -----------------------------------
    let mut secs: Vec<(String, SecBuild)> = Vec::new();
    let mut sec_index: HashMap<String, usize> = HashMap::new();
    let mut current: usize = usize::MAX;
    let mut syminfo: HashMap<String, SymInfo> = HashMap::new();
    let mut gnu_stack_flags: Option<u64> = None;
    // (sym, size, align, line)
    let mut commons: Vec<(String, u64, u64, u32)> = Vec::new();
    let mut common_names: HashSet<String> = HashSet::new();
    // label -> section index (for cross-section checks + reloc targets)
    let mut label_section: HashMap<String, usize> = HashMap::new();

    let ensure_sec = |name: &str,
                      secs: &mut Vec<(String, SecBuild)>,
                      sec_index: &mut HashMap<String, usize>|
     -> usize {
        if let Some(&i) = sec_index.get(name) {
            return i;
        }
        secs.push((name.to_string(), SecBuild::default()));
        sec_index.insert(name.to_string(), secs.len() - 1);
        secs.len() - 1
    };

    let err = |line: u32, msg: String| AsmX86Error { line, msg };

    // gas always creates .text/.data/.bss even when empty; match it so
    // objects diff clean against the system assembler.
    ensure_sec(".text", &mut secs, &mut sec_index);
    ensure_sec(".data", &mut secs, &mut sec_index);
    ensure_sec(".bss", &mut secs, &mut sec_index);

    for located in &stmts {
        let line = located.line;
        match &located.stmt {
            Stmt::Label(name) => {
                if common_names.contains(name) {
                    return Err(err(line, format!("symbol '{}' is already defined", name)));
                }
                if current == usize::MAX {
                    current = ensure_sec(".text", &mut secs, &mut sec_index);
                }
                let sb = &mut secs[current].1;
                if label_section.insert(name.clone(), current).is_some() {
                    return Err(err(line, format!("duplicate label '{}'", name)));
                }
                sb.labels.insert(name.clone(), sb.items.len());
                sb.label_order.push(name.clone());
            }
            Stmt::Directive(d) => match d {
                Directive::Text => current = ensure_sec(".text", &mut secs, &mut sec_index),
                Directive::Data => current = ensure_sec(".data", &mut secs, &mut sec_index),
                Directive::Bss => current = ensure_sec(".bss", &mut secs, &mut sec_index),
                Directive::Section { name } => {
                    current = ensure_sec(name, &mut secs, &mut sec_index)
                }
                Directive::NoteGnuStack { executable } => {
                    gnu_stack_flags.get_or_insert(if *executable { SHF_EXECINSTR } else { 0 });
                }
                Directive::Globl(s) => syminfo.entry(s.clone()).or_default().globl = true,
                Directive::Extern(s) => {
                    syminfo.entry(s.clone()).or_default();
                }
                Directive::Weak(s) => syminfo.entry(s.clone()).or_default().weak = true,
                Directive::Local(s) => syminfo.entry(s.clone()).or_default().local = true,
                Directive::Type { sym, kind } => {
                    syminfo.entry(sym.clone()).or_default().typ = Some(match kind {
                        SymKind::Function => STT_FUNC,
                        SymKind::Object => STT_OBJECT,
                    })
                }
                Directive::Size { sym, arg } => {
                    if matches!(arg, SizeArg::DotMinus(_)) && current == usize::MAX {
                        return Err(err(
                            line,
                            format!(".size {}, .-... before any section", sym),
                        ));
                    }
                    let e = syminfo.entry(sym.clone()).or_default();
                    e.size = Some(arg.clone());
                    e.size_line = line;
                    e.size_section = matches!(arg, SizeArg::DotMinus(_)).then_some(current);
                    if matches!(arg, SizeArg::DotMinus(_)) {
                        secs[current].1.items.push(Item::SizeDot(sym.clone()));
                    }
                }
                Directive::Comm { sym, size, align } => {
                    if label_section.contains_key(sym) {
                        return Err(err(line, format!("symbol '{}' is already defined", sym)));
                    }
                    common_names.insert(sym.clone());
                    commons.push((sym.clone(), *size, *align, line))
                }
                Directive::File(_) => {}
                Directive::P2Align {
                    pow,
                    fill,
                    max_skip,
                } => {
                    if current == usize::MAX {
                        current = ensure_sec(".text", &mut secs, &mut sec_index);
                    }
                    let sb = &mut secs[current].1;
                    sb.max_align = sb.max_align.max(1u64 << pow);
                    sb.items.push(Item::Align {
                        pow: *pow,
                        fill: *fill,
                        max_skip: *max_skip,
                    });
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
                        current = ensure_sec(".text", &mut secs, &mut sec_index);
                    }
                    let mut bytes = Vec::new();
                    let mut relocs = Vec::new();
                    for it in items {
                        match it {
                            DataItem::Num(v) => bytes.extend_from_slice(&v.to_le_bytes()[..width]),
                            DataItem::Sym { name, addend } => {
                                if width != 8 {
                                    return Err(err(
                                        line,
                                        format!(
                                            "symbolic data item '{}' only supported in .quad",
                                            name
                                        ),
                                    ));
                                }
                                relocs.push(InsnReloc {
                                    offset: bytes.len() as u32,
                                    sym: name.clone(),
                                    r_type: R_X86_64_64,
                                    addend: *addend,
                                });
                                bytes.extend_from_slice(&0u64.to_le_bytes());
                            }
                        }
                    }
                    if secs[current].0 == ".bss"
                        && (bytes.iter().any(|&b| b != 0) || !relocs.is_empty())
                    {
                        return Err(err(
                            line,
                            "attempt to store non-zero value in section `.bss'".into(),
                        ));
                    }
                    if secs[current].0 == ".bss" {
                        secs[current].1.items.push(Item::Zero(bytes.len() as u64));
                    } else {
                        secs[current].1.items.push(Item::Bytes(bytes, relocs));
                    }
                }
                Directive::Ascii(b) => {
                    if current == usize::MAX {
                        current = ensure_sec(".text", &mut secs, &mut sec_index);
                    }
                    if secs[current].0 == ".bss" && b.iter().any(|&x| x != 0) {
                        return Err(err(
                            line,
                            "attempt to store non-empty string in section `.bss'".into(),
                        ));
                    }
                    if secs[current].0 == ".bss" {
                        secs[current].1.items.push(Item::Zero(b.len() as u64));
                    } else {
                        secs[current].1.items.push(Item::Bytes(b.clone(), vec![]));
                    }
                }
                Directive::Asciz(b) => {
                    if current == usize::MAX {
                        current = ensure_sec(".text", &mut secs, &mut sec_index);
                    }
                    if secs[current].0 == ".bss" && b.iter().any(|&x| x != 0) {
                        return Err(err(
                            line,
                            "attempt to store non-empty string in section `.bss'".into(),
                        ));
                    }
                    if secs[current].0 == ".bss" {
                        secs[current].1.items.push(Item::Zero(b.len() as u64 + 1));
                    } else {
                        let mut v = b.clone();
                        v.push(0);
                        secs[current].1.items.push(Item::Bytes(v, vec![]));
                    }
                }
                Directive::Space { size, fill } => {
                    if current == usize::MAX {
                        current = ensure_sec(".text", &mut secs, &mut sec_index);
                    }
                    if secs[current].0 == ".bss" && *fill != 0 {
                        return Err(err(
                            line,
                            "attempt to store non-zero value in section `.bss'".into(),
                        ));
                    }
                    secs[current].1.items.push(Item::Fill {
                        size: *size,
                        byte: *fill,
                    });
                }
                Directive::Zero(n) => {
                    if current == usize::MAX {
                        current = ensure_sec(".text", &mut secs, &mut sec_index);
                    }
                    if secs[current].0 == ".bss" {
                        secs[current].1.items.push(Item::Zero(*n));
                    } else {
                        secs[current]
                            .1
                            .items
                            .push(Item::Bytes(vec![0u8; *n as usize], vec![]));
                    }
                }
            },
            Stmt::Insn { mnemonic, operands } => {
                if current == usize::MAX {
                    current = ensure_sec(".text", &mut secs, &mut sec_index);
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
                    secs[current].1.items.push(Item::Branch { kind, label });
                    continue;
                }
                let enc = encode(mnemonic, operands)
                    .map_err(|e| err(line, format!("{}: {}", mnemonic, e)))?;
                if enc.label_fix.is_some() {
                    return Err(err(line, format!("unexpected label fix for {}", mnemonic)));
                }
                secs[current]
                    .1
                    .items
                    .push(Item::Bytes(enc.bytes, enc.reloc.into_iter().collect()));
            }
        }
    }

    // ---- Relaxation + layout per section ---------------------------
    struct Laid {
        name: String,
        size: u64,
        bytes: Vec<u8>,
        relocs: Vec<InsnReloc>, // section-relative offsets
        labels: HashMap<String, u64>,
        /// sym -> dot position at its `.size sym, .-base` directive.
        size_dot: HashMap<String, u64>,
        max_align: u64,
    }
    let mut laid: Vec<Laid> = Vec::new();

    for (name, sb) in &secs {
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
            for (i, item) in sb.items.iter().enumerate() {
                offsets.push(pos);
                pos += match item {
                    Item::Bytes(b, _) => b.len() as u64,
                    Item::Zero(n) => *n,
                    Item::Fill { size, .. } => *size,
                    Item::Branch { kind, label } => {
                        if is_local_branch(label) {
                            branch_len(*kind, long[i])
                        } else {
                            branch_len(*kind, true)
                        }
                    }
                    Item::Align { pow, max_skip, .. } => align_pad(pos, *pow, *max_skip),
                    Item::SizeDot(_) => 0,
                };
            }
            offsets.push(pos);
            // Grow any short branch whose displacement overflows i8.
            let mut grew = false;
            for (i, item) in sb.items.iter().enumerate() {
                if let Item::Branch { kind, label } = item {
                    if long[i] || !is_local_branch(label) {
                        continue;
                    }
                    let target_item = sb.labels[label];
                    let target_off = offsets[target_item];
                    let end = offsets[i] + branch_len(*kind, false);
                    let disp = target_off as i64 - end as i64;
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
        let mut relocs: Vec<InsnReloc> = Vec::new();
        let mut item_offsets: Vec<u64> = Vec::with_capacity(sb.items.len());
        // First recompute final offsets (same walk as above).
        {
            let mut pos = 0u64;
            for (i, item) in sb.items.iter().enumerate() {
                item_offsets.push(pos);
                pos += match item {
                    Item::Bytes(b, _) => b.len() as u64,
                    Item::Zero(n) => *n,
                    Item::Fill { size, .. } => *size,
                    Item::Branch { kind, label } => {
                        if is_local_branch(label) {
                            branch_len(*kind, long[i])
                        } else {
                            branch_len(*kind, true)
                        }
                    }
                    Item::Align { pow, max_skip, .. } => align_pad(pos, *pow, *max_skip),
                    Item::SizeDot(_) => 0,
                };
            }
            item_offsets.push(pos);
        }
        let mut size_dot: HashMap<String, u64> = HashMap::new();
        let is_bss = name == ".bss";
        let mut pos = 0u64;
        let reloc_offset = |offset: u64| -> Result<u32, AsmX86Error> {
            u32::try_from(offset)
                .map_err(|_| err(0, format!("relocation offset {} exceeds u32", offset)))
        };
        for (i, item) in sb.items.iter().enumerate() {
            match item {
                Item::Bytes(b, rs) => {
                    let base = reloc_offset(pos)?;
                    for r in rs {
                        let offset = base.checked_add(r.offset).ok_or_else(|| {
                            err(
                                0,
                                format!("relocation offset {} + {} overflows u32", base, r.offset),
                            )
                        })?;
                        relocs.push(InsnReloc {
                            offset,
                            ..r.clone()
                        });
                    }
                    if !is_bss {
                        bytes.extend_from_slice(b);
                    }
                    pos += b.len() as u64;
                }
                Item::Zero(n) => {
                    if !is_bss {
                        let len = usize::try_from(*n)
                            .map_err(|_| err(0, format!("zero fill too large: {}", n)))?;
                        let new_len = bytes
                            .len()
                            .checked_add(len)
                            .ok_or_else(|| err(0, format!("zero fill too large: {}", n)))?;
                        bytes.resize(new_len, 0);
                    }
                    pos += *n;
                }
                Item::Fill { size, byte } => {
                    if !is_bss {
                        let len = usize::try_from(*size)
                            .map_err(|_| err(0, format!("space fill too large: {}", size)))?;
                        let new_len = bytes
                            .len()
                            .checked_add(len)
                            .ok_or_else(|| err(0, format!("space fill too large: {}", size)))?;
                        bytes.resize(new_len, *byte);
                    }
                    pos += *size;
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
                            let disp = target as i64 - (here + len) as i64;
                            if !is_bss {
                                bytes.append(&mut head);
                                bytes.extend_from_slice(&(disp as i32).to_le_bytes());
                            }
                            pos += len;
                        } else {
                            let disp = target as i64 - (here + 2) as i64;
                            let d8 = i8::try_from(disp).expect("relaxation fixed-point violated");
                            if !is_bss {
                                match kind {
                                    BranchKind::Jmp => bytes.extend_from_slice(&[0xeb, d8 as u8]),
                                    BranchKind::Jcc(cc) => {
                                        bytes.extend_from_slice(&[0x70 + cc, d8 as u8])
                                    }
                                }
                            }
                            pos += 2;
                        }
                    } else {
                        let (head, disp_offset): (&[u8], u64) = match kind {
                            BranchKind::Jmp => (&[0xe9], 1),
                            BranchKind::Jcc(cc) => (&[0x0f, 0x80 + cc], 2),
                        };
                        relocs.push(InsnReloc {
                            offset: reloc_offset(pos + disp_offset)?,
                            sym: label.clone(),
                            r_type: R_X86_64_PLT32,
                            addend: -4,
                        });
                        if !is_bss {
                            bytes.extend_from_slice(head);
                            bytes.extend_from_slice(&0i32.to_le_bytes());
                        }
                        pos += head.len() as u64 + 4;
                    }
                }
                Item::Align {
                    pow,
                    fill,
                    max_skip,
                } => {
                    let pad = align_pad(pos, *pow, *max_skip);
                    if !is_bss {
                        let len = usize::try_from(pad)
                            .map_err(|_| err(0, format!("alignment fill too large: {}", pad)))?;
                        let here = bytes.len();
                        if let Some(byte) = fill {
                            bytes.resize(here + len, *byte);
                        } else if is_text {
                            fill_nops(&mut bytes, len);
                        } else {
                            bytes.resize(here + len, 0);
                        }
                    }
                    pos += pad;
                }
                Item::SizeDot(sym) => {
                    size_dot.insert(sym.clone(), pos);
                }
            }
        }
        let section_size = pos;
        // Label -> final offset map.
        let mut labels = HashMap::new();
        for (l, &item_idx) in &sb.labels {
            let off = item_offsets.get(item_idx).copied().unwrap_or(section_size);
            labels.insert(l.clone(), off);
        }
        laid.push(Laid {
            name: name.clone(),
            size: section_size,
            size_dot,
            bytes,
            relocs,
            labels,
            max_align: sb.max_align,
        });
    }

    // ---- Build the ELF model ---------------------------------------
    let mut obj = ObjectFile::new(EM_X86_64, osabi);
    obj.gnu_stack_flags = gnu_stack_flags;
    let mut model_sec_index: HashMap<String, usize> = HashMap::new();
    for l in &laid {
        let (sh_flags, align_default) = match l.name.as_str() {
            ".text" => (SHF_ALLOC | SHF_EXECINSTR, 1),
            ".data" => (SHF_ALLOC | SHF_WRITE, 1),
            ".bss" => (SHF_ALLOC | SHF_WRITE, 1),
            ".rodata" => (SHF_ALLOC, 1),
            other => {
                return Err(err(0, format!("unsupported section '{}'", other)));
            }
        };
        let sh_type = if l.name == ".bss" {
            SHT_NOBITS
        } else {
            SHT_PROGBITS
        };
        obj.sections.push(Section {
            name: l.name.clone(),
            sh_type,
            sh_flags,
            sh_addralign: l.max_align.max(align_default),
            nobits_size: if sh_type == SHT_NOBITS { l.size } else { 0 },
            data: if sh_type == SHT_NOBITS {
                Vec::new()
            } else {
                l.bytes.clone()
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
        for (sym, _, _, _) in &commons {
            if syminfo.get(sym).is_some_and(|i| i.local) {
                bss_needed = true;
            }
        }
        if bss_needed && !model_sec_index.contains_key(".bss") {
            obj.sections.push(Section::bss());
            model_sec_index.insert(".bss".into(), obj.sections.len() - 1);
        }
        for (sym, size, align, _) in &commons {
            if syminfo.get(sym).is_some_and(|i| i.local) {
                let bss = &mut obj.sections[model_sec_index[".bss"]];
                let off = bss.nobits_size.next_multiple_of((*align).max(1));
                bss.nobits_size = off + size;
                bss.sh_addralign = bss.sh_addralign.max(*align);
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
                        format!(".size {}: directive has no section", symbol),
                    )
                })?;
                if defined_section.is_some_and(|section| section != size_section) {
                    return Err(err(
                        info.size_line,
                        format!(".size {}: directive not in the symbol's section", symbol),
                    ));
                }
                let section = &laid[size_section];
                let dot = section.size_dot.get(symbol).copied().ok_or_else(|| {
                    err(
                        info.size_line,
                        format!(".size {}: directive has no recorded position", symbol),
                    )
                })?;
                let start = section.labels.get(base).copied().ok_or_else(|| {
                    err(
                        info.size_line,
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
            // `.size sym, .-base`: dot recorded at the directive, so
            // padding and local labels after the body don't skew it.
            let size = symbol_size(label, &info, Some(li))?;
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
    for (sym, (off, size)) in &local_bss {
        let info = syminfo.get(sym).cloned().unwrap_or_default();
        model_sym_index.insert(sym.clone(), obj.symbols.len());
        obj.symbols.push(Symbol {
            name: sym.clone(),
            bind: STB_LOCAL,
            typ: info.typ.unwrap_or(STT_OBJECT),
            vis: STV_DEFAULT,
            place: SymbolPlace::Section(model_sec_index[".bss"]),
            value: *off,
            size: *size,
        });
    }
    // Global commons.
    for (sym, size, align, _) in &commons {
        if local_bss.contains_key(sym) {
            continue;
        }
        model_sym_index.insert(sym.clone(), obj.symbols.len());
        obj.symbols.push(Symbol {
            name: sym.clone(),
            bind: STB_GLOBAL,
            typ: STT_OBJECT,
            vis: STV_DEFAULT,
            place: SymbolPlace::Common,
            value: *align,
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
        if model_sym_index.contains_key(symbol) {
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
        for r in &l.relocs {
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
                        let disp = off as i64 + r.addend - r.offset as i64;
                        let d = i32::try_from(disp).map_err(|_| {
                            err(0, format!("displacement to '{}' overflows i32", r.sym))
                        })?;
                        let start = r.offset as usize;
                        if start + 4 > obj.sections[sec_idx].data.len() {
                            return Err(err(
                                0,
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
                    (ss, rt, r.addend + off as i64)
                }
                Some((_, _, false)) => {
                    let sym_idx = model_sym_index.get(&r.sym).copied().ok_or_else(|| {
                        err(0, format!("missing symbol table entry for '{}'", r.sym))
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
            obj.sections[sec_idx].relas.push(Rela {
                offset: r.offset as u64,
                symbol: sym_idx,
                r_type,
                addend,
            });
        }
    }

    elf::validate(&obj).map_err(|e| err(0, e.to_string()))?;
    Ok(obj)
}

/// gas-style multi-byte NOP fill for text alignment. Single NOPs run
/// up to 11 bytes (66/2e-prefixed nopw forms), longer fills go
/// longest-first. Table measured from gas 2.44 output for every fill
/// size 1..=15.
/// Padding a `.p2align pow` inserts at `pos`, honoring a `.p2align N,,M`
/// max-skip: gas emits no padding at all when it would exceed `max_skip`.
fn align_pad(pos: u64, pow: u32, max_skip: Option<u64>) -> u64 {
    let pad = pos.next_multiple_of(1u64 << pow) - pos;
    if max_skip.is_some_and(|m| pad > m) {
        0
    } else {
        pad
    }
}

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
    while n > 0 {
        let take = n.min(11);
        out.extend_from_slice(NOPS[take - 1]);
        n -= take;
    }
}
