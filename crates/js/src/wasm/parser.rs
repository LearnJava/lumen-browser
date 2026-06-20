//! WebAssembly binary decoder (MVP / WASM 1.0 core + a few common post-MVP ops).
//!
//! Decodes a `.wasm` byte image into a [`Module`]: type/import/function/table/
//! memory/global/export/start/element/code/data sections. Function bodies are
//! decoded into a flat [`Instr`] vector with structured-control targets
//! (`Block`/`Loop`/`If` carry the index of their matching `End`/`Else`) so the
//! interpreter never re-scans bytecode at run time.
//!
//! Fixed-width SIMD (`0xFD` prefix) is decoded into dedicated [`Instr`] variants.
//! Anything the decoder does not understand (unknown opcodes, relaxed-SIMD,
//! malformed sections) yields `Err` so `WebAssembly.compile`/`validate` reject
//! cleanly rather than producing a half-decoded module.

use super::value::{FuncType, Limits, ValType};

/// Result of decoding, with a human-readable error for `CompileError`.
pub type DecodeResult<T> = Result<T, String>;

/// Block signature for `block`/`loop`/`if`.
#[derive(Clone, Copy, Debug)]
pub enum BlockType {
    /// No result (`0x40`).
    Empty,
    /// Single result value type.
    Val(ValType),
    /// Reference to a full function type by index (multi-value).
    Func(u32),
}

/// A decoded instruction. Numeric/comparison/conversion ops with no immediate
/// are kept as their raw opcode byte in [`Instr::Num`]; everything carrying an
/// immediate gets a dedicated variant.
#[derive(Clone, Debug)]
pub enum Instr {
    Unreachable,
    Nop,
    /// `block` — `end` is the index just past the matching `End`.
    Block { ty: BlockType, end: usize },
    /// `loop` — `end` is the index just past the matching `End`.
    Loop { ty: BlockType, end: usize },
    /// `if` — `else_` is the index of the matching `Else` (or `end` if none),
    /// `end` is the index just past the matching `End`.
    If { ty: BlockType, else_: usize, end: usize },
    Else,
    End,
    Br(u32),
    BrIf(u32),
    BrTable { targets: Vec<u32>, default: u32 },
    Return,
    Call(u32),
    CallIndirect { type_idx: u32, table_idx: u32 },
    Drop,
    Select,
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(u32),
    GlobalSet(u32),
    /// Memory load. `op` is the raw opcode (0x28..=0x35), `offset` the static
    /// address offset (alignment is parsed but ignored — it is only a hint).
    Load { op: u8, offset: u32 },
    /// Memory store. `op` is the raw opcode (0x36..=0x3E).
    Store { op: u8, offset: u32 },
    MemorySize,
    MemoryGrow,
    I32Const(i32),
    I64Const(i64),
    F32Const(f32),
    F64Const(f64),
    /// Pure numeric/comparison/conversion op identified by its opcode byte.
    Num(u8),
    /// Saturating float→int truncation (`0xFC` sub-opcodes 0..=7).
    TruncSat(u8),
    /// `memory.copy` (`0xFC 10`).
    MemoryCopy,
    /// `memory.fill` (`0xFC 11`).
    MemoryFill,
    RefNull(ValType),
    RefFunc(u32),
    RefIsNull,
    // ── SIMD (`0xFD` prefix) ────────────────────────────────────────────────
    /// `v128.const` — the 16 immediate bytes (little-endian).
    V128Const([u8; 16]),
    /// SIMD memory load: `sub` is the `0xFD` sub-opcode (0..=10, 92, 93),
    /// `offset` the static address offset (alignment hint is ignored).
    V128Load { sub: u32, offset: u32 },
    /// `v128.store` (`0xFD 11`).
    V128Store { offset: u32 },
    /// SIMD load-into-lane (`0xFD` 84..=87): `lane` is the destination lane.
    V128LoadLane { sub: u32, offset: u32, lane: u8 },
    /// SIMD store-from-lane (`0xFD` 88..=91): `lane` is the source lane.
    V128StoreLane { sub: u32, offset: u32, lane: u8 },
    /// `i8x16.shuffle` (`0xFD 13`) — 16 immediate lane indices (0..=31).
    Shuffle([u8; 16]),
    /// `*.extract_lane*` / `*.replace_lane` (`0xFD` 21..=34): `sub` selects the
    /// shape and direction, `lane` the lane index.
    SimdLane { sub: u32, lane: u8 },
    /// Any other SIMD op with no immediate beyond the sub-opcode.
    Simd(u32),
    // ── Threads / atomics (`0xFE` prefix) ───────────────────────────────────
    /// Atomic memory op (`0xFE` sub-opcodes 0x00..=0x02, 0x10..=0x4E): `sub`
    /// selects notify/wait/load/store/rmw/cmpxchg, `offset` the static address
    /// offset (alignment hint is ignored). Executed with single-threaded,
    /// non-blocking semantics (there is only one agent, so every operation is
    /// trivially atomic and `wait`/`notify` never actually block).
    Atomic { sub: u32, offset: u32 },
    /// `atomic.fence` (`0xFE 0x03`) — a no-op under single-threaded semantics.
    AtomicFence,
}

