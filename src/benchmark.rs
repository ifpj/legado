//! Bounded, import-free execution of the recognized arithmetic operand.
use crate::{
    epoch_ms,
    models::{has, sha, Context, Results},
    re, require, Error, Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{collections::HashSet, time::Instant};
fn failure(e: impl std::fmt::Display) -> Error {
    Error::new("BENCHMARK_ERROR", e.to_string())
}
struct EpochTicker(Option<tokio::task::JoinHandle<()>>);
impl Drop for EpochTicker {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}
pub fn run(
    code: &[u8],
    order: [i32; 4],
    counts: [i32; 4],
    repeats: u32,
    warmup: i32,
) -> Result<Value> {
    require(
        code.len() <= 16384
            && sha(code) == "25c4ff7eb859a44b7e8734f710e43901dea7a720462cea79671d4bd356bdff60",
        "Unknown arithmetic module",
    )?;
    let mut sorted = order;
    sorted.sort();
    require(
        sorted == [0, 1, 2, 3]
            && counts.iter().all(|v| (1..=10_000_000).contains(v))
            && (1..=3).contains(&repeats)
            && (1..=50000).contains(&warmup),
        "Arithmetic limits",
    )?;
    let started = Instant::now();
    let mut config = wasmtime::Config::new();
    crate::cpu::check()?;
    config
        .consume_fuel(true)
        .wasm_simd(true)
        .epoch_interruption(true);
    let engine = wasmtime::Engine::new(&config).map_err(failure)?;
    let module = wasmtime::Module::new(&engine, code).map_err(failure)?;
    require(
        module.imports().next().is_none()
            && module
                .exports()
                .map(|e| e.name().to_string())
                .collect::<HashSet<_>>()
                == HashSet::from(["memory", "a", "b", "c", "d"].map(String::from)),
        "Arithmetic host interfaces forbidden",
    )?;
    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(1024 * 1024)
        .instances(1)
        .tables(0)
        .memories(1)
        .build();
    let mut store = wasmtime::Store::new(&engine, limits);
    let _ticker = EpochTicker(tokio::runtime::Handle::try_current().ok().map(|runtime| {
        let engine = engine.clone();
        runtime.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                engine.increment_epoch();
            }
        })
    }));
    store.set_epoch_deadline(1);
    store.epoch_deadline_callback(|_| {
        crate::cpu::check().map_err(|error| wasmtime::Error::msg(error.to_string()))?;
        Ok(wasmtime::UpdateDeadline::Continue(1))
    });
    store.limiter(|limits| limits);
    store.set_fuel(2_000_000_000).map_err(failure)?;
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).map_err(failure)?;
    let compiled = started.elapsed().as_secs_f64() * 1000.;
    let mut samples: Vec<Vec<f64>> = vec![vec![]; 4];
    let timed = Instant::now();
    for _ in 0..repeats {
        for (index, function) in order.iter().enumerate() {
            crate::cpu::check()?;
            let name = ["a", "b", "c", "d"][*function as usize];
            let call = instance
                .get_func(&mut store, name)
                .ok_or_else(|| failure("Missing arithmetic function"))?;
            let mut output = [wasmtime::Val::I32(0)];
            call.call(
                &mut store,
                &[wasmtime::Val::I32(counts[index].min(warmup))],
                &mut output,
            )
            .map_err(failure)?;
            let entered = Instant::now();
            call.call(
                &mut store,
                &[wasmtime::Val::I32(counts[index])],
                &mut output,
            )
            .map_err(failure)?;
            samples[index].push(entered.elapsed().as_secs_f64() * 1000.);
            require(
                matches!(output[0], wasmtime::Val::I32(_) | wasmtime::Val::I64(_))
                    || output[0].f64().is_some_and(f64::is_finite)
                    || output[0].f32().is_some_and(f32::is_finite),
                "Invalid arithmetic output",
            )?;
        }
    }
    Ok(
        json!({"jpZy6":order,"QaoMZ6":counts,"Czvlo8":true,"BvSgL3":samples,"kFiz5":compiled,"YHqsV6":timed.elapsed().as_secs_f64()*1000.}),
    )
}
pub fn calculate(c: &Context<'_>, results: &mut Results) -> Result<()> {
    for m in &c.catalog.modules {
        crate::cpu::check()?;
        let entered = epoch_ms();
        let literals = c.response.between(m.start, m.end);
        let values: HashSet<_> = literals.iter().map(|l| l.value.as_str()).collect();
        if !has(&values, &["BvSgL3", "kFiz5", "YHqsV6", "jpZy6:"]) {
            continue;
        }
        let mut codes = HashSet::new();
        let mut repeats = HashSet::new();
        let mut warmups = HashSet::new();
        for value in &values {
            for capture in re(r#"[`"'](AGFzbQ[A-Za-z0-9+/=]{100,22000})[`"']"#).captures_iter(value)
            {
                if let Ok(code) = STANDARD.decode(&capture[1]) {
                    codes.insert(code);
                }
            }
            if let Some(v) = re(r"^dHbG9:([0-9]+),$").captures(value) {
                repeats.insert(v[1].parse::<u32>().unwrap_or(0));
            }
            if let Some(v) = re(r"^QWXZH5:([0-9]+)};$").captures(value) {
                warmups.insert(v[1].parse::<i32>().unwrap_or(0));
            }
        }
        let mut orders = HashSet::new();
        let mut counts = HashSet::new();
        for (_, value) in c
            .response
            .packed
            .iter()
            .filter(|(p, _)| m.start <= *p && *p < m.end)
        {
            if let Ok(a) = serde_json::from_str::<[i32; 4]>(value) {
                let mut sorted = a;
                sorted.sort();
                if sorted == [0, 1, 2, 3] {
                    orders.insert(a);
                } else if a.iter().all(|n| (1..=10_000_000).contains(n)) {
                    counts.insert(a);
                }
            }
        }
        require(
            codes.len() == 1
                && repeats.len() == 1
                && warmups.len() == 1
                && orders.len() == 1
                && counts.len() == 1,
            "Ambiguous arithmetic inputs",
        )?;
        let fields = run(
            &codes.into_iter().next().unwrap(),
            orders.into_iter().next().unwrap(),
            counts.into_iter().next().unwrap(),
            repeats.into_iter().next().unwrap(),
            warmups.into_iter().next().unwrap(),
        )?;
        results.insert(&m.id, "native-arithmetic-benchmark", fields, entered);
    }
    Ok(())
}
