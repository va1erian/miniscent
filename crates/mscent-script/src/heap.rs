//! Values and the garbage-collected heap: a slot table with generation-tagged
//! references, counted in bytes, collected by precise mark and sweep.

/// Heap limit in bytes, live data and uncollected garbage together.
///
/// Collection only happens between steps, so one step can allocate up to its
/// whole budget (16 runs of 20,000 operations) before anything is freed. A
/// legitimate step allocates a few kilobytes of environments and strings. The
/// worst hostile loop, `s := s , s`, allocates about 1.4 KB per operation and
/// would reach 450 MB in a step if nothing stopped it. This limit stops it at
/// 4 MiB, far above any real level and small enough that collecting a full
/// heap of small objects costs about a millisecond.
pub const MAX_HEAP_BYTES: usize = 4 << 20;
/// Longest string a script can build, in bytes.
pub const MAX_STR_LEN: usize = 4096;
/// Most items an array can hold.
pub const MAX_ARRAY_LEN: usize = 4096;

/// Bookkeeping charged to every object, standing in for its slot, generation
/// and allocation header.
const OBJ_OVERHEAD: usize = 32;
/// Bytes charged per array item or variable slot. A `Value` is 12 bytes; the
/// rest covers a `Vec` that grew by doubling.
const ITEM_BYTES: usize = 16;

/// A checked reference to a heap object. The generation changes whenever the
/// slot is freed, so a reference that outlived its object reads nothing
/// instead of whatever lives there now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ref {
    index: u32,
    generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Num(f32),
    Ref(Ref),
    /// A host handle: the kind picks its meaning, the id its identity.
    Handle(u8, u32),
}

#[derive(Clone, Debug)]
pub(crate) enum Obj {
    Str(String),
    Array(Vec<Value>),
    Env {
        parent: Option<Ref>,
        vars: Vec<Value>,
    },
    Block {
        code: u16,
        env: Ref,
        home: u64,
    },
}

impl Obj {
    fn bytes(&self) -> usize {
        let payload = match self {
            Obj::Str(s) => s.len(),
            Obj::Array(v) => v.len().saturating_mul(ITEM_BYTES),
            Obj::Env { vars, .. } => vars.len().saturating_mul(ITEM_BYTES),
            Obj::Block { .. } => 0,
        };
        OBJ_OVERHEAD.saturating_add(payload)
    }
}

#[derive(Clone, Debug)]
struct Slot {
    generation: u32,
    obj: Option<Obj>,
}

#[derive(Clone, Debug, Default)]
pub struct Heap {
    slots: Vec<Slot>,
    free: Vec<u32>,
    bytes: usize,
}

fn out_of_memory() -> String {
    format!("out of script memory (limit {} MiB)", MAX_HEAP_BYTES >> 20)
}

impl Heap {
    /// Bytes charged to live objects and garbage not yet collected.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Objects alive in the table, garbage included until the next collection.
    pub fn live_objects(&self) -> usize {
        self.slots.iter().filter(|s| s.obj.is_some()).count()
    }

    pub(crate) fn alloc(&mut self, obj: Obj) -> Result<Ref, String> {
        let size = obj.bytes();
        if self.bytes.saturating_add(size) > MAX_HEAP_BYTES {
            return Err(out_of_memory());
        }
        if let Some(index) = self.free.pop()
            && let Some(slot) = self.slots.get_mut(index as usize)
        {
            slot.obj = Some(obj);
            self.bytes += size;
            return Ok(Ref {
                index,
                generation: slot.generation,
            });
        }
        let index = u32::try_from(self.slots.len()).map_err(|_| out_of_memory())?;
        self.slots.push(Slot {
            generation: 0,
            obj: Some(obj),
        });
        self.bytes += size;
        Ok(Ref {
            index,
            generation: 0,
        })
    }

    pub(crate) fn get(&self, r: Ref) -> Option<&Obj> {
        let slot = self.slots.get(r.index as usize)?;
        if slot.generation == r.generation {
            slot.obj.as_ref()
        } else {
            None
        }
    }

    fn get_mut(&mut self, r: Ref) -> Option<&mut Obj> {
        let slot = self.slots.get_mut(r.index as usize)?;
        if slot.generation == r.generation {
            slot.obj.as_mut()
        } else {
            None
        }
    }

    /// The text of a string value, or `None` for anything else, including a
    /// reference whose object has been collected.
    pub fn str(&self, v: Value) -> Option<&str> {
        match v {
            Value::Ref(r) => match self.get(r) {
                Some(Obj::Str(s)) => Some(s),
                _ => None,
            },
            _ => None,
        }
    }

