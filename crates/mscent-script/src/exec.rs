//! The run loop and message sends. No Rust recursion: a call pushes a frame.
//! Nothing here indexes a slice or unwraps; a bytecode that does not make
//! sense, which the compiler never emits, ends the task with an error.

use crate::builtin;
use crate::code::{Impl, Op, Program};
use crate::error::ScriptError;
use crate::heap::{Obj, Value};
use crate::sched::Stop;
use crate::vm::{Flow, Frame, MAX_FRAMES, MAX_STACK, OPS_PER_RUN, STEP_HZ, Task, Vm};
use std::sync::Arc;

enum Next {
    Continue,
    Done,
    Suspend,
}

type Res<T> = Result<T, String>;

const CORRUPT: &str = "corrupt bytecode";

impl<H> Vm<H> {
    pub(crate) fn run(&mut self, t: &mut Task, host: &mut H) -> Stop {
        let program = Arc::clone(&self.program);
        let mut budget = OPS_PER_RUN;
        loop {
            if budget == 0 {
                let msg = format!("this ran over its budget of {OPS_PER_RUN} operations");
                return Stop::Failed(error_at(&program, t, msg));
            }
            budget -= 1;
            self.ops = self.ops.saturating_add(1);
            let step = if t.stack.len() > MAX_STACK {
                Err(format!("value stack deeper than {MAX_STACK}"))
            } else {
                self.step_op(&program, t, host)
            };
            match step {
                Ok(Next::Continue) => {}
                Ok(Next::Done) => return Stop::Done,
                Ok(Next::Suspend) => return Stop::Suspended,
                Err(msg) => return Stop::Failed(error_at(&program, t, msg)),
            }
        }
    }

    fn step_op(&mut self, program: &Program<H>, t: &mut Task, host: &mut H) -> Res<Next> {
        let Some(frame) = t.frames.last_mut() else {
            return Ok(Next::Done);
        };
        let (code_id, ip, env, home) = (frame.code, frame.ip, frame.env, frame.home);
        let code = program.codes.get(usize::from(code_id)).ok_or(CORRUPT)?;
        let Some(&op) = code.ops.get(ip as usize) else {
            return Ok(Next::Done);
        };
        frame.ip = ip.saturating_add(1);
        match op {
            Op::Const(i) => t
                .stack
                .push(*program.consts.get(usize::from(i)).ok_or(CORRUPT)?),
            Op::Nil => t.stack.push(Value::Nil),
            Op::True => t.stack.push(Value::Bool(true)),
            Op::False => t.stack.push(Value::Bool(false)),
            Op::Pop => drop(t.stack.pop()),
            Op::Load(depth, slot) => {
                let at = self.env_at(env, depth)?;
                t.stack.push(self.heap.env_get(at, slot).ok_or(CORRUPT)?);
            }
            Op::Store(depth, slot) => {
                let v = t.stack.last().copied().ok_or(CORRUPT)?;
                let at = self.env_at(env, depth)?;
                if !self.heap.env_set(at, slot, v) {
                    return Err(CORRUPT.into());
                }
            }
            Op::Jump(to) => set_ip(t, to),
            Op::JumpIfFalse(to) => {
                if !pop_bool(self, t)? {
                    set_ip(t, to);
                }
            }
            Op::JumpIfTrue(to) => {
                if pop_bool(self, t)? {
                    set_ip(t, to);
                }
            }
            Op::Block(code) => {
                let r = self.heap.alloc(Obj::Block { code, env, home })?;
                t.stack.push(Value::Ref(r));
            }
            Op::Array(n) => {
                let at = t.stack.len().saturating_sub(usize::from(n));
                let items = t.stack.split_off(at);
                t.stack.push(self.heap.new_array(items)?);
            }
            Op::Ret => return Ok(ret(t)),
            Op::RetHome => return ret_home(t, home),
            Op::EndTask => return Ok(Next::Done),
            Op::Send(sel, argc) => return self.send(program, t, sel, usize::from(argc), host),
        }
        Ok(Next::Continue)
    }

    fn env_at(&self, mut env: crate::heap::Ref, depth: u8) -> Res<crate::heap::Ref> {
        for _ in 0..depth {
            env = self.heap.env_parent(env).ok_or(CORRUPT)?;
        }
        Ok(env)
    }

    /// Pushes a frame running `block` with `args` onto `t`.
    pub(crate) fn push_block(
        &mut self,
        program: &Program<H>,
        t: &mut Task,
        block: Value,
        args: &[Value],
    ) -> Res<()> {
        let Some((code, env, home)) = self.heap.block(block) else {
            return Err(format!(
                "{} does not understand value",
                self.heap.describe(block)
            ));
        };
        let c = program.codes.get(usize::from(code)).ok_or(CORRUPT)?;
        if usize::from(c.nparams) != args.len() {
            return Err(format!(
                "this block takes {} arguments, but got {}",
                c.nparams,
                args.len()
            ));
        }
        let env = self.new_frame_env(Some(env), usize::from(c.nslots), None, args)?;
        let id = self.frame_id();
        t.frames.push(Frame {
            code,
            ip: 0,
            env,
            id,
            home,
            base: t.stack.len(),
        });
        Ok(())
    }

    /// An environment with the receiver (when there is one) and arguments in
    /// its first slots.
    fn new_frame_env(
        &mut self,
        parent: Option<crate::heap::Ref>,
        nslots: usize,
        recv: Option<Value>,
        args: &[Value],
    ) -> Res<crate::heap::Ref> {
        let mut vars = vec![Value::Nil; nslots];
        let given = recv.iter().chain(args.iter());
        for (slot, v) in vars.iter_mut().zip(given) {
            *slot = *v;
        }
        self.alloc_env(parent, vars)
    }

