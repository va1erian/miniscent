//! The VM's state and its per-step entry points. Running ops is in `exec.rs`.
//! All state is plain indexed data, so `Vm` is `Clone` and `Send`, and a task
//! parks by leaving the run loop rather than holding a Rust stack.

use crate::code::Program;
use crate::error::ScriptError;
use crate::heap::{Heap, Obj, Ref, Value};
use std::collections::VecDeque;
use std::sync::Arc;

/// The game steps at 120 Hz; `wait:` seconds are rounded up to whole steps.
pub const STEP_HZ: f32 = 120.0;
/// Operations one task may run before it is stopped. Counting operations, not
/// time, is what makes a replay stop in the same place.
pub const OPS_PER_RUN: u32 = 20_000;
/// Task runs in one step. The rest are deferred, ahead of everything else in
/// the next step, so none can starve.
pub const RUNS_PER_STEP: usize = 16;
/// Live tasks, suspended ones included. Reaching it is a script error. No
/// real level has a hundred suspended handlers.
pub const MAX_TASKS: usize = 128;
/// Calls in one task, a block value counting as a call.
pub(crate) const MAX_FRAMES: usize = 128;
/// Values on one task's stack. Stacks sit outside the heap budget, so the
/// worst case is `MAX_TASKS * MAX_STACK * 12` bytes, about 6 MiB.
pub(crate) const MAX_STACK: usize = 4096;
/// Runtime errors kept until the host drains them. A host that never drains
/// loses the newest, not memory.
const MAX_PENDING_ERRORS: usize = 256;
/// Collection is skipped while the heap is under this many bytes.
const MIN_GC_BYTES: usize = 256 << 10;

pub enum Flow {
    Val(Value),
    /// Suspend this task for a number of seconds, then resume with nil.
    Wait(f32),
}

pub type Native<H> = fn(&mut H, &mut Heap, Value, &[Value]) -> Result<Flow, String>;

/// Shows the collector the values the host holds, such as handler blocks.
/// A value the host keeps and does not show here can be collected.
pub trait Host {
    fn roots(&self, visit: &mut dyn FnMut(Value));
}

#[derive(Clone, Debug)]
pub(crate) struct Frame {
    pub code: u16,
    pub ip: u32,
    pub env: Ref,
    /// Unique for the life of the VM, so `^` can find its method's frame.
    pub id: u64,
    pub home: u64,
    pub base: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Task {
    pub id: u64,
    pub frames: Vec<Frame>,
    pub stack: Vec<Value>,
    pub wake: u64,
}

pub struct Vm<H> {
    pub(crate) program: Arc<Program<H>>,
    pub(crate) heap: Heap,
    pub(crate) tasks: VecDeque<Task>,
    pub(crate) tick: u64,
    pub(crate) next_frame: u64,
    next_task: u64,
    pub(crate) live_tasks: usize,
    pub(crate) top_env: Ref,
    pub(crate) errors: Vec<ScriptError>,
    pub(crate) ops: u64,
    gc_at: usize,
}

// Not derived: that would demand `H: Clone`, and `H` only appears in fn pointers.
impl<H> Clone for Vm<H> {
    fn clone(&self) -> Self {
        Self {
            program: Arc::clone(&self.program),
            heap: self.heap.clone(),
            tasks: self.tasks.clone(),
            tick: self.tick,
            next_frame: self.next_frame,
            next_task: self.next_task,
            live_tasks: self.live_tasks,
            top_env: self.top_env,
            errors: self.errors.clone(),
            ops: self.ops,
            gc_at: self.gc_at,
        }
    }
}

impl<H> Vm<H> {
    pub(crate) fn new(program: Program<H>, heap: Heap, top_env: Ref) -> Self {
        Self {
            program: Arc::new(program),
            heap,
            tasks: VecDeque::new(),
            tick: 0,
            next_frame: 0,
            next_task: 0,
            live_tasks: 0,
            top_env,
            errors: Vec::new(),
            ops: 0,
            gc_at: MIN_GC_BYTES,
        }
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }

    /// Steps taken so far.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Operations run so far, across all tasks.
    pub fn ops(&self) -> u64 {
        self.ops
    }

