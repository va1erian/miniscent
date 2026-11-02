//! Seeded byte-mutation fuzz over real scripts, as an ordinary test. The seed
//! is fixed so a failure replays; the same loop is the corpus-free stand-in for
//! the cargo fuzz target in `fuzz/`.

mod common;
use common::*;
use mscent_script::{MAX_HEAP_BYTES, MAX_TASKS, Value};

/// xorshift64*, written here because this crate depends on nothing, not even
/// the workspace's own math crate.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const TOKENS: &[&str] = &[
    "[",
    "]",
    "(",
    ")",
    ":",
    ".",
    "^",
    "|",
    "'",
    "\"",
    "{",
    "}",
    ",",
    "-",
    "+",
    "=",
    "<",
    " ",
    "\n",
    "x",
    "say:",
    "wait:",
    "fork:",
    "define",
    ":=",
    "0",
    "1e9",
    "ifTrue:",
    "whileTrue:",
    "value",
    "nil",
    "z",
    "9999999999",
];

fn mutate(rng: &mut Rng, source: &str) -> String {
    let mut bytes = source.as_bytes().to_vec();
    for _ in 0..=rng.below(6) {
        let at = rng.below(bytes.len().max(1));
        match rng.below(5) {
            0 => {
                if let Some(b) = bytes.get_mut(at) {
                    *b = rng.next().to_le_bytes()[0] % 127;
                }
            }
            1 if !bytes.is_empty() => drop(bytes.remove(at)),
            2 => {
                let t = TOKENS[rng.below(TOKENS.len())].as_bytes();
                bytes.splice(at.min(bytes.len())..at.min(bytes.len()), t.iter().copied());
            }
            3 => {
                let end = (at + rng.below(40)).min(bytes.len());
                let copy = bytes.get(at..end).unwrap_or(&[]).to_vec();
                bytes.splice(at..at, copy);
            }
            _ => bytes.truncate(at),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Returns (loaded, runtime errors) for one mutant.
fn exercise(src: &str, lib: &str, rounds: u64) -> Option<usize> {
    let loaded = load(&[("lib.st", lib), ("level.st", src)]).ok()?;
    let (mut vm, mut host) = (
        loaded.vm,
        Stub {
            alive: 1.0,
            ..Stub::default()
        },
    );
    vm.start(&mut host);
    let held: Vec<Value> = host.handlers.iter().map(|h| h.1).collect();
    for b in held {
        let _ = vm.fire(b, &[]);
    }
    for t in 1..=rounds {
        host.tick = t;
        vm.step(&mut host);
        assert!(vm.task_count() <= MAX_TASKS && vm.heap().bytes() <= MAX_HEAP_BYTES);
    }
    Some(vm.take_errors().len())
}

#[test]
fn mutated_real_scripts_never_panic_and_never_break_a_limit() {
    let lib = format!("{}\n{}", script("lib_wait"), script("lib_doors"));
    let names = [
        "switches",
        "keycard",
        "movers",
        "movers2",
        "ambush",
        "terminal",
        "terminal2",
        "vault_test",
    ];
    let sources: Vec<String> = names.iter().map(|n| script(n)).collect();
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let (mut loaded, mut rejected, mut failed_at_runtime) = (0, 0, 0);
    for i in 0..40_000 {
        let src = mutate(&mut rng, &sources[i % sources.len()]);
        match exercise(&src, &lib, 30) {
            None => rejected += 1,
            Some(errors) => {
                loaded += 1;
                failed_at_runtime += usize::from(errors > 0);
            }
        }
    }
    println!("fuzz: {loaded} loaded, {rejected} rejected, {failed_at_runtime} with runtime errors");
    assert!(
        loaded > 1000 && rejected > 1000,
        "the mutations must reach both outcomes: {loaded} loaded, {rejected} rejected"
    );
}

#[test]
fn random_token_soup_never_panics() {
    let mut rng = Rng(0xDEAD_BEEF_0000_0001);
    for _ in 0..20_000 {
        let len = rng.below(60);
        let soup: String = (0..len)
            .map(|_| TOKENS[rng.below(TOKENS.len())])
            .collect::<Vec<_>>()
            .join(" ");
        let _ = exercise(&soup, "", 5);
    }
}
