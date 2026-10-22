//! Fork-only (never upstream): reopen times of a large design whose model cache is whole, thinned
//! to fractions of its size, at its smallest, and left out (#3220).

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::doc::SavedModel;
use std::time::Instant;

#[test]
#[ignore]
fn reopen_times() {
    let holes: usize = std::env::var("HOLES").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 400, "width": 300, "height": 20, "body_name": "Plate"})).unwrap();
    s.execute("solid.fillet", &json!({"edges": [[0, 0, 10], [400, 0, 10], [400, 300, 10], [0, 300, 10]], "radius": "12 mm"})).unwrap();
    for i in 0..holes {
        let (x, y) = (25.0 + 30.0 * (i % 12) as f64, 25.0 + 30.0 * (i / 12) as f64);
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.circle.center", &json!({"center": [x, y], "diameter": 6 + i % 7})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        s.execute("solid.extrude", &json!({"distance": 20, "operation": "cut"})).unwrap();
    }
    assert!(s.model.results.iter().all(|r| r.error.is_none()));
    let n = s.doc.features.len();
    let design = s.design();
    let saved = s.model.saved(&design).unwrap();
    let sizes: Vec<usize> = saved.bodies.iter().map(|b| solidcraft_engine::kernel::encode_native_body(b).unwrap().len()).collect();
    let pieces: Vec<Vec<(usize, usize)>> = sizes.iter().enumerate().map(|(i, n)| vec![(i, *n)]).collect();
    let total: usize = sizes.iter().sum();
    // The smallest budget that keeps a model (the final state's).
    let (mut lo, mut hi) = (0, total);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if saved.thinned(&pieces, mid).is_some() { hi = mid } else { lo = mid + 1 }
    }
    println!("{}", json!({"design": "plate", "features": n, "bodies stored": saved.bodies.len(), "payload": total, "final": lo}));
    let mut cases: Vec<(String, Option<SavedModel>)> = vec![("none (before #3220: the cache left out)".into(), None)];
    for (name, budget) in [("whole", total), ("50%", total / 2), ("25%", total / 4), ("10%", total / 10), ("smallest", lo)] {
        cases.push((name.into(), saved.thinned(&pieces, budget.max(lo)).map(|(m, _)| m)));
    }
    let dir = std::env::temp_dir();
    for (name, model) in &cases {
        let bytes = solidcraft_engine::io::write_design_container_with_cache(&design, solidcraft_engine::io::DesignCompression::Fast, None, model.as_ref()).unwrap();
        let path = dir.join(format!("reopen-{}.solidcraft", name.split(' ').next().unwrap_or("x").replace('%', "pct")));
        std::fs::write(&path, &bytes).unwrap();
        let kept = model.as_ref().map_or(0, |m| m.records["features"].as_array().unwrap().iter().filter(|f| f.get("state").is_some()).count());
        let mut times = Vec::new();
        let mut recomputed = 0;
        for _ in 0..3 {
            let t = Instant::now();
            let (doc, saved) = solidcraft_engine::io::read_design_file(path.to_str().unwrap(), None::<fn(solidcraft_engine::io::DesignPreview)>).unwrap();
            let opened = Session::opened(doc, saved.as_ref());
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            recomputed = opened.model.last_recomputed;
            assert!(opened.model.results.iter().all(|r| r.error.is_none()));
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{}",
            json!({"cache": name, "file_bytes": bytes.len(), "features_with_state": kept, "features": n, "evaluated_on_open": recomputed, "open_ms_median": times[1].round(), "open_ms_min": times[0].round()})
        );
        let _ = std::fs::remove_file(&path);
    }
}
