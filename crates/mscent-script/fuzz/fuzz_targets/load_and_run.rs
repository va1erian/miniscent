//! Feeds arbitrary bytes to the compiler as a level script, then runs
//! whatever loads. The only acceptable outcomes are an error or a bounded run.
//!
//! From this directory, with the nightly toolchain, seeding from the test scripts:
//! `cargo +nightly fuzz run load_and_run corpus/load_and_run ../tests/scripts`
//! On Windows the Visual Studio ASAN runtime must be on `PATH`
//! (`clang_rt.asan_dynamic-x86_64.dll`, under `VC/Tools/MSVC/*/bin/Hostx64/x64`).
#![no_main]

use libfuzzer_sys::fuzz_target;
use mscent_script::{Flow, Heap, Host, Native, Source, Value, Vm};

#[derive(Default)]
struct Fuzzy {
    held: Vec<Value>,
    said: usize,
}

impl Host for Fuzzy {
    fn roots(&self, visit: &mut dyn FnMut(Value)) {
        self.held.iter().for_each(|v| visit(*v));
    }
}

fn natives() -> Vec<(&'static str, Native<Fuzzy>)> {
    vec![
        ("say:", |h, heap, _, a| {
            h.said += heap.text(a.first().copied().unwrap_or(Value::Nil)).len();
            Ok(Flow::Val(Value::Nil))
        }),
        ("hold:", |h, _, _, a| {
            h.held.extend(a.iter().copied());
            Ok(Flow::Val(Value::Nil))
        }),
        ("park:", |_, _, _, a| match a.first() {
            Some(Value::Num(n)) => Ok(Flow::Wait(*n)),
            _ => Err("park: wants a number".into()),
        }),
        ("make:", |_, heap: &mut Heap, _, a| {
            heap.new_array(a.to_vec()).map(Flow::Val)
        }),
    ]
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let sources = [Source {
        name: "fuzz.st",
        text: &text,
    }];
    let Ok(loaded) = Vm::load(&sources, &natives(), &[]) else {
        return;
    };
    let (mut vm, mut host) = (loaded.vm, Fuzzy::default());
    vm.start(&mut host);
    for block in host.held.clone() {
        let _ = vm.fire(block, &[]);
    }
    for _ in 0..40 {
        vm.step(&mut host);
    }
    let _ = vm.take_errors();
});
