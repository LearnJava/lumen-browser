//! Stack-based interpreter for the decoded [`Module`] (MVP / WASM 1.0 core).
//!
//! Execution is pure Rust and independent of the JS engine: imported functions
//! are reached through the [`HostImports`] trait, which the JS bridge
//! implements to call back into QuickJS. This keeps the interpreter unit-testable
//! with a trivial stub host.
//!
//! Linear memory lives in Rust ([`Instance::memory`]) and is the single source
//! of truth; the JS side reads/writes it through copy helpers on the bridge.

use std::rc::Rc;

use super::parser::{Instr, Module};
use super::value::{FuncType, Value, ValType};

const PAGE_SIZE: usize = 65536;
const MAX_CALL_DEPTH: usize = 1024;

/// A runtime trap (maps to `WebAssembly.RuntimeError` on the JS side).
#[derive(Clone, Debug)]
pub struct Trap(pub String);

impl Trap {
    fn new(msg: impl Into<String>) -> Trap {
        Trap(msg.into())
    }
}

/// Host import callback surface. The interpreter calls this when WASM invokes
/// an imported function; the implementor maps `import_index` (the 0-based index
/// among the module's *function* imports) to a host (JS) function.
pub trait HostImports {
    /// Invoke imported function `import_index` with `args`; return its results.
    fn call_host(&mut self, import_index: usize, args: &[Value]) -> Result<Vec<Value>, Trap>;
}

/// A no-op host that traps on any imported call. Used when a module declares no
/// function imports, and as a test stub.
pub struct NullHost;
impl HostImports for NullHost {
    fn call_host(&mut self, import_index: usize, _args: &[Value]) -> Result<Vec<Value>, Trap> {
        Err(Trap::new(format!(
            "imported function {import_index} called but no host provided"
        )))
    }
}

/// An instantiated module: linear memory, globals, table, and a reference back
/// to the decoded [`Module`].
pub struct Instance {
    /// The decoded module (shared, immutable).
    pub module: Rc<Module>,
    /// Linear memory bytes (length is always a multiple of [`PAGE_SIZE`]).
    pub memory: Vec<u8>,
    /// Maximum memory pages (`None` = unbounded up to the 4 GiB ceiling).
    pub mem_max_pages: Option<u32>,
    /// Global values, indexed by global index (imported globals first).
    pub globals: Vec<Value>,
    /// Mutability flags parallel to [`Self::globals`].
    pub global_mut: Vec<bool>,
    /// `funcref` table: each slot is `Some(func_index)` or `None` (null).
    pub table: Vec<Option<u32>>,
}

/// A branch label on the control stack.
#[derive(Clone, Copy)]
struct Label {
    /// Number of result values the label's block produces (block/if) or
    /// consumes on entry (loop — always 0 for MVP block types).
    arity: usize,
    /// PC to jump to on branch (block/if: past the matching `End`; loop: the
    /// instruction after the `loop`).
    target: usize,
    /// Operand-stack height when the block was entered.
    height: usize,
    /// Whether this is a `loop` (branch re-enters instead of exits).
    is_loop: bool,
}