    fn send(
        &mut self,
        program: &Program<H>,
        t: &mut Task,
        sel: u16,
        argc: usize,
        host: &mut H,
    ) -> Res<Next> {
        let start = t.stack.len().checked_sub(argc).ok_or(CORRUPT)?;
        let recv_at = start.checked_sub(1).ok_or(CORRUPT)?;
        let recv = *t.stack.get(recv_at).ok_or(CORRUPT)?;
        let args = t.stack.get(start..).unwrap_or(&[]);
        match program.impls.get(usize::from(sel)).ok_or(CORRUPT)? {
            Impl::Method(code) => {
                check_depth(t)?;
                let c = program.codes.get(usize::from(*code)).ok_or(CORRUPT)?;
                let given = c.has_recv.then_some(recv);
                let env =
                    self.new_frame_env(Some(self.top_env), usize::from(c.nslots), given, args)?;
                t.stack.truncate(recv_at);
                let id = self.frame_id();
                t.frames.push(Frame {
                    code: *code,
                    ip: 0,
                    env,
                    id,
                    home: id,
                    base: recv_at,
                });
            }
            Impl::BlockValue => {
                check_depth(t)?;
                let args = args.to_vec();
                t.stack.truncate(recv_at);
                self.push_block(program, t, recv, &args)?;
            }
            Impl::Fork => {
                let block = args.first().copied().ok_or(CORRUPT)?;
                t.stack.truncate(recv_at);
                let mut task = self.new_task()?;
                if let Err(e) = self.push_block(program, &mut task, block, &[]) {
                    self.live_tasks = self.live_tasks.saturating_sub(1);
                    return Err(e);
                }
                self.tasks.push_back(task);
                t.stack.push(Value::Nil);
            }
            Impl::Wait => {
                let Some(&Value::Num(secs)) = args.first() else {
                    return Err("wait: wants a number of seconds".into());
                };
                t.stack.truncate(recv_at);
                return self.suspend(t, secs);
            }
            Impl::Builtin(b) => {
                let v = builtin::apply(&mut self.heap, *b, recv, args)?;
                t.stack.truncate(recv_at);
                t.stack.push(v);
            }
            Impl::Native(native) => {
                let flow = native(host, &mut self.heap, recv, args)?;
                t.stack.truncate(recv_at);
                match flow {
                    Flow::Val(v) => t.stack.push(v),
                    Flow::Wait(secs) => return self.suspend(t, secs),
                }
            }
        }
        Ok(Next::Continue)
    }

    /// Parks the task for `secs`, resuming with nil. At least one step, and
    /// saturating, so no number a script writes can overflow the clock.
    fn suspend(&mut self, t: &mut Task, secs: f32) -> Res<Next> {
        if !secs.is_finite() || secs < 0.0 {
            return Err("wait: wants zero or more seconds".into());
        }
        // Float to integer casts saturate, so a huge wait is a very long one.
        let steps = ((secs * STEP_HZ).ceil() as u64).max(1);
        t.stack.push(Value::Nil);
        t.wake = self.tick.saturating_add(steps);
        Ok(Next::Suspend)
    }
}

fn set_ip(t: &mut Task, to: u32) {
    if let Some(f) = t.frames.last_mut() {
        f.ip = to;
    }
}

fn check_depth(t: &Task) -> Res<()> {
    if t.frames.len() >= MAX_FRAMES {
        return Err(format!("calls nested more than {MAX_FRAMES} deep"));
    }
    Ok(())
}

fn pop_bool<H>(vm: &Vm<H>, t: &mut Task) -> Res<bool> {
    match t.stack.pop() {
        Some(Value::Bool(b)) => Ok(b),
        Some(v) => Err(format!(
            "expected true or false, got {}",
            vm.heap.describe(v)
        )),
        None => Err(CORRUPT.into()),
    }
}

fn ret(t: &mut Task) -> Next {
    let v = t.stack.pop().unwrap_or(Value::Nil);
    let Some(done) = t.frames.pop() else {
        return Next::Done;
    };
    if t.frames.is_empty() {
        return Next::Done;
    }
    t.stack.truncate(done.base);
    t.stack.push(v);
    Next::Continue
}

/// `^`: unwinds to the frame of the method the block was written in.
fn ret_home(t: &mut Task, home: u64) -> Res<Next> {
    let v = t.stack.pop().unwrap_or(Value::Nil);
    let Some(at) = t.frames.iter().rposition(|f| f.id == home) else {
        return Err(
            "^ cannot return: its method already returned, or it is running in another task".into(),
        );
    };
    let base = t.frames.get(at).map_or(0, |f| f.base);
    t.frames.truncate(at);
    if t.frames.is_empty() {
        return Ok(Next::Done);
    }
    t.stack.truncate(base);
    t.stack.push(v);
    Ok(Next::Continue)
}

/// An error at the op the task just ran: its file and line.
fn error_at<H>(program: &Program<H>, t: &Task, msg: String) -> ScriptError {
    let at = t.frames.last().and_then(|f| {
        let code = program.codes.get(usize::from(f.code))?;
        let line = code
            .lines
            .get((f.ip as usize).saturating_sub(1))
            .copied()
            .unwrap_or(code.line);
        Some((code.file, line))
    });
    let (file, line) = at.unwrap_or((0, 0));
    let file = program
        .files
        .get(usize::from(file))
        .cloned()
        .unwrap_or_else(|| Arc::from("<unknown>"));
    ScriptError {
        file,
        line,
        msg,
        task: Some(t.id),
    }
}