    /// Live tasks, suspended ones included.
    pub fn task_count(&self) -> usize {
        self.live_tasks
    }

    pub fn task_alive(&self, id: u64) -> bool {
        self.tasks.iter().any(|t| t.id == id)
    }

    /// Selectors of the natives this VM was loaded with, in table order.
    pub fn native_selectors(&self) -> impl Iterator<Item = &str> {
        self.program.native_names.iter().map(String::as_str)
    }

    pub fn errors(&self) -> &[ScriptError] {
        &self.errors
    }

    /// Hands over the runtime errors raised since the last call.
    pub fn take_errors(&mut self) -> Vec<ScriptError> {
        std::mem::take(&mut self.errors)
    }

    pub(crate) fn push_error(&mut self, e: ScriptError) {
        if self.errors.len() < MAX_PENDING_ERRORS {
            self.errors.push(e);
        }
    }

    pub(crate) fn frame_id(&mut self) -> u64 {
        self.next_frame = self.next_frame.saturating_add(1);
        self.next_frame
    }

    pub(crate) fn new_task(&mut self) -> Result<Task, String> {
        if self.live_tasks >= MAX_TASKS {
            return Err(format!("too many tasks: the limit is {MAX_TASKS}"));
        }
        self.next_task = self.next_task.saturating_add(1);
        self.live_tasks += 1;
        Ok(Task {
            id: self.next_task,
            frames: Vec::new(),
            stack: Vec::new(),
            wake: self.tick,
        })
    }

    /// Where an error with no running op belongs: the block's own definition.
    fn host_error(&self, block: Value, msg: String) -> ScriptError {
        let at = self
            .heap
            .block(block)
            .and_then(|(code, _, _)| self.program.codes.get(usize::from(code)));
        let (file, line) = match at {
            Some(c) => (self.program.files.get(usize::from(c.file)).cloned(), c.line),
            None => (None, 0),
        };
        ScriptError {
            file: file.unwrap_or_else(|| Arc::from("<host>")),
            line,
            msg,
            task: None,
        }
    }

    /// Starts a held block as a new task; it runs in the next `step`.
    /// Returns its task id. A failure is queued as a runtime error.
    pub fn fire(&mut self, block: Value, args: &[Value]) -> Option<u64> {
        let mut t = match self.new_task() {
            Ok(t) => t,
            Err(e) => {
                let e = self.host_error(block, e);
                self.push_error(e);
                return None;
            }
        };
        let program = Arc::clone(&self.program);
        if let Err(e) = self.push_block(&program, &mut t, block, args) {
            self.live_tasks = self.live_tasks.saturating_sub(1);
            let e = self.host_error(block, e);
            self.push_error(e);
            return None;
        }
        let id = t.id;
        self.tasks.push_back(t);
        Some(id)
    }

    /// Frees what nothing reaches. Roots are the constants, the top-level
    /// variables, every task and whatever the host shows. Call it between
    /// steps only; `step` does so itself when the heap has grown.
    pub fn collect(&mut self, host: &impl Host) {
        let (program, tasks, top) = (&self.program, &self.tasks, self.top_env);
        self.heap.collect(|visit| {
            program.consts.iter().for_each(|&v| visit(v));
            visit(Value::Ref(top));
            for t in tasks {
                t.stack.iter().for_each(|&v| visit(v));
                t.frames.iter().for_each(|f| visit(Value::Ref(f.env)));
            }
            host.roots(visit);
        });
        self.gc_at = self
            .heap
            .bytes()
            .saturating_mul(2)
            .clamp(MIN_GC_BYTES, crate::heap::MAX_HEAP_BYTES / 2);
    }

    pub(crate) fn maybe_collect(&mut self, host: &impl Host) {
        if self.heap.bytes() >= self.gc_at {
            self.collect(host);
        }
    }

    pub(crate) fn alloc_env(
        &mut self,
        parent: Option<Ref>,
        vars: Vec<Value>,
    ) -> Result<Ref, String> {
        self.heap.alloc(Obj::Env { parent, vars })
    }
}