impl Instance {
    /// Instantiate a decoded module.
    ///
    /// `imported_globals` supplies the values for imported globals in import
    /// order (best-effort; missing entries default to the global's zero value).
    /// Function imports are resolved lazily through [`HostImports`] at call time.
    pub fn new(module: Rc<Module>, imported_globals: Vec<Value>) -> Result<Instance, String> {
        // ── Memory ──────────────────────────────────────────────────────────
        let mem_limits = if let Some(l) = module.memories.first() {
            Some(*l)
        } else {
            // imported memory?
            module.imports.iter().find_map(|imp| {
                if let super::parser::ImportKind::Memory(l) = imp.kind {
                    Some(l)
                } else {
                    None
                }
            })
        };
        let (memory, mem_max_pages) = match mem_limits {
            Some(l) => (vec![0u8; l.min as usize * PAGE_SIZE], l.max),
            None => (Vec::new(), Some(0)),
        };

        // ── Globals ─────────────────────────────────────────────────────────
        let mut globals: Vec<Value> = Vec::new();
        let mut global_mut: Vec<bool> = Vec::new();
        // imported globals first
        let mut imp_iter = imported_globals.into_iter();
        for imp in &module.imports {
            if let super::parser::ImportKind::Global { ty, mutable } = imp.kind {
                let v = imp_iter.next().unwrap_or_else(|| ty.default_value());
                globals.push(v);
                global_mut.push(mutable);
            }
        }
        // defined globals (init exprs may reference earlier globals)
        for g in &module.globals {
            let v = eval_const_expr(&g.init, &globals)?;
            globals.push(v);
            global_mut.push(g.mutable);
        }

        // ── Table ───────────────────────────────────────────────────────────
        let table_size = module
            .tables
            .first()
            .map(|(_, l)| l.min as usize)
            .or_else(|| {
                module.imports.iter().find_map(|imp| {
                    if let super::parser::ImportKind::Table { limits, .. } = imp.kind {
                        Some(limits.min as usize)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(0);
        let table: Vec<Option<u32>> = vec![None; table_size];

        let mut inst = Instance {
            module: module.clone(),
            memory,
            mem_max_pages,
            globals,
            global_mut,
            table,
        };

        // ── Element segments (active) ─────────────────────────────────────────
        let module2 = module.clone();
        for seg in &module2.elems {
            if seg.passive {
                continue;
            }
            let off = eval_const_expr(&seg.offset, &inst.globals)?.as_i32() as usize;
            for (i, &fi) in seg.func_indices.iter().enumerate() {
                let idx = off + i;
                if idx < inst.table.len() {
                    inst.table[idx] = Some(fi);
                }
            }
        }

        // ── Data segments (active) ────────────────────────────────────────────
        for seg in &module2.data {
            if seg.passive {
                continue;
            }
            let off = eval_const_expr(&seg.offset, &inst.globals)?.as_i32() as usize;
            let end = off.saturating_add(seg.bytes.len());
            if end > inst.memory.len() {
                return Err("data segment exceeds memory bounds".into());
            }
            inst.memory[off..end].copy_from_slice(&seg.bytes);
        }

        // `table` was moved into `inst` above; element segments wrote through
        // `inst.table`.
        Ok(inst)
    }

    /// Run the module's `start` function, if any.
    pub fn run_start(&mut self, host: &mut dyn HostImports) -> Result<(), Trap> {
        if let Some(idx) = self.module.start {
            self.invoke(idx, &[], host, 0)?;
        }
        Ok(())
    }

    /// Resolve an exported function's index by name.
    pub fn export_func_index(&self, name: &str) -> Option<u32> {
        self.module.exports.iter().find_map(|e| {
            if e.kind == super::parser::ExportKind::Func && e.name == name {
                Some(e.index)
            } else {
                None
            }
        })
    }

    /// Current memory size in pages.
    pub fn mem_pages(&self) -> u32 {
        (self.memory.len() / PAGE_SIZE) as u32
    }

    /// Grow memory by `delta` pages; return the previous page count, or -1 on
    /// failure (exceeds max or the 65536-page ceiling).
    pub fn mem_grow(&mut self, delta: u32) -> i32 {
        let prev = self.mem_pages();
        let next = prev as u64 + delta as u64;
        if next > 65536 {
            return -1;
        }
        if let Some(max) = self.mem_max_pages
            && next > max as u64
        {
            return -1;
        }
        self.memory.resize(next as usize * PAGE_SIZE, 0);
        prev as i32
    }

    /// Invoke any function by index (imported → host, defined → interpret).
    pub fn invoke(
        &mut self,
        func_idx: u32,
        args: &[Value],
        host: &mut dyn HostImports,
        depth: usize,
    ) -> Result<Vec<Value>, Trap> {
        if depth > MAX_CALL_DEPTH {
            return Err(Trap::new("call stack exhausted"));
        }
        let nimp = self.module.num_imported_funcs;
        if func_idx < nimp {
            // imported (host) function
            return host.call_host(func_idx as usize, args);
        }
        let defined_idx = (func_idx - nimp) as usize;
        let type_idx = *self
            .module
            .funcs
            .get(defined_idx)
            .ok_or_else(|| Trap::new("function index out of bounds"))?;
        let ftype = self
            .module
            .types
            .get(type_idx as usize)
            .ok_or_else(|| Trap::new("type index out of bounds"))?
            .clone();
        let body = self
            .module
            .code
            .get(defined_idx)
            .ok_or_else(|| Trap::new("code index out of bounds"))?
            .clone();

        // Locals = params followed by zero-initialised declared locals.
        let mut locals: Vec<Value> = Vec::with_capacity(ftype.params.len() + body.locals.len());
        for (i, _pt) in ftype.params.iter().enumerate() {
            locals.push(*args.get(i).ok_or_else(|| Trap::new("missing argument"))?);
        }
        for lt in &body.locals {
            locals.push(lt.default_value());
        }

        self.exec(&body.code, &mut locals, &ftype, host, depth)
    }

    /// Execute a function body to completion, returning its result values.
    fn exec(
        &mut self,
        body: &[Instr],
        locals: &mut [Value],
        ftype: &FuncType,
        host: &mut dyn HostImports,
        depth: usize,
    ) -> Result<Vec<Value>, Trap> {
        let result_arity = ftype.results.len();
        let mut stack: Vec<Value> = Vec::new();
        let mut labels: Vec<Label> = vec![Label {
            arity: result_arity,
            target: body.len(),
            height: 0,
            is_loop: false,
        }];
        let mut pc: usize = 0;

        macro_rules! pop {
            () => {
                stack.pop().ok_or_else(|| Trap::new("operand stack underflow"))?
            };
        }

        while pc < body.len() {
            let mut next = pc + 1;
            match &body[pc] {
                Instr::Unreachable => return Err(Trap::new("unreachable executed")),
                Instr::Nop => {}
                Instr::Block { ty, end } => {
                    labels.push(Label {
                        arity: block_result_arity(self, *ty),
                        target: *end,
                        height: stack.len(),
                        is_loop: false,
                    });
                }
                Instr::Loop { ty, .. } => {
                    labels.push(Label {
                        arity: block_param_arity(self, *ty),
                        target: pc + 1,
                        height: stack.len(),
                        is_loop: true,
                    });
                }
                Instr::If { ty, else_, end } => {
                    let c = pop!().as_i32();
                    labels.push(Label {
                        arity: block_result_arity(self, *ty),
                        target: *end,
                        height: stack.len(),
                        is_loop: false,
                    });
                    if c == 0 {
                        next = *else_;
                        if *else_ == *end {
                            // no else body — exit the block immediately
                            labels.pop();
                        }
                    }
                }
                Instr::Else => {
                    // reached at the end of a then-branch: skip the else body
                    let l = labels.pop().ok_or_else(|| Trap::new("else without label"))?;
                    next = l.target;
                }
                Instr::End => {
                    labels.pop();
                }
                Instr::Br(d) => {
                    next = do_branch(*d, &mut stack, &mut labels)?;
                }
                Instr::BrIf(d) => {
                    let c = pop!().as_i32();
                    if c != 0 {
                        next = do_branch(*d, &mut stack, &mut labels)?;
                    }
                }
                Instr::BrTable { targets, default } => {
                    let i = pop!().as_i32();
                    let d = if (i as usize) < targets.len() && i >= 0 {
                        targets[i as usize]
                    } else {
                        *default
                    };
                    next = do_branch(d, &mut stack, &mut labels)?;
                }
                Instr::Return => {
                    return Ok(take_top(&mut stack, result_arity));
                }
                Instr::Call(idx) => {
                    let ftype2 = self
                        .module
                        .func_type(*idx)
                        .ok_or_else(|| Trap::new("call: unknown function type"))?
                        .clone();
                    let nargs = ftype2.params.len();
                    if stack.len() < nargs {
                        return Err(Trap::new("call: not enough arguments"));
                    }
                    let args = stack.split_off(stack.len() - nargs);
                    let res = self.invoke(*idx, &args, host, depth + 1)?;
                    stack.extend(res);
                }
                Instr::CallIndirect { type_idx, .. } => {
                    let ti = pop!().as_i32();
                    if ti < 0 || (ti as usize) >= self.table.len() {
                        return Err(Trap::new("call_indirect: table index out of bounds"));
                    }
                    let func_idx = self.table[ti as usize]
                        .ok_or_else(|| Trap::new("call_indirect: null table element"))?;
                    let expected = self
                        .module
                        .types
                        .get(*type_idx as usize)
                        .ok_or_else(|| Trap::new("call_indirect: bad type index"))?;
                    let actual = self
                        .module
                        .func_type(func_idx)
                        .ok_or_else(|| Trap::new("call_indirect: bad function"))?;
                    if expected != actual {
                        return Err(Trap::new("call_indirect: signature mismatch"));
                    }
                    let nargs = expected.params.len();
                    if stack.len() < nargs {
                        return Err(Trap::new("call_indirect: not enough arguments"));
                    }
                    let args = stack.split_off(stack.len() - nargs);
                    let res = self.invoke(func_idx, &args, host, depth + 1)?;
                    stack.extend(res);
                }
                Instr::Drop => {
                    pop!();
                }
                Instr::Select => {
                    let c = pop!().as_i32();
                    let b = pop!();
                    let a = pop!();
                    stack.push(if c != 0 { a } else { b });
                }
                Instr::LocalGet(i) => {
                    let v = *locals
                        .get(*i as usize)
                        .ok_or_else(|| Trap::new("local.get out of bounds"))?;
                    stack.push(v);
                }
                Instr::LocalSet(i) => {
                    let v = pop!();
                    *locals
                        .get_mut(*i as usize)
                        .ok_or_else(|| Trap::new("local.set out of bounds"))? = v;
                }
                Instr::LocalTee(i) => {
                    let v = *stack.last().ok_or_else(|| Trap::new("local.tee underflow"))?;
                    *locals
                        .get_mut(*i as usize)
                        .ok_or_else(|| Trap::new("local.tee out of bounds"))? = v;
                }
                Instr::GlobalGet(i) => {
                    let v = *self
                        .globals
                        .get(*i as usize)
                        .ok_or_else(|| Trap::new("global.get out of bounds"))?;
                    stack.push(v);
                }
                Instr::GlobalSet(i) => {
                    let v = pop!();
                    let idx = *i as usize;
                    if idx >= self.globals.len() {
                        return Err(Trap::new("global.set out of bounds"));
                    }
                    self.globals[idx] = v;
                }
                Instr::Load { op, offset } => {
                    let addr = pop!().as_i32() as u32 as usize + *offset as usize;
                    let v = self.load(*op, addr)?;
                    stack.push(v);
                }
                Instr::Store { op, offset } => {
                    let v = pop!();
                    let addr = pop!().as_i32() as u32 as usize + *offset as usize;
                    self.store(*op, addr, v)?;
                }
                Instr::MemorySize => stack.push(Value::I32(self.mem_pages() as i32)),
                Instr::MemoryGrow => {
                    let delta = pop!().as_i32() as u32;
                    let prev = self.mem_grow(delta);
                    stack.push(Value::I32(prev));
                }
                Instr::I32Const(v) => stack.push(Value::I32(*v)),
                Instr::I64Const(v) => stack.push(Value::I64(*v)),
                Instr::F32Const(v) => stack.push(Value::F32(*v)),
                Instr::F64Const(v) => stack.push(Value::F64(*v)),
                Instr::Num(op) => exec_num(*op, &mut stack)?,
                Instr::TruncSat(sub) => exec_trunc_sat(*sub, &mut stack)?,
                Instr::MemoryCopy => {
                    let n = pop!().as_i32() as u32 as usize;
                    let src = pop!().as_i32() as u32 as usize;
                    let dst = pop!().as_i32() as u32 as usize;
                    if src + n > self.memory.len() || dst + n > self.memory.len() {
                        return Err(Trap::new("memory.copy out of bounds"));
                    }
                    self.memory.copy_within(src..src + n, dst);
                }
                Instr::MemoryFill => {
                    let n = pop!().as_i32() as u32 as usize;
                    let val = pop!().as_i32() as u8;
                    let dst = pop!().as_i32() as u32 as usize;
                    if dst + n > self.memory.len() {
                        return Err(Trap::new("memory.fill out of bounds"));
                    }
                    for b in &mut self.memory[dst..dst + n] {
                        *b = val;
                    }
                }
                Instr::RefNull(_) => stack.push(Value::FuncRef(None)),
                Instr::RefIsNull => {
                    let v = pop!();
                    let is_null = matches!(v, Value::FuncRef(None) | Value::ExternRef(None));
                    stack.push(Value::I32(is_null as i32));
                }
                Instr::RefFunc(idx) => stack.push(Value::FuncRef(Some(*idx))),
            }
            pc = next;
        }

        Ok(take_top(&mut stack, result_arity))
    }

    // ── Memory access ────────────────────────────────────────────────────────

    fn read_bytes(&self, addr: usize, n: usize) -> Result<&[u8], Trap> {
        let end = addr.checked_add(n).ok_or_else(|| Trap::new("address overflow"))?;
        if end > self.memory.len() {
            return Err(Trap::new("out-of-bounds memory access"));
        }
        Ok(&self.memory[addr..end])
    }

    fn write_bytes(&mut self, addr: usize, bytes: &[u8]) -> Result<(), Trap> {
        let end = addr
            .checked_add(bytes.len())
            .ok_or_else(|| Trap::new("address overflow"))?;
        if end > self.memory.len() {
            return Err(Trap::new("out-of-bounds memory access"));
        }
        self.memory[addr..end].copy_from_slice(bytes);
        Ok(())
    }

    fn load(&self, op: u8, addr: usize) -> Result<Value, Trap> {
        Ok(match op {
            0x28 => {
                let b = self.read_bytes(addr, 4)?;
                Value::I32(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }
            0x29 => {
                let b = self.read_bytes(addr, 8)?;
                Value::I64(i64::from_le_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]))
            }
            0x2A => {
                let b = self.read_bytes(addr, 4)?;
                Value::F32(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }
            0x2B => {
                let b = self.read_bytes(addr, 8)?;
                Value::F64(f64::from_le_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]))
            }
            0x2C => Value::I32(self.read_bytes(addr, 1)?[0] as i8 as i32),
            0x2D => Value::I32(self.read_bytes(addr, 1)?[0] as i32),
            0x2E => {
                let b = self.read_bytes(addr, 2)?;
                Value::I32(i16::from_le_bytes([b[0], b[1]]) as i32)
            }
            0x2F => {
                let b = self.read_bytes(addr, 2)?;
                Value::I32(u16::from_le_bytes([b[0], b[1]]) as i32)
            }
            0x30 => Value::I64(self.read_bytes(addr, 1)?[0] as i8 as i64),
            0x31 => Value::I64(self.read_bytes(addr, 1)?[0] as i64),
            0x32 => {
                let b = self.read_bytes(addr, 2)?;
                Value::I64(i16::from_le_bytes([b[0], b[1]]) as i64)
            }
            0x33 => {
                let b = self.read_bytes(addr, 2)?;
                Value::I64(u16::from_le_bytes([b[0], b[1]]) as i64)
            }
            0x34 => {
                let b = self.read_bytes(addr, 4)?;
                Value::I64(i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64)
            }
            0x35 => {
                let b = self.read_bytes(addr, 4)?;
                Value::I64(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64)
            }
            _ => return Err(Trap::new("bad load opcode")),
        })
    }

    fn store(&mut self, op: u8, addr: usize, v: Value) -> Result<(), Trap> {
        match op {
            0x36 => self.write_bytes(addr, &v.as_i32().to_le_bytes())?,
            0x37 => self.write_bytes(addr, &v.as_i64().to_le_bytes())?,
            0x38 => self.write_bytes(addr, &v.as_f32().to_le_bytes())?,
            0x39 => self.write_bytes(addr, &v.as_f64().to_le_bytes())?,
            0x3A => self.write_bytes(addr, &[(v.as_i32() as u8)])?,
            0x3B => self.write_bytes(addr, &(v.as_i32() as u16).to_le_bytes())?,
            0x3C => self.write_bytes(addr, &[(v.as_i64() as u8)])?,
            0x3D => self.write_bytes(addr, &(v.as_i64() as u16).to_le_bytes())?,
            0x3E => self.write_bytes(addr, &(v.as_i64() as u32).to_le_bytes())?,
            _ => return Err(Trap::new("bad store opcode")),
        }
        Ok(())
    }
}

/// Number of result values for a block type.
fn block_result_arity(inst: &Instance, ty: super::parser::BlockType) -> usize {
    use super::parser::BlockType;
    match ty {
        BlockType::Empty => 0,
        BlockType::Val(_) => 1,
        BlockType::Func(idx) => inst
            .module
            .types
            .get(idx as usize)
            .map(|t| t.results.len())
            .unwrap_or(0),
    }
}

/// Number of parameter values for a block type (loop branch arity).
fn block_param_arity(inst: &Instance, ty: super::parser::BlockType) -> usize {
    use super::parser::BlockType;
    match ty {
        BlockType::Empty | BlockType::Val(_) => 0,
        BlockType::Func(idx) => inst
            .module
            .types
            .get(idx as usize)
            .map(|t| t.params.len())
            .unwrap_or(0),
    }
}

/// Take the top `n` values off the stack (preserving order), discarding the rest.
fn take_top(stack: &mut Vec<Value>, n: usize) -> Vec<Value> {
    let len = stack.len();
    if n >= len {
        return std::mem::take(stack);
    }
    stack.split_off(len - n)
}

/// Perform a branch to label depth `d`, returning the new PC.
///
/// Keeps the top `arity` operands, resets the operand stack to the block's
/// entry height, and unwinds the label stack: a `loop` target stays on the
/// stack (the branch re-enters it), a `block`/`if` target is popped (the branch
/// exits it).
fn do_branch(d: u32, stack: &mut Vec<Value>, labels: &mut Vec<Label>) -> Result<usize, Trap> {
    if d as usize >= labels.len() {
        return Err(Trap::new("branch depth out of range"));
    }
    let idx = labels.len() - 1 - d as usize;
    let label = labels[idx];
    // Keep the top `arity` values, drop everything down to the block's height.
    let keep = take_top(stack, label.arity);
    stack.truncate(label.height);
    stack.extend(keep);
    if label.is_loop {
        labels.truncate(idx + 1);
    } else {
        labels.truncate(idx);
    }
    Ok(label.target)
}

/// Evaluate a constant initialiser expression (globals, segment offsets).
fn eval_const_expr(expr: &[Instr], globals: &[Value]) -> Result<Value, String> {
    // A constant expression is a single value-producing instruction terminated
    // by `End`; take the first non-`End` instruction.
    let Some(instr) = expr.iter().find(|i| !matches!(i, Instr::End)) else {
        return Ok(Value::I32(0));
    };
    match instr {
        Instr::I32Const(v) => Ok(Value::I32(*v)),
        Instr::I64Const(v) => Ok(Value::I64(*v)),
        Instr::F32Const(v) => Ok(Value::F32(*v)),
        Instr::F64Const(v) => Ok(Value::F64(*v)),
        Instr::GlobalGet(i) => globals
            .get(*i as usize)
            .copied()
            .ok_or_else(|| "const expr references unknown global".to_string()),
        Instr::RefNull(_) => Ok(Value::FuncRef(None)),
        Instr::RefFunc(idx) => Ok(Value::FuncRef(Some(*idx))),
        _ => Err("unsupported constant expression".into()),
    }
}

// ── Numeric op dispatch ────────────────────────────────────────────────────────

/// Execute a pure numeric/comparison/conversion op (opcodes 0x45..=0xC4).
#[allow(clippy::too_many_lines)]
fn exec_num(op: u8, stack: &mut Vec<Value>) -> Result<(), Trap> {
    macro_rules! pop {
        () => {
            stack.pop().ok_or_else(|| Trap::new("operand stack underflow"))?
        };
    }
    macro_rules! bool_i32 {
        ($e:expr) => {
            stack.push(Value::I32(if $e { 1 } else { 0 }))
        };
    }
    match op {
        // ── i32 comparisons ──
        0x45 => {
            let a = pop!().as_i32();
            bool_i32!(a == 0);
        }
        0x46 => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            bool_i32!(a == b);
        }
        0x47 => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            bool_i32!(a != b);
        }
        0x48 => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            bool_i32!(a < b);
        }
        0x49 => {
            let b = pop!().as_i32() as u32;
            let a = pop!().as_i32() as u32;
            bool_i32!(a < b);
        }
        0x4A => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            bool_i32!(a > b);
        }
        0x4B => {
            let b = pop!().as_i32() as u32;
            let a = pop!().as_i32() as u32;
            bool_i32!(a > b);
        }
        0x4C => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            bool_i32!(a <= b);
        }
        0x4D => {
            let b = pop!().as_i32() as u32;
            let a = pop!().as_i32() as u32;
            bool_i32!(a <= b);
        }
        0x4E => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            bool_i32!(a >= b);
        }
        0x4F => {
            let b = pop!().as_i32() as u32;
            let a = pop!().as_i32() as u32;
            bool_i32!(a >= b);
        }
        // ── i64 comparisons ──
        0x50 => {
            let a = pop!().as_i64();
            bool_i32!(a == 0);
        }
        0x51 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            bool_i32!(a == b);
        }
        0x52 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            bool_i32!(a != b);
        }
        0x53 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            bool_i32!(a < b);
        }
        0x54 => {
            let b = pop!().as_i64() as u64;
            let a = pop!().as_i64() as u64;
            bool_i32!(a < b);
        }
        0x55 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            bool_i32!(a > b);
        }
        0x56 => {
            let b = pop!().as_i64() as u64;
            let a = pop!().as_i64() as u64;
            bool_i32!(a > b);
        }
        0x57 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            bool_i32!(a <= b);
        }
        0x58 => {
            let b = pop!().as_i64() as u64;
            let a = pop!().as_i64() as u64;
            bool_i32!(a <= b);
        }
        0x59 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            bool_i32!(a >= b);
        }
        0x5A => {
            let b = pop!().as_i64() as u64;
            let a = pop!().as_i64() as u64;
            bool_i32!(a >= b);
        }
        // ── f32 comparisons ──
        0x5B => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            bool_i32!(a == b);
        }
        0x5C => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            bool_i32!(a != b);
        }
        0x5D => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            bool_i32!(a < b);
        }
        0x5E => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            bool_i32!(a > b);
        }
        0x5F => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            bool_i32!(a <= b);
        }
        0x60 => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            bool_i32!(a >= b);
        }
        // ── f64 comparisons ──
        0x61 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            bool_i32!(a == b);
        }
        0x62 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            bool_i32!(a != b);
        }
        0x63 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            bool_i32!(a < b);
        }
        0x64 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            bool_i32!(a > b);
        }
        0x65 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            bool_i32!(a <= b);
        }
        0x66 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            bool_i32!(a >= b);
        }
        // ── i32 arithmetic ──
        0x67 => {
            let a = pop!().as_i32();
            stack.push(Value::I32(a.leading_zeros() as i32));
        }
        0x68 => {
            let a = pop!().as_i32();
            stack.push(Value::I32(a.trailing_zeros() as i32));
        }
        0x69 => {
            let a = pop!().as_i32();
            stack.push(Value::I32(a.count_ones() as i32));
        }
        0x6A => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            stack.push(Value::I32(a.wrapping_add(b)));
        }
        0x6B => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            stack.push(Value::I32(a.wrapping_sub(b)));
        }
        0x6C => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            stack.push(Value::I32(a.wrapping_mul(b)));
        }
        0x6D => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            if b == 0 {
                return Err(Trap::new("integer divide by zero"));
            }
            if a == i32::MIN && b == -1 {
                return Err(Trap::new("integer overflow"));
            }
            stack.push(Value::I32(a / b));
        }
        0x6E => {
            let b = pop!().as_i32() as u32;
            let a = pop!().as_i32() as u32;
            if b == 0 {
                return Err(Trap::new("integer divide by zero"));
            }
            stack.push(Value::I32((a / b) as i32));
        }
        0x6F => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            if b == 0 {
                return Err(Trap::new("integer divide by zero"));
            }
            stack.push(Value::I32(a.wrapping_rem(b)));
        }
        0x70 => {
            let b = pop!().as_i32() as u32;
            let a = pop!().as_i32() as u32;
            if b == 0 {
                return Err(Trap::new("integer divide by zero"));
            }
            stack.push(Value::I32((a % b) as i32));
        }
        0x71 => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            stack.push(Value::I32(a & b));
        }
        0x72 => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            stack.push(Value::I32(a | b));
        }
        0x73 => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            stack.push(Value::I32(a ^ b));
        }
        0x74 => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            stack.push(Value::I32(a.wrapping_shl(b as u32)));
        }
        0x75 => {
            let b = pop!().as_i32();
            let a = pop!().as_i32();
            stack.push(Value::I32(a.wrapping_shr(b as u32)));
        }
        0x76 => {
            let b = pop!().as_i32() as u32;
            let a = pop!().as_i32() as u32;
            stack.push(Value::I32(a.wrapping_shr(b) as i32));
        }
        0x77 => {
            let b = pop!().as_i32() as u32;
            let a = pop!().as_i32();
            stack.push(Value::I32(a.rotate_left(b & 31)));
        }
        0x78 => {
            let b = pop!().as_i32() as u32;
            let a = pop!().as_i32();
            stack.push(Value::I32(a.rotate_right(b & 31)));
        }
        // ── i64 arithmetic ──
        0x79 => {
            let a = pop!().as_i64();
            stack.push(Value::I64(a.leading_zeros() as i64));
        }
        0x7A => {
            let a = pop!().as_i64();
            stack.push(Value::I64(a.trailing_zeros() as i64));
        }
        0x7B => {
            let a = pop!().as_i64();
            stack.push(Value::I64(a.count_ones() as i64));
        }
        0x7C => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a.wrapping_add(b)));
        }
        0x7D => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a.wrapping_sub(b)));
        }
        0x7E => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a.wrapping_mul(b)));
        }
        0x7F => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            if b == 0 {
                return Err(Trap::new("integer divide by zero"));
            }
            if a == i64::MIN && b == -1 {
                return Err(Trap::new("integer overflow"));
            }
            stack.push(Value::I64(a / b));
        }
        0x80 => {
            let b = pop!().as_i64() as u64;
            let a = pop!().as_i64() as u64;
            if b == 0 {
                return Err(Trap::new("integer divide by zero"));
            }
            stack.push(Value::I64((a / b) as i64));
        }
        0x81 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            if b == 0 {
                return Err(Trap::new("integer divide by zero"));
            }
            stack.push(Value::I64(a.wrapping_rem(b)));
        }
        0x82 => {
            let b = pop!().as_i64() as u64;
            let a = pop!().as_i64() as u64;
            if b == 0 {
                return Err(Trap::new("integer divide by zero"));
            }
            stack.push(Value::I64((a % b) as i64));
        }
        0x83 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a & b));
        }
        0x84 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a | b));
        }
        0x85 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a ^ b));
        }
        0x86 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a.wrapping_shl(b as u32)));
        }
        0x87 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a.wrapping_shr(b as u32)));
        }
        0x88 => {
            let b = pop!().as_i64() as u64;
            let a = pop!().as_i64() as u64;
            stack.push(Value::I64(a.wrapping_shr(b as u32) as i64));
        }
        0x89 => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a.rotate_left((b & 63) as u32)));
        }
        0x8A => {
            let b = pop!().as_i64();
            let a = pop!().as_i64();
            stack.push(Value::I64(a.rotate_right((b & 63) as u32)));
        }
        // ── f32 arithmetic ──
        0x8B => {
            let a = pop!().as_f32();
            stack.push(Value::F32(a.abs()));
        }
        0x8C => {
            let a = pop!().as_f32();
            stack.push(Value::F32(-a));
        }
        0x8D => {
            let a = pop!().as_f32();
            stack.push(Value::F32(a.ceil()));
        }
        0x8E => {
            let a = pop!().as_f32();
            stack.push(Value::F32(a.floor()));
        }
        0x8F => {
            let a = pop!().as_f32();
            stack.push(Value::F32(a.trunc()));
        }
        0x90 => {
            let a = pop!().as_f32();
            stack.push(Value::F32(round_nearest_even_f32(a)));
        }
        0x91 => {
            let a = pop!().as_f32();
            stack.push(Value::F32(a.sqrt()));
        }
        0x92 => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            stack.push(Value::F32(a + b));
        }
        0x93 => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            stack.push(Value::F32(a - b));
        }
        0x94 => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            stack.push(Value::F32(a * b));
        }
        0x95 => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            stack.push(Value::F32(a / b));
        }
        0x96 => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            stack.push(Value::F32(wasm_fmin_f32(a, b)));
        }
        0x97 => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            stack.push(Value::F32(wasm_fmax_f32(a, b)));
        }
        0x98 => {
            let b = pop!().as_f32();
            let a = pop!().as_f32();
            stack.push(Value::F32(a.copysign(b)));
        }
        // ── f64 arithmetic ──
        0x99 => {
            let a = pop!().as_f64();
            stack.push(Value::F64(a.abs()));
        }
        0x9A => {
            let a = pop!().as_f64();
            stack.push(Value::F64(-a));
        }
        0x9B => {
            let a = pop!().as_f64();
            stack.push(Value::F64(a.ceil()));
        }
        0x9C => {
            let a = pop!().as_f64();
            stack.push(Value::F64(a.floor()));
        }
        0x9D => {
            let a = pop!().as_f64();
            stack.push(Value::F64(a.trunc()));
        }
        0x9E => {
            let a = pop!().as_f64();
            stack.push(Value::F64(round_nearest_even_f64(a)));
        }
        0x9F => {
            let a = pop!().as_f64();
            stack.push(Value::F64(a.sqrt()));
        }
        0xA0 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            stack.push(Value::F64(a + b));
        }
        0xA1 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            stack.push(Value::F64(a - b));
        }
        0xA2 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            stack.push(Value::F64(a * b));
        }
        0xA3 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            stack.push(Value::F64(a / b));
        }
        0xA4 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            stack.push(Value::F64(wasm_fmin_f64(a, b)));
        }
        0xA5 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            stack.push(Value::F64(wasm_fmax_f64(a, b)));
        }
        0xA6 => {
            let b = pop!().as_f64();
            let a = pop!().as_f64();
            stack.push(Value::F64(a.copysign(b)));
        }
        // ── conversions ──
        0xA7 => {
            let a = pop!().as_i64();
            stack.push(Value::I32(a as i32));
        }
        0xA8 => {
            let a = pop!().as_f32();
            stack.push(Value::I32(trunc_f32_i32(a, true)?));
        }
        0xA9 => {
            let a = pop!().as_f32();
            stack.push(Value::I32(trunc_f32_u32(a)? as i32));
        }
        0xAA => {
            let a = pop!().as_f64();
            stack.push(Value::I32(trunc_f64_i32(a, true)?));
        }
        0xAB => {
            let a = pop!().as_f64();
            stack.push(Value::I32(trunc_f64_u32(a)? as i32));
        }
        0xAC => {
            let a = pop!().as_i32();
            stack.push(Value::I64(a as i64));
        }
        0xAD => {
            let a = pop!().as_i32();
            stack.push(Value::I64(a as u32 as i64));
        }
        0xAE => {
            let a = pop!().as_f32();
            stack.push(Value::I64(trunc_f32_i64(a)?));
        }
        0xAF => {
            let a = pop!().as_f32();
            stack.push(Value::I64(trunc_f32_u64(a)? as i64));
        }
        0xB0 => {
            let a = pop!().as_f64();
            stack.push(Value::I64(trunc_f64_i64(a)?));
        }
        0xB1 => {
            let a = pop!().as_f64();
            stack.push(Value::I64(trunc_f64_u64(a)? as i64));
        }
        0xB2 => {
            let a = pop!().as_i32();
            stack.push(Value::F32(a as f32));
        }
        0xB3 => {
            let a = pop!().as_i32();
            stack.push(Value::F32(a as u32 as f32));
        }
        0xB4 => {
            let a = pop!().as_i64();
            stack.push(Value::F32(a as f32));
        }
        0xB5 => {
            let a = pop!().as_i64();
            stack.push(Value::F32(a as u64 as f32));
        }
        0xB6 => {
            let a = pop!().as_f64();
            stack.push(Value::F32(a as f32));
        }
        0xB7 => {
            let a = pop!().as_i32();
            stack.push(Value::F64(a as f64));
        }
        0xB8 => {
            let a = pop!().as_i32();
            stack.push(Value::F64(a as u32 as f64));
        }
        0xB9 => {
            let a = pop!().as_i64();
            stack.push(Value::F64(a as f64));
        }
        0xBA => {
            let a = pop!().as_i64();
            stack.push(Value::F64(a as u64 as f64));
        }
        0xBB => {
            let a = pop!().as_f32();
            stack.push(Value::F64(a as f64));
        }
        0xBC => {
            let a = pop!().as_f32();
            stack.push(Value::I32(a.to_bits() as i32));
        }
        0xBD => {
            let a = pop!().as_f64();
            stack.push(Value::I64(a.to_bits() as i64));
        }
        0xBE => {
            let a = pop!().as_i32();
            stack.push(Value::F32(f32::from_bits(a as u32)));
        }
        0xBF => {
            let a = pop!().as_i64();
            stack.push(Value::F64(f64::from_bits(a as u64)));
        }
        // ── sign extension ──
        0xC0 => {
            let a = pop!().as_i32();
            stack.push(Value::I32(a as i8 as i32));
        }
        0xC1 => {
            let a = pop!().as_i32();
            stack.push(Value::I32(a as i16 as i32));
        }
        0xC2 => {
            let a = pop!().as_i64();
            stack.push(Value::I64(a as i8 as i64));
        }
        0xC3 => {
            let a = pop!().as_i64();
            stack.push(Value::I64(a as i16 as i64));
        }
        0xC4 => {
            let a = pop!().as_i64();
            stack.push(Value::I64(a as i32 as i64));
        }
        _ => return Err(Trap::new(format!("unsupported numeric opcode 0x{op:02X}"))),
    }
    Ok(())
}

