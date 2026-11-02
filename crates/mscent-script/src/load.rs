//! Loading: parse every source, check names, compile, and build the VM.

use crate::ast::Program as Ast;
use crate::builtin::BUILTINS;
use crate::check::{Known, MethodMeta};
use crate::code::{Impl, Program};
use crate::compile::{Compiler, ConstVal};
use crate::error::{Diag, MAX_LOAD_ERRORS, ScriptError, shorten};
use crate::heap::{Heap, Obj, Value};
use crate::parse::parse;
use crate::prelude;
use crate::vm::{Host, Native, Vm};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// A named piece of script text. The name is what errors call the file.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

/// The level names a selector's first argument may be, when it is a string
/// literal. A typo then fails at load even inside a handler that never runs.
#[derive(Clone, Copy, Debug)]
pub struct NameCheck<'a> {
    pub selector: &'a str,
    pub names: &'a [&'a str],
}

/// A VM that loaded, and what the compiler wants the author to look at.
pub struct Loaded<H> {
    pub vm: Vm<H>,
    pub warnings: Vec<ScriptError>,
}

fn host_error(msg: String) -> ScriptError {
    ScriptError {
        file: Arc::from("<host>"),
        line: 0,
        msg,
        task: None,
    }
}

struct Parsed {
    file: u16,
    ast: Ast,
}

impl<H: Host> Vm<H> {
    /// Loads the language's prelude, then `sources` in order: library files,
    /// then the level, then optionally a test. Each source's top level
    /// becomes its own task for `start`. Returns every error found, or the
    /// VM and its warnings.
    pub fn load(
        sources: &[Source],
        natives: &[(&str, Native<H>)],
        names: &[NameCheck],
    ) -> Result<Loaded<H>, Vec<ScriptError>> {
        let all: Vec<Source> = std::iter::once(Source {
            name: prelude::NAME,
            text: prelude::SOURCE,
        })
        .chain(sources.iter().copied())
        .collect();
        let files: Vec<Arc<str>> = all.iter().map(|s| Arc::from(s.name)).collect();
        if files.len() > usize::from(u16::MAX) {
            return Err(vec![host_error(format!(
                "too many source files: the limit is {}",
                u16::MAX
            ))]);
        }
        let mut errors = Vec::new();
        let parsed = parse_all(&all, &files, &mut errors);
        let native_map = native_table(natives, &mut errors);
        stop_if(&errors)?;
        let known = known_names(&parsed, &native_map, &files, &mut errors);
        stop_if(&errors)?;
        let name_map: HashMap<&str, &[&str]> =
            names.iter().map(|n| (n.selector, n.names)).collect();
        let mut compiler = Compiler::new(&known, name_map);
        let inputs: Vec<(u16, &[_], &[_])> = parsed
            .iter()
            .map(|p| (p.file, p.ast.top.as_slice(), p.ast.defines.as_slice()))
            .collect();
        if let Err(d) = compiler.program(&inputs) {
            errors.push(d.resolve(&files));
        }
        compiler.soft.sort_by_key(|d| (d.file, d.line));
        errors.extend(compiler.soft.drain(..).map(|d| d.resolve(&files)));
        errors.truncate(MAX_LOAD_ERRORS);
        stop_if(&errors)?;
        let mut warnings: Vec<Diag> = std::mem::take(&mut compiler.warnings);
        warnings.sort_by_key(|d| (d.file, d.line));
        let warnings = warnings.into_iter().map(|d| d.resolve(&files)).collect();
        let vm = build(compiler, &native_map, natives, files).map_err(|e| vec![e])?;
        Ok(Loaded { vm, warnings })
    }
}

fn stop_if(errors: &[ScriptError]) -> Result<(), Vec<ScriptError>> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.iter().take(MAX_LOAD_ERRORS).cloned().collect())
    }
}

fn parse_all(all: &[Source], files: &[Arc<str>], errors: &mut Vec<ScriptError>) -> Vec<Parsed> {
    let mut out = Vec::new();
    for (i, src) in all.iter().enumerate() {
        let Ok(file) = u16::try_from(i) else { break };
        match parse(src.text) {
            Ok(ast) => out.push(Parsed { file, ast }),
            Err(e) => errors.push(Diag::new(file, e.line, e.msg).resolve(files)),
        }
    }
    out
}