/// What an import binds to.
#[derive(Clone, Debug)]
pub enum ImportKind {
    /// Imported function with the given type index.
    Func(u32),
    /// Imported table.
    Table { elem: ValType, limits: Limits },
    /// Imported memory.
    Memory(Limits),
    /// Imported global.
    Global { ty: ValType, mutable: bool },
}

/// A single import entry.
#[derive(Clone, Debug)]
pub struct Import {
    /// Module name (the `env` in `env.foo`).
    pub module: String,
    /// Field name (the `foo` in `env.foo`).
    pub name: String,
    /// What kind of thing is imported.
    pub kind: ImportKind,
}

/// The export kind tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Func,
    Table,
    Memory,
    Global,
}

/// A single export entry.
#[derive(Clone, Debug)]
pub struct Export {
    /// Exported name.
    pub name: String,
    /// What kind of thing is exported.
    pub kind: ExportKind,
    /// Index into the relevant (import-prefixed) index space.
    pub index: u32,
}

/// A defined global: its type, mutability, and initialiser expression.
#[derive(Clone, Debug)]
pub struct GlobalDef {
    /// Value type.
    pub ty: ValType,
    /// Whether the global is mutable.
    pub mutable: bool,
    /// Constant initialiser expression (decoded instructions, `End`-terminated).
    pub init: Vec<Instr>,
}

/// A decoded function body: extra locals plus its instruction stream.
#[derive(Clone, Debug)]
pub struct FuncBody {
    /// Local variable types (beyond parameters), already expanded from the
    /// run-length-encoded form.
    pub locals: Vec<ValType>,
    /// Instruction stream, terminated by the function-level `End`.
    pub code: Vec<Instr>,
}

/// An active data segment: target memory offset expression + raw bytes.
#[derive(Clone, Debug)]
pub struct DataSegment {
    /// `true` for passive segments (no automatic init).
    pub passive: bool,
    /// Offset initialiser expression (for active segments).
    pub offset: Vec<Instr>,
    /// Raw segment bytes.
    pub bytes: Vec<u8>,
}

/// An active element segment for a table: offset expression + function indices.
#[derive(Clone, Debug)]
pub struct ElemSegment {
    /// `true` for passive/declarative segments.
    pub passive: bool,
    /// Offset initialiser expression (for active segments).
    pub offset: Vec<Instr>,
    /// Function indices placed into the table.
    pub func_indices: Vec<u32>,
}