/// Saturating float→int truncation (`0xFC` sub-opcodes 0..=7).
fn exec_trunc_sat(sub: u8, stack: &mut Vec<Value>) -> Result<(), Trap> {
    macro_rules! pop {
        () => {
            stack.pop().ok_or_else(|| Trap::new("operand stack underflow"))?
        };
    }
    match sub {
        0 => {
            let a = pop!().as_f32();
            stack.push(Value::I32(sat_f32_i32(a)));
        }
        1 => {
            let a = pop!().as_f32();
            stack.push(Value::I32(sat_f32_u32(a) as i32));
        }
        2 => {
            let a = pop!().as_f64();
            stack.push(Value::I32(sat_f64_i32(a)));
        }
        3 => {
            let a = pop!().as_f64();
            stack.push(Value::I32(sat_f64_u32(a) as i32));
        }
        4 => {
            let a = pop!().as_f32();
            stack.push(Value::I64(sat_f32_i64(a)));
        }
        5 => {
            let a = pop!().as_f32();
            stack.push(Value::I64(sat_f32_u64(a) as i64));
        }
        6 => {
            let a = pop!().as_f64();
            stack.push(Value::I64(sat_f64_i64(a)));
        }
        7 => {
            let a = pop!().as_f64();
            stack.push(Value::I64(sat_f64_u64(a) as i64));
        }
        _ => return Err(Trap::new("bad trunc_sat sub-opcode")),
    }
    Ok(())
}