fn native_table<'a, H>(
    natives: &[(&'a str, Native<H>)],
    errors: &mut Vec<ScriptError>,
) -> HashMap<&'a str, Native<H>> {
    let mut map = HashMap::new();
    for &(sel, f) in natives {
        if Known::reserved(sel) {
            errors.push(host_error(format!(
                "native '{sel}' has the name of a language message"
            )));
        } else if map.insert(sel, f).is_some() {
            errors.push(host_error(format!("native '{sel}' is registered twice")));
        }
    }
    map
}

/// Collects every `define`, rejecting a name defined twice or one that
/// collides with a native or a language message.
fn known_names<'a, H>(
    parsed: &'a [Parsed],
    natives: &HashMap<&'a str, Native<H>>,
    files: &[Arc<str>],
    errors: &mut Vec<ScriptError>,
) -> Known<'a> {
    let mut methods: HashMap<&str, MethodMeta> = HashMap::new();
    for p in parsed {
        for d in &p.ast.defines {
            let sel = d.sel.as_str();
            let problem = if Known::reserved(sel) {
                Some(format!(
                    "'{}' is a message the language provides, so it cannot be defined",
                    shorten(sel)
                ))
            } else if natives.contains_key(sel) {
                Some(format!(
                    "'{}' is a game function, so it cannot be defined",
                    shorten(sel)
                ))
            } else {
                methods.get(sel).map(|m| {
                    let other = files.get(usize::from(m.file)).map_or("?", |f| f);
                    format!(
                        "'{}' is already defined at {other}:{}",
                        shorten(sel),
                        m.line
                    )
                })
            };
            match problem {
                Some(msg) if errors.len() < MAX_LOAD_ERRORS => {
                    errors.push(Diag::new(p.file, d.line, msg).resolve(files))
                }
                Some(_) => {}
                None => {
                    methods.insert(
                        sel,
                        MethodMeta {
                            recv: d.recv.is_some(),
                            file: p.file,
                            line: d.line,
                        },
                    );
                }
            }
        }
    }
    Known {
        natives: natives.keys().copied().collect::<HashSet<_>>(),
        methods,
    }
}

fn resolve_impl<H>(sel: &str, c: &Compiler, natives: &HashMap<&str, Native<H>>) -> Option<Impl<H>> {
    if let Some(&code) = c.method_code.get(sel) {
        return Some(Impl::Method(code));
    }
    if let Some(&f) = natives.get(sel) {
        return Some(Impl::Native(f));
    }
    if let Some(b) = BUILTINS.iter().find(|b| b.0 == sel) {
        return Some(Impl::Builtin(b.1));
    }
    match sel {
        "value" | "value:" | "value:value:" => Some(Impl::BlockValue),
        "fork:" => Some(Impl::Fork),
        "wait:" => Some(Impl::Wait),
        _ => None,
    }
}

fn build<H>(
    c: Compiler,
    natives: &HashMap<&str, Native<H>>,
    table: &[(&str, Native<H>)],
    files: Vec<Arc<str>>,
) -> Result<Vm<H>, ScriptError> {
    let mut impls = Vec::with_capacity(c.sels.len());
    for sel in &c.sels {
        let imp = resolve_impl(sel, &c, natives)
            .ok_or_else(|| host_error(format!("no implementation for '{}'", shorten(sel))))?;
        impls.push(imp);
    }
    let mut heap = Heap::default();
    let mut consts = Vec::with_capacity(c.consts.len());
    for k in &c.consts {
        consts.push(match k {
            ConstVal::Num(n) => Value::Num(*n),
            ConstVal::Str(s) => heap.new_str(s.clone()).map_err(host_error)?,
        });
    }
    let top = heap
        .alloc(Obj::Env {
            parent: None,
            vars: vec![Value::Nil; c.top_slots],
        })
        .map_err(host_error)?;
    let program = Program {
        codes: c.codes,
        consts,
        impls,
        files,
        entries: c.entries,
        native_names: table.iter().map(|(n, _)| (*n).to_string()).collect(),
    };
    Ok(Vm::new(program, heap, top))
}