    /// The items of an array value, or `None` for anything else.
    pub fn array(&self, v: Value) -> Option<&[Value]> {
        match v {
            Value::Ref(r) => match self.get(r) {
                Some(Obj::Array(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn new_str(&mut self, s: String) -> Result<Value, String> {
        if s.len() > MAX_STR_LEN {
            return Err(format!("string longer than {MAX_STR_LEN} bytes"));
        }
        Ok(Value::Ref(self.alloc(Obj::Str(s))?))
    }

    pub fn new_array(&mut self, items: Vec<Value>) -> Result<Value, String> {
        if items.len() > MAX_ARRAY_LEN {
            return Err(format!("array longer than {MAX_ARRAY_LEN} items"));
        }
        Ok(Value::Ref(self.alloc(Obj::Array(items))?))
    }

    pub(crate) fn env_parent(&self, r: Ref) -> Option<Ref> {
        match self.get(r) {
            Some(Obj::Env { parent, .. }) => *parent,
            _ => None,
        }
    }

    pub(crate) fn env_get(&self, r: Ref, slot: u16) -> Option<Value> {
        match self.get(r) {
            Some(Obj::Env { vars, .. }) => vars.get(usize::from(slot)).copied(),
            _ => None,
        }
    }

    pub(crate) fn env_set(&mut self, r: Ref, slot: u16, v: Value) -> bool {
        match self.get_mut(r) {
            Some(Obj::Env { vars, .. }) => {
                vars.get_mut(usize::from(slot)).map(|s| *s = v).is_some()
            }
            _ => false,
        }
    }

    /// A block value's code id, defining environment and home frame.
    pub(crate) fn block(&self, v: Value) -> Option<(u16, Ref, u64)> {
        match v {
            Value::Ref(r) => match self.get(r) {
                Some(&Obj::Block { code, env, home }) => Some((code, env, home)),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn array_mut(&mut self, r: Ref) -> Option<&mut Vec<Value>> {
        match self.get_mut(r) {
            Some(Obj::Array(a)) => Some(a),
            _ => None,
        }
    }

    /// Appends to an array, charging the heap for the new item.
    pub(crate) fn array_push(&mut self, r: Ref, v: Value) -> Result<(), String> {
        if self.bytes.saturating_add(ITEM_BYTES) > MAX_HEAP_BYTES {
            return Err(out_of_memory());
        }
        let Some(items) = self.array_mut(r) else {
            return Err("add: sent to something that is not an array".into());
        };
        if items.len() >= MAX_ARRAY_LEN {
            return Err(format!("array longer than {MAX_ARRAY_LEN} items"));
        }
        items.push(v);
        self.bytes += ITEM_BYTES;
        Ok(())
    }

    /// The value as `say:` and `,` show it.
    pub fn text(&self, v: Value) -> String {
        match v {
            Value::Nil => "nil".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) => n.to_string(),
            Value::Handle(k, i) => format!("<{k}:{i}>"),
            Value::Ref(_) => match self.str(v) {
                Some(s) => s.to_string(),
                None => self.kind_name(v).into(),
            },
        }
    }

    /// The value for an error message: strings quoted and shortened.
    pub fn describe(&self, v: Value) -> String {
        match self.str(v) {
            Some(s) => format!("'{}'", crate::error::shorten(s)),
            None => match v {
                Value::Ref(_) => self.kind_name(v).to_string(),
                _ => self.text(v),
            },
        }
    }

    fn kind_name(&self, v: Value) -> &'static str {
        match v {
            Value::Ref(r) => match self.get(r) {
                Some(Obj::Str(_)) => "a string",
                Some(Obj::Array(_)) => "an array",
                Some(Obj::Block { .. }) => "a block",
                Some(Obj::Env { .. }) => "an environment",
                None => "a stale reference",
            },
            _ => "a value",
        }
    }

    /// Mark and sweep. `roots` must show every value held outside the heap.
    /// Only call it where no script is mid-operation, which is between steps.
    pub(crate) fn collect(&mut self, roots: impl FnOnce(&mut dyn FnMut(Value))) {
        let mut marks = vec![false; self.slots.len()];
        let mut work: Vec<Ref> = Vec::new();
        roots(&mut |v| {
            if let Value::Ref(r) = v {
                work.push(r);
            }
        });
        while let Some(r) = work.pop() {
            if self.get(r).is_none() {
                continue;
            }
            let Some(mark) = marks.get_mut(r.index as usize) else {
                continue;
            };
            if std::mem::replace(mark, true) {
                continue;
            }
            if let Some(obj) = self.get(r) {
                trace(obj, &mut work);
            }
        }
        self.sweep(&marks);
    }

    fn sweep(&mut self, marks: &[bool]) {
        let mut bytes = 0usize;
        for (i, slot) in self.slots.iter_mut().enumerate() {
            let Some(obj) = &slot.obj else { continue };
            if marks.get(i).copied().unwrap_or(false) {
                bytes = bytes.saturating_add(obj.bytes());
                continue;
            }
            slot.obj = None;
            // A slot whose generation is used up is retired, not reused, so no
            // stale reference can ever match it again.
            if let Some(next) = slot.generation.checked_add(1)
                && let Ok(index) = u32::try_from(i)
            {
                slot.generation = next;
                self.free.push(index);
            }
        }
        self.bytes = bytes;
    }
}

fn trace(obj: &Obj, work: &mut Vec<Ref>) {
    let mut push = |v: &Value| {
        if let Value::Ref(r) = v {
            work.push(*r);
        }
    };
    match obj {
        Obj::Array(items) => items.iter().for_each(&mut push),
        Obj::Env { vars, .. } => vars.iter().for_each(&mut push),
        Obj::Str(_) | Obj::Block { .. } => {}
    }
    match obj {
        Obj::Env {
            parent: Some(p), ..
        } => work.push(*p),
        Obj::Block { env, .. } => work.push(*env),
        _ => {}
    }
}