// ── Float helpers ───────────────────────────────────────────────────────────

/// IEEE round-half-to-even for f32 (`f32.nearest`).
fn round_nearest_even_f32(a: f32) -> f32 {
    let r = a.round();
    if (a - a.floor() - 0.5).abs() < f32::EPSILON {
        // halfway: pick the even neighbour
        let f = a.floor();
        if (f as i64) % 2 == 0 {
            f
        } else {
            f + 1.0
        }
    } else {
        r
    }
}

/// IEEE round-half-to-even for f64 (`f64.nearest`).
fn round_nearest_even_f64(a: f64) -> f64 {
    let r = a.round();
    if (a - a.floor() - 0.5).abs() < f64::EPSILON {
        let f = a.floor();
        if (f as i64) % 2 == 0 {
            f
        } else {
            f + 1.0
        }
    } else {
        r
    }
}

/// WASM `f32.min`: NaN-propagating, -0 < +0.
fn wasm_fmin_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b {
        // handle ±0: min picks -0
        if a.is_sign_negative() { a } else { b }
    } else {
        a.min(b)
    }
}

fn wasm_fmax_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b {
        if a.is_sign_positive() { a } else { b }
    } else {
        a.max(b)
    }
}

fn wasm_fmin_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else {
        a.min(b)
    }
}

