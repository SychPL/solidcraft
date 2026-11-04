
// Fork only, never in a pull request: the scenes of DevonHope's review of #2457, timed.
#[test]
#[ignore]
fn zz_review_timings() {
    let o = Options::default();
    let time = |name: &str, b: &Brep, scene: &[Placement], view: &View| {
        let t = std::time::Instant::now();
        let def = prepare(b, &o).unwrap();
        let p = t.elapsed();
        let d = hidden_lines(&[def], scene, view, &o, &never).unwrap();
        eprintln!(
            "TIMING {name}: prepare {p:.2?}, hidden_lines {:.2?} (phases ms {:?}), tracks {}, pieces {}, crossings {}, rays {}, uncertain uncovered {}",
            t.elapsed() - p,
            d.stats.phase_ms,
            d.stats.tracks,
            d.stats.pieces,
            d.stats.crossings,
            d.stats.rays,
            d.stats.uncertain_uncovered
        );
    };
    let iso = View::new(Vec3::new(-1., -0.8, -0.6), Vec3::new(0., 0., 1.)).unwrap();
    time("plate with 16x16 holes, iso", &holed_plate(16, 10., 3., 5.), &[identity(0, [0.; 3])], &iso);
    let cyl = cylinder(4., 10.);
    // 64 cylinders one behind the other along the line of sight: every circle and outline of
    // one falls on the others'.
    let d = Vec3::new(-1., -0.3, -0.5);
    let along: Vec<Placement> = (0..64).map(|k| identity(0, [-30. * k as f64 * d.x, -30. * k as f64 * d.y, -30. * k as f64 * d.z])).collect();
    time("64 cylinders aligned along the view", &cyl, &along, &View::new(d, Vec3::new(0., 0., 1.)).unwrap());
    let stacked: Vec<Placement> = (0..64).map(|k| identity(0, [0., 0., 15. * k as f64])).collect();
    time("64 coaxial cylinders, top view", &cyl, &stacked, &View::new(Vec3::new(0., 0., -1.), Vec3::new(0., 1., 0.)).unwrap());
    let boxes: Vec<Placement> = (0..300).map(|k| identity(0, [4. * (k % 20) as f64, 4. * (k / 20) as f64, 0.])).collect();
    time("300 boxes", &cuboid([0.; 3], [1.; 3]), &boxes, &View::new(Vec3::new(0.3, 0.2, -1.), Vec3::new(0., 1., 0.)).unwrap());
}