/// A fully decoded module ready for instantiation.
#[derive(Clone, Debug, Default)]
pub struct Module {
    /// Function type table.
    pub types: Vec<FuncType>,
    /// Imports, in binary order.
    pub imports: Vec<Import>,
    /// Type index for each *locally defined* function (parallel to [`Self::code`]).
    pub funcs: Vec<u32>,
    /// Table definitions (element type + limits).
    pub tables: Vec<(ValType, Limits)>,
    /// Memory definitions (limits, in 64 KiB pages).
    pub memories: Vec<Limits>,
    /// Defined globals.
    pub globals: Vec<GlobalDef>,
    /// Exports.
    pub exports: Vec<Export>,
    /// Start function index, if any.
    pub start: Option<u32>,
    /// Element segments.
    pub elems: Vec<ElemSegment>,
    /// Locally-defined function bodies (parallel to [`Self::funcs`]).
    pub code: Vec<FuncBody>,
    /// Data segments.
    pub data: Vec<DataSegment>,
    /// Number of imported functions (the prefix of the function index space).
    pub num_imported_funcs: u32,
    /// Number of imported globals.
    pub num_imported_globals: u32,
    /// Number of imported memories.
    pub num_imported_memories: u32,
    /// Number of imported tables.
    pub num_imported_tables: u32,
}

impl Module {
    /// Look up the function type for any function index (imported or defined).
    pub fn func_type(&self, func_idx: u32) -> Option<&FuncType> {
        let type_idx = if func_idx < self.num_imported_funcs {
            // Imported function: find the n-th function import.
            let mut seen = 0u32;
            let mut ti = None;
            for imp in &self.imports {
                if let ImportKind::Func(t) = imp.kind {
                    if seen == func_idx {
                        ti = Some(t);
                        break;
                    }
                    seen += 1;
                }
            }
            ti?
        } else {
            *self.funcs.get((func_idx - self.num_imported_funcs) as usize)?
        };
        self.types.get(type_idx as usize)
    }
}