fn wasm_fmax_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_positive() { a } else { b }
    } else {
        a.max(b)
    }
}

// Trapping truncations: trap on NaN or out-of-range.
fn trunc_f32_i32(a: f32, _signed: bool) -> Result<i32, Trap> {
    if a.is_nan() {
        return Err(Trap::new("invalid conversion to integer"));
    }
    let t = a.trunc();
    if t < i32::MIN as f32 || t >= 2147483648.0 {
        return Err(Trap::new("integer overflow"));
    }
    Ok(t as i32)
}
fn trunc_f32_u32(a: f32) -> Result<u32, Trap> {
    if a.is_nan() {
        return Err(Trap::new("invalid conversion to integer"));
    }
    let t = a.trunc();
    if t <= -1.0 || t >= 4294967296.0 {
        return Err(Trap::new("integer overflow"));
    }
    Ok(t as u32)
}
fn trunc_f64_i32(a: f64, _signed: bool) -> Result<i32, Trap> {
    if a.is_nan() {
        return Err(Trap::new("invalid conversion to integer"));
    }
    let t = a.trunc();
    if t < i32::MIN as f64 || t > i32::MAX as f64 {
        return Err(Trap::new("integer overflow"));
    }
    Ok(t as i32)
}
fn trunc_f64_u32(a: f64) -> Result<u32, Trap> {
    if a.is_nan() {
        return Err(Trap::new("invalid conversion to integer"));
    }
    let t = a.trunc();
    if t <= -1.0 || t > u32::MAX as f64 {
        return Err(Trap::new("integer overflow"));
    }
    Ok(t as u32)
}
fn trunc_f32_i64(a: f32) -> Result<i64, Trap> {
    if a.is_nan() {
        return Err(Trap::new("invalid conversion to integer"));
    }
    let t = a.trunc();
    if t < i64::MIN as f32 || t >= 9223372036854775808.0 {
        return Err(Trap::new("integer overflow"));
    }
    Ok(t as i64)
}
fn trunc_f32_u64(a: f32) -> Result<u64, Trap> {
    if a.is_nan() {
        return Err(Trap::new("invalid conversion to integer"));
    }
    let t = a.trunc();
    if t <= -1.0 || t >= 18446744073709551616.0 {
        return Err(Trap::new("integer overflow"));
    }
    Ok(t as u64)
}
fn trunc_f64_i64(a: f64) -> Result<i64, Trap> {
    if a.is_nan() {
        return Err(Trap::new("invalid conversion to integer"));
    }
    let t = a.trunc();
    if t < i64::MIN as f64 || t >= 9223372036854775808.0 {
        return Err(Trap::new("integer overflow"));
    }
    Ok(t as i64)
}
fn trunc_f64_u64(a: f64) -> Result<u64, Trap> {
    if a.is_nan() {
        return Err(Trap::new("invalid conversion to integer"));
    }
    let t = a.trunc();
    if t <= -1.0 || t >= 18446744073709551616.0 {
        return Err(Trap::new("integer overflow"));
    }
    Ok(t as u64)
}

