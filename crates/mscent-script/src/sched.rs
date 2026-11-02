//! Starting the level's top-level tasks and running due tasks each step.

use crate::vm::{Frame, Host, RUNS_PER_STEP, Task, Vm};
use std::collections::VecDeque;
use std::sync::Arc;

pub(crate) enum Stop {
    Done,
    Suspended,
    Failed(crate::error::ScriptError),
}

impl<H: Host> Vm<H> {
    /// Starts each source's top level as its own task, in load order, and
    /// runs them until they finish or wait. All share one set of top-level
    /// variables. Returns the task ids, so a host can tell when a test ends.
    pub fn start(&mut self, host: &mut H) -> Vec<u64> {
        let program = Arc::clone(&self.program);
        let mut ids = Vec::new();
        for &code in &program.entries {
            let Ok(mut task) = self.new_task() else { break };
            let id = self.frame_id();
            task.frames.push(Frame {
                code,
                ip: 0,
                env: self.top_env,
                id,
                home: id,
                base: 0,
            });
            ids.push(task.id);
            self.tasks.push_back(task);
        }
        self.run_due(host);
        ids
    }

    /// One game step: advances time, runs due tasks under the op budget, then
    /// collects garbage if the heap has grown.
    pub fn step(&mut self, host: &mut H) {
        self.tick = self.tick.saturating_add(1);
        self.run_due(host);
        self.maybe_collect(host);
    }

    /// Runs up to `RUNS_PER_STEP` due tasks in queue order. A task that was
    /// due but over the cap goes to the front of the queue, so it runs first
    /// next step and none starves. A task forked meanwhile joins the end of
    /// this pass. Rebuilding the queue in one pass keeps a step linear in the
    /// task count, with no removal from the middle.
    fn run_due(&mut self, host: &mut H) {
        let mut pending = std::mem::take(&mut self.tasks);
        let mut kept: VecDeque<Task> = VecDeque::with_capacity(pending.len());
        let mut deferred: VecDeque<Task> = VecDeque::new();
        let mut runs = 0;
        while let Some(mut task) = pending.pop_front() {
            if task.wake > self.tick {
                kept.push_back(task);
            } else if runs == RUNS_PER_STEP {
                deferred.push_back(task);
            } else {
                runs += 1;
                match self.run(&mut task, host) {
                    Stop::Suspended => kept.push_back(task),
                    Stop::Done => self.live_tasks = self.live_tasks.saturating_sub(1),
                    Stop::Failed(e) => {
                        self.live_tasks = self.live_tasks.saturating_sub(1);
                        self.push_error(e);
                    }
                }
                pending.append(&mut self.tasks);
            }
        }
        deferred.append(&mut kept);
        self.tasks = deferred;
    }
}