/// Byte cursor over the module image.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn byte(&mut self) -> DecodeResult<u8> {
        let b = *self.data.get(self.pos).ok_or("unexpected end of input")?;
        self.pos += 1;
        Ok(b)
    }

    fn bytes(&mut self, n: usize) -> DecodeResult<&'a [u8]> {
        if self.remaining() < n {
            return Err("unexpected end of input".into());
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// Unsigned LEB128, up to 32 bits.
    fn u32(&mut self) -> DecodeResult<u32> {
        Ok(self.u64()? as u32)
    }

    /// Unsigned LEB128, up to 64 bits.
    fn u64(&mut self) -> DecodeResult<u64> {
        let mut result: u64 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.byte()?;
            if shift >= 64 {
                return Err("LEB128 overflow".into());
            }
            result |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        Ok(result)
    }

    /// Signed LEB128, up to 64 bits (used for i32/i64 consts and blocktypes).
    fn i64(&mut self) -> DecodeResult<i64> {
        let mut result: i64 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.byte()?;
            if shift >= 64 {
                return Err("LEB128 overflow".into());
            }
            result |= ((b & 0x7F) as i64) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                if shift < 64 && (b & 0x40) != 0 {
                    result |= -1i64 << shift;
                }
                break;
            }
        }
        Ok(result)
    }

    fn i32(&mut self) -> DecodeResult<i32> {
        Ok(self.i64()? as i32)
    }

    fn f32(&mut self) -> DecodeResult<f32> {
        let b = self.bytes(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f64(&mut self) -> DecodeResult<f64> {
        let b = self.bytes(8)?;
        Ok(f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    fn name(&mut self) -> DecodeResult<String> {
        let len = self.u32()? as usize;
        let b = self.bytes(len)?;
        String::from_utf8(b.to_vec()).map_err(|_| "invalid UTF-8 in name".into())
    }

    fn val_type(&mut self) -> DecodeResult<ValType> {
        let b = self.byte()?;
        ValType::from_byte(b).ok_or_else(|| format!("unknown value type 0x{b:02X}"))
    }

    fn limits(&mut self) -> DecodeResult<Limits> {
        let flag = self.byte()?;
        let min = self.u32()?;
        let max = if flag & 0x01 != 0 {
            Some(self.u32()?)
        } else {
            None
        };
        Ok(Limits { min, max })
    }
}

/// Validate the WASM magic + version header without a full decode (used by
/// `WebAssembly.validate`'s fast path).
pub fn check_header(data: &[u8]) -> bool {
    data.len() >= 8 && &data[0..4] == b"\0asm" && data[4..8] == [0x01, 0x00, 0x00, 0x00]
}

/// Decode a full module image.
pub fn parse_module(data: &[u8]) -> DecodeResult<Module> {
    if !check_header(data) {
        return Err("invalid WASM magic or version".into());
    }
    let mut r = Reader::new(data);
    r.pos = 8; // skip magic + version
    let mut m = Module::default();
    let mut last_section = 0u8;

    while r.remaining() > 0 {
        let id = r.byte()?;
        let size = r.u32()? as usize;
        let end = r.pos + size;
        if end > r.data.len() {
            return Err("section size exceeds module".into());
        }
        // Ordering check (custom sections id=0 may appear anywhere).
        if id != 0 {
            if id <= last_section {
                return Err(format!("section {id} out of order"));
            }
            last_section = id;
        }
        match id {
            0 => {} // custom section — skip
            1 => parse_type_section(&mut r, &mut m)?,
            2 => parse_import_section(&mut r, &mut m)?,
            3 => parse_function_section(&mut r, &mut m)?,
            4 => parse_table_section(&mut r, &mut m)?,
            5 => parse_memory_section(&mut r, &mut m)?,
            6 => parse_global_section(&mut r, &mut m)?,
            7 => parse_export_section(&mut r, &mut m)?,
            8 => m.start = Some(r.u32()?),
            9 => parse_element_section(&mut r, &mut m)?,
            10 => parse_code_section(&mut r, &mut m)?,
            11 => parse_data_section(&mut r, &mut m)?,
            12 => {
                // DataCount section — informational; skip.
                let _ = r.u32()?;
            }
            _ => return Err(format!("unknown section id {id}")),
        }
        // Always resync to the declared section end (tolerates skipped customs).
        r.pos = end;
    }

    if m.code.len() != m.funcs.len() {
        return Err("function and code section length mismatch".into());
    }
    Ok(m)
}

fn parse_type_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        let form = r.byte()?;
        if form != 0x60 {
            return Err(format!("expected func type 0x60, got 0x{form:02X}"));
        }
        let np = r.u32()?;
        let mut params = Vec::with_capacity(np as usize);
        for _ in 0..np {
            params.push(r.val_type()?);
        }
        let nr = r.u32()?;
        let mut results = Vec::with_capacity(nr as usize);
        for _ in 0..nr {
            results.push(r.val_type()?);
        }
        m.types.push(FuncType { params, results });
    }
    Ok(())
}

fn parse_import_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        let module = r.name()?;
        let name = r.name()?;
        let kind = r.byte()?;
        let ik = match kind {
            0x00 => {
                let t = r.u32()?;
                m.num_imported_funcs += 1;
                ImportKind::Func(t)
            }
            0x01 => {
                let elem = r.val_type()?;
                let limits = r.limits()?;
                m.num_imported_tables += 1;
                ImportKind::Table { elem, limits }
            }
            0x02 => {
                let limits = r.limits()?;
                m.num_imported_memories += 1;
                ImportKind::Memory(limits)
            }
            0x03 => {
                let ty = r.val_type()?;
                let mutb = r.byte()?;
                m.num_imported_globals += 1;
                ImportKind::Global {
                    ty,
                    mutable: mutb != 0,
                }
            }
            _ => return Err(format!("unknown import kind 0x{kind:02X}")),
        };
        m.imports.push(Import { module, name, kind: ik });
    }
    Ok(())
}

fn parse_function_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        m.funcs.push(r.u32()?);
    }
    Ok(())
}

fn parse_table_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        let elem = r.val_type()?;
        let limits = r.limits()?;
        m.tables.push((elem, limits));
    }
    Ok(())
}

fn parse_memory_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        m.memories.push(r.limits()?);
    }
    Ok(())
}

fn parse_global_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        let ty = r.val_type()?;
        let mutb = r.byte()?;
        let init = decode_expr(r)?;
        m.globals.push(GlobalDef {
            ty,
            mutable: mutb != 0,
            init,
        });
    }
    Ok(())
}

