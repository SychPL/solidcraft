//! Measurement probe for #1221: compiles and instantiates a component with the host
//! configuration docs/plugin-interface.md proposes (component model, async, epoch
//! interruption, closed WASI p2 context, the `command-plugin` imports) and reports times.
//!
//! Measurement branch only, not for merging.

use std::time::{Duration, Instant};

wasmtime::component::bindgen!({
    path: "wit",
    world: "command-plugin",
    exports: { default: async },
});

use solidcraft::plugin::types::{Json, PluginError};

struct Ctx {
    wasi: wasmtime_wasi::WasiCtx,
    table: wasmtime::component::ResourceTable,
}

impl wasmtime_wasi::WasiView for Ctx {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

impl solidcraft::plugin::types::Host for Ctx {}
impl solidcraft::plugin::engine::Host for Ctx {
    fn run(&mut self, command: String, _args: Json) -> Result<Json, PluginError> {
        Err(PluginError::Denied(format!("probe host has no engine ({command})")))
    }
}
impl solidcraft::plugin::log::Host for Ctx {
    fn log(&mut self, _level: solidcraft::plugin::log::Level, _message: String) {}
}
impl solidcraft::plugin::store::Host for Ctx {
    fn get(&mut self, _key: String) -> Result<Option<Vec<u8>>, PluginError> {
        Ok(None)
    }
    fn set(&mut self, _key: String, _value: Vec<u8>) -> Result<(), PluginError> {
        Ok(())
    }
    fn remove(&mut self, _key: String) -> Result<(), PluginError> {
        Ok(())
    }
    fn keys(&mut self) -> Result<Vec<String>, PluginError> {
        Ok(Vec::new())
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// `solidcraft --plugin-probe <component.wasm> [cranelift|winch] [call]` (measurement build only).
pub fn run(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("usage: --plugin-probe <component.wasm> [cranelift|winch] [call]")?;
    let strategy = args.get(1).map(String::as_str).unwrap_or("cranelift");
    let call = args.get(2).is_some_and(|a| a == "call");

    let mut config = wasmtime::Config::new();
    config.epoch_interruption(true);
    config.max_wasm_stack(512 * 1024);
    config.async_stack_size(512 * 1024 + 2 * 1024 * 1024);
    config.strategy(match strategy {
        "winch" => wasmtime::Strategy::Winch,
        _ => wasmtime::Strategy::Cranelift,
    });
    let engine = wasmtime::Engine::new(&config).map_err(|e| e.to_string())?;

    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let t = Instant::now();
    let component = wasmtime::component::Component::new(&engine, &bytes).map_err(|e| e.to_string())?;
    let compile = t.elapsed();

    let mut linker = wasmtime::component::Linker::<Ctx>::new(&engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|e| e.to_string())?;
    CommandPlugin::add_to_linker::<Ctx, wasmtime::component::HasSelf<Ctx>>(&mut linker, |c| c).map_err(|e| e.to_string())?;

    let wasi = wasmtime_wasi::WasiCtxBuilder::new().build();
    let mut store = wasmtime::Store::new(&engine, Ctx { wasi, table: wasmtime::component::ResourceTable::new() });
    store.set_epoch_deadline(u64::MAX / 2);

    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().map_err(|e| e.to_string())?;
    let t = Instant::now();
    let plugin = rt.block_on(CommandPlugin::instantiate_async(&mut store, &component, &linker)).map_err(|e| e.to_string())?;
    let instantiate = t.elapsed();

    let mut call_ms = None;
    if call {
        let t = Instant::now();
        let out = rt
            .block_on(plugin.solidcraft_plugin_command().call_execute(&mut store, "probe", &"{}".to_string()))
            .map_err(|e| e.to_string())?;
        call_ms = Some(ms(t.elapsed()));
        eprintln!("execute -> {out:?}");
    }

    println!(
        "{{\"file\":\"{}\",\"bytes\":{},\"strategy\":\"{strategy}\",\"compile_ms\":{:.0},\"instantiate_ms\":{:.1},\"call_ms\":{}}}",
        path.replace('\\', "/"),
        bytes.len(),
        ms(compile),
        ms(instantiate),
        call_ms.map_or("null".to_string(), |v| format!("{v:.1}"))
    );
    Ok(())
}
