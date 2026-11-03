//! Fork-only (copied into crates/engine/tests/ by .github/workflows/fork-11483.yml): what the
//! Interference dialog does after Compute (tangency-labs/solidcraft#11483), on a thread with a
//! given stack: intersect the two parts in world placement and mesh the shared volume. The
//! report's design and replay come from FORK_11483_CRASH. One test per stack size, so a stack
//! overflow ends only its own process.

use serde_json::{Value, json};
use solidcraft_engine::{Session, kernel};

fn opened() -> Session {
    let crash = std::env::var("FORK_11483_CRASH").unwrap();
    let mut s = Session::default();
    s.execute("doc.open", &json!({"path": format!("{crash}/design.sc")})).unwrap();
    let replay: Value = serde_json::from_str(&std::fs::read_to_string(format!("{crash}/replay.json")).unwrap()).unwrap();
    if std::env::var("FORK_11483_REACH").is_ok() {
        for c in replay["commands"].as_array().unwrap() {
            if c["command"] == "joint.reach" {
                s.execute("joint.reach", &c["params"]).unwrap();
            }
        }
    }
    s
}

fn parts() -> (kernel::Body, kernel::Body) {
    // Open on a big stack: the app opens a design off the window thread.
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let s = opened();
            let st = WORLD;
            (st.body("Cut-Extrude2").unwrap().body.clone(), st.body("Cut-Extrude12").unwrap().body.clone())
        })
        .unwrap()
        .join()
        .unwrap()
}

fn on_stack(kib: usize) {
    let (a, b) = parts();
    eprintln!("fork-11483: intersecting on a {kib} KiB stack");
    let r = std::thread::Builder::new()
        .stack_size(kib << 10)
        .spawn(move || {
            let t0 = std::time::Instant::now();
            let o = kernel::boolean(&a, &b, kernel::BoolOp::Intersect);
            let boolean_ms = t0.elapsed().as_secs_f64() * 1e3;
            let o = match o {
                Ok(Some(o)) => o,
                other => return format!("boolean: {:?} in {boolean_ms:.0} ms", other.map(|o| o.is_some())),
            };
            let t1 = std::time::Instant::now();
            let meshed = MESH.is_ok();
            format!("boolean ok in {boolean_ms:.0} ms (size {:.3}), mesh ok: {meshed} in {:.0} ms", o.size(), t1.elapsed().as_secs_f64() * 1e3)
        })
        .unwrap()
        .join();
    eprintln!("fork-11483: {kib} KiB: {r:?}");
}

#[test]
fn stack_1024_kib() {
    on_stack(1024);
}

#[test]
fn stack_2048_kib() {
    on_stack(2048);
}

#[test]
fn stack_8192_kib() {
    on_stack(8192);
}