fn parse_export_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        let name = r.name()?;
        let kind_b = r.byte()?;
        let kind = match kind_b {
            0x00 => ExportKind::Func,
            0x01 => ExportKind::Table,
            0x02 => ExportKind::Memory,
            0x03 => ExportKind::Global,
            _ => return Err(format!("unknown export kind 0x{kind_b:02X}")),
        };
        let index = r.u32()?;
        m.exports.push(Export { name, kind, index });
    }
    Ok(())
}

fn parse_element_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        let flags = r.u32()?;
        // Common encodings: 0 = active table 0 with func indices.
        match flags {
            0 => {
                let offset = decode_expr(r)?;
                let n = r.u32()?;
                let mut func_indices = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    func_indices.push(r.u32()?);
                }
                m.elems.push(ElemSegment {
                    passive: false,
                    offset,
                    func_indices,
                });
            }
            1 => {
                // passive, elemkind + func indices
                let _elemkind = r.byte()?;
                let n = r.u32()?;
                let mut func_indices = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    func_indices.push(r.u32()?);
                }
                m.elems.push(ElemSegment {
                    passive: true,
                    offset: Vec::new(),
                    func_indices,
                });
            }
            2 => {
                let _table_idx = r.u32()?;
                let offset = decode_expr(r)?;
                let _elemkind = r.byte()?;
                let n = r.u32()?;
                let mut func_indices = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    func_indices.push(r.u32()?);
                }
                m.elems.push(ElemSegment {
                    passive: false,
                    offset,
                    func_indices,
                });
            }
            _ => return Err(format!("unsupported element segment flags {flags}")),
        }
    }
    Ok(())
}

fn parse_data_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        let flags = r.u32()?;
        match flags {
            0 => {
                let offset = decode_expr(r)?;
                let len = r.u32()? as usize;
                let bytes = r.bytes(len)?.to_vec();
                m.data.push(DataSegment {
                    passive: false,
                    offset,
                    bytes,
                });
            }
            1 => {
                let len = r.u32()? as usize;
                let bytes = r.bytes(len)?.to_vec();
                m.data.push(DataSegment {
                    passive: true,
                    offset: Vec::new(),
                    bytes,
                });
            }
            2 => {
                let _mem_idx = r.u32()?;
                let offset = decode_expr(r)?;
                let len = r.u32()? as usize;
                let bytes = r.bytes(len)?.to_vec();
                m.data.push(DataSegment {
                    passive: false,
                    offset,
                    bytes,
                });
            }
            _ => return Err(format!("unsupported data segment flags {flags}")),
        }
    }
    Ok(())
}

fn parse_code_section(r: &mut Reader, m: &mut Module) -> DecodeResult<()> {
    let count = r.u32()?;
    for _ in 0..count {
        let body_size = r.u32()? as usize;
        let body_end = r.pos + body_size;
        // locals
        let num_local_decls = r.u32()?;
        let mut locals = Vec::new();
        for _ in 0..num_local_decls {
            let n = r.u32()?;
            let ty = r.val_type()?;
            for _ in 0..n {
                locals.push(ty);
            }
        }
        let code = decode_expr(r)?;
        // resync to declared body end
        r.pos = body_end;
        m.code.push(FuncBody { locals, code });
    }
    Ok(())
}

/// Decode a block type immediate.
fn decode_block_type(r: &mut Reader) -> DecodeResult<BlockType> {
    let v = r.i64()?;
    Ok(match v {
        -64 => BlockType::Empty, // 0x40
        -1 => BlockType::Val(ValType::I32),
        -2 => BlockType::Val(ValType::I64),
        -3 => BlockType::Val(ValType::F32),
        -4 => BlockType::Val(ValType::F64),
        -5 => BlockType::Val(ValType::V128), // 0x7B
        -16 => BlockType::Val(ValType::FuncRef),
        -17 => BlockType::Val(ValType::ExternRef),
        n if n >= 0 => BlockType::Func(n as u32),
        _ => return Err(format!("invalid block type {v}")),
    })
}