// Saturating truncations.
fn sat_f32_i32(a: f32) -> i32 {
    if a.is_nan() {
        0
    } else {
        let t = a.trunc();
        if t < i32::MIN as f32 {
            i32::MIN
        } else if t >= 2147483648.0 {
            i32::MAX
        } else {
            t as i32
        }
    }
}
fn sat_f32_u32(a: f32) -> u32 {
    if a.is_nan() {
        0
    } else {
        let t = a.trunc();
        if t <= 0.0 {
            0
        } else if t >= 4294967296.0 {
            u32::MAX
        } else {
            t as u32
        }
    }
}
fn sat_f64_i32(a: f64) -> i32 {
    if a.is_nan() {
        0
    } else {
        let t = a.trunc();
        if t < i32::MIN as f64 {
            i32::MIN
        } else if t > i32::MAX as f64 {
            i32::MAX
        } else {
            t as i32
        }
    }
}
fn sat_f64_u32(a: f64) -> u32 {
    if a.is_nan() {
        0
    } else {
        let t = a.trunc();
        if t <= 0.0 {
            0
        } else if t > u32::MAX as f64 {
            u32::MAX
        } else {
            t as u32
        }
    }
}
fn sat_f32_i64(a: f32) -> i64 {
    if a.is_nan() {
        0
    } else {
        let t = a.trunc();
        if t < i64::MIN as f32 {
            i64::MIN
        } else if t >= 9223372036854775808.0 {
            i64::MAX
        } else {
            t as i64
        }
    }
}
fn sat_f32_u64(a: f32) -> u64 {
    if a.is_nan() {
        0
    } else {
        let t = a.trunc();
        if t <= 0.0 {
            0
        } else if t >= 18446744073709551616.0 {
            u64::MAX
        } else {
            t as u64
        }
    }
}
fn sat_f64_i64(a: f64) -> i64 {
    if a.is_nan() {
        0
    } else {
        let t = a.trunc();
        if t < i64::MIN as f64 {
            i64::MIN
        } else if t >= 9223372036854775808.0 {
            i64::MAX
        } else {
            t as i64
        }
    }
}
fn sat_f64_u64(a: f64) -> u64 {
    if a.is_nan() {
        0
    } else {
        let t = a.trunc();
        if t <= 0.0 {
            0
        } else if t >= 18446744073709551616.0 {
            u64::MAX
        } else {
            t as u64
        }
    }
}

/// Helper used by tests/value bridging: drop unused warning.
#[allow(dead_code)]
fn _vt(_: ValType) {}