/// Decode an instruction stream up to and including the matching outermost
/// `End`, then back-patch `Block`/`Loop`/`If` targets.
fn decode_expr(r: &mut Reader) -> DecodeResult<Vec<Instr>> {
    let mut out: Vec<Instr> = Vec::new();
    // Stack of (kind, index-in-`out`): kind 0=block,1=loop,2=if.
    let mut ctrl: Vec<(u8, usize)> = Vec::new();
    let mut depth: i64 = 1; // outer (implicit function/expr) block

    loop {
        let op = r.byte()?;
        match op {
            0x00 => out.push(Instr::Unreachable),
            0x01 => out.push(Instr::Nop),
            0x02 => {
                let ty = decode_block_type(r)?;
                ctrl.push((0, out.len()));
                out.push(Instr::Block { ty, end: 0 });
                depth += 1;
            }
            0x03 => {
                let ty = decode_block_type(r)?;
                ctrl.push((1, out.len()));
                out.push(Instr::Loop { ty, end: 0 });
                depth += 1;
            }
            0x04 => {
                let ty = decode_block_type(r)?;
                ctrl.push((2, out.len()));
                out.push(Instr::If {
                    ty,
                    else_: 0,
                    end: 0,
                });
                depth += 1;
            }
            0x05 => {
                // else — patch the owning if's else_ target to here+1
                let &(_, if_idx) = ctrl
                    .last()
                    .ok_or("else without matching if")?;
                out.push(Instr::Else);
                let after_else = out.len(); // first instr after Else
                if let Instr::If { else_, .. } = &mut out[if_idx] {
                    *else_ = after_else;
                } else {
                    return Err("else does not match an if".into());
                }
            }
            0x0B => {
                out.push(Instr::End);
                depth -= 1;
                if depth == 0 {
                    break;
                }
                let (_, start_idx) = ctrl.pop().ok_or("end without matching block")?;
                let end_pos = out.len(); // index past this End
                match &mut out[start_idx] {
                    Instr::Block { end, .. } | Instr::Loop { end, .. } => *end = end_pos,
                    Instr::If { end, else_, .. } => {
                        *end = end_pos;
                        if *else_ == 0 {
                            *else_ = end_pos; // no else: fall straight to end
                        }
                    }
                    _ => return Err("control stack desync".into()),
                }
            }
            0x0C => out.push(Instr::Br(r.u32()?)),
            0x0D => out.push(Instr::BrIf(r.u32()?)),
            0x0E => {
                let n = r.u32()?;
                let mut targets = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    targets.push(r.u32()?);
                }
                let default = r.u32()?;
                out.push(Instr::BrTable { targets, default });
            }
            0x0F => out.push(Instr::Return),
            0x10 => out.push(Instr::Call(r.u32()?)),
            0x11 => {
                let type_idx = r.u32()?;
                let table_idx = r.u32()?;
                out.push(Instr::CallIndirect {
                    type_idx,
                    table_idx,
                });
            }
            0x1A => out.push(Instr::Drop),
            0x1B => out.push(Instr::Select),
            0x1C => {
                // select with explicit types
                let n = r.u32()?;
                for _ in 0..n {
                    let _ = r.val_type()?;
                }
                out.push(Instr::Select);
            }
            0x20 => out.push(Instr::LocalGet(r.u32()?)),
            0x21 => out.push(Instr::LocalSet(r.u32()?)),
            0x22 => out.push(Instr::LocalTee(r.u32()?)),
            0x23 => out.push(Instr::GlobalGet(r.u32()?)),
            0x24 => out.push(Instr::GlobalSet(r.u32()?)),
            // memory loads 0x28..=0x35
            0x28..=0x35 => {
                let _align = r.u32()?;
                let offset = r.u32()?;
                out.push(Instr::Load { op, offset });
            }
            // memory stores 0x36..=0x3E
            0x36..=0x3E => {
                let _align = r.u32()?;
                let offset = r.u32()?;
                out.push(Instr::Store { op, offset });
            }
            0x3F => {
                let _reserved = r.byte()?;
                out.push(Instr::MemorySize);
            }
            0x40 => {
                let _reserved = r.byte()?;
                out.push(Instr::MemoryGrow);
            }
            0x41 => out.push(Instr::I32Const(r.i32()?)),
            0x42 => out.push(Instr::I64Const(r.i64()?)),
            0x43 => out.push(Instr::F32Const(r.f32()?)),
            0x44 => out.push(Instr::F64Const(r.f64()?)),
            // pure numeric/comparison/conversion/sign-ext ops
            0x45..=0xC4 => out.push(Instr::Num(op)),
            0xD0 => {
                let ty = r.val_type()?;
                out.push(Instr::RefNull(ty));
            }
            0xD1 => out.push(Instr::RefIsNull),
            0xD2 => out.push(Instr::RefFunc(r.u32()?)),
            0xFC => {
                let sub = r.u32()?;
                match sub {
                    0..=7 => out.push(Instr::TruncSat(sub as u8)),
                    8 => {
                        // memory.init — decode data idx + reserved, then trap at run time
                        let _data_idx = r.u32()?;
                        let _reserved = r.byte()?;
                        return Err("memory.init not supported".into());
                    }
                    9 => {
                        let _data_idx = r.u32()?;
                        // data.drop — ignore
                        out.push(Instr::Nop);
                    }
                    10 => {
                        let _dst = r.byte()?;
                        let _src = r.byte()?;
                        out.push(Instr::MemoryCopy);
                    }
                    11 => {
                        let _mem = r.byte()?;
                        out.push(Instr::MemoryFill);
                    }
                    _ => return Err(format!("unsupported 0xFC sub-opcode {sub}")),
                }
            }
            0xFD => {
                let sub = r.u32()?;
                match sub {
                    // memarg-only loads + load*_zero
                    0..=10 | 92 | 93 => {
                        let _align = r.u32()?;
                        let offset = r.u32()?;
                        out.push(Instr::V128Load { sub, offset });
                    }
                    // v128.store
                    11 => {
                        let _align = r.u32()?;
                        let offset = r.u32()?;
                        out.push(Instr::V128Store { offset });
                    }
                    // v128.const — 16 immediate bytes
                    12 => {
                        let b = r.bytes(16)?;
                        let mut bytes = [0u8; 16];
                        bytes.copy_from_slice(b);
                        out.push(Instr::V128Const(bytes));
                    }
                    // i8x16.shuffle — 16 immediate lane indices
                    13 => {
                        let b = r.bytes(16)?;
                        let mut lanes = [0u8; 16];
                        lanes.copy_from_slice(b);
                        out.push(Instr::Shuffle(lanes));
                    }
                    // extract_lane / replace_lane — single lane index byte
                    21..=34 => {
                        let lane = r.byte()?;
                        out.push(Instr::SimdLane { sub, lane });
                    }
                    // load_lane / store_lane — memarg + lane index byte
                    84..=91 => {
                        let _align = r.u32()?;
                        let offset = r.u32()?;
                        let lane = r.byte()?;
                        if sub <= 87 {
                            out.push(Instr::V128LoadLane { sub, offset, lane });
                        } else {
                            out.push(Instr::V128StoreLane { sub, offset, lane });
                        }
                    }
                    // everything else: pure stack op identified by sub-opcode
                    _ => out.push(Instr::Simd(sub)),
                }
            }
            0xFE => {
                let sub = r.u32()?;
                match sub {
                    // atomic.fence — single reserved byte, no memarg.
                    0x03 => {
                        let _reserved = r.byte()?;
                        out.push(Instr::AtomicFence);
                    }
                    // notify / wait32 / wait64 / loads / stores / rmw / cmpxchg —
                    // all carry a memarg (alignment hint + static offset).
                    0x00..=0x02 | 0x10..=0x4E => {
                        let _align = r.u32()?;
                        let offset = r.u32()?;
                        out.push(Instr::Atomic { sub, offset });
                    }
                    _ => return Err(format!("unsupported 0xFE sub-opcode {sub}")),
                }
            }
            _ => return Err(format!("unknown opcode 0x{op:02X}")),
        }
    }
    Ok(out)
}
