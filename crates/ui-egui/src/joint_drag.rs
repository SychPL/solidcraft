//! Dragging a part that joints place moves it as far as its joints allow: each frame runs
//! `joint.reach`, taking the point grabbed toward the pointer in the plane through it that faces
//! the view. The whole drag is one undo step.
//!
//! Assemble ▸ Move (or the context menu's Move) on such a part shows handles for only the
//! freedoms its joint allows instead (#6679): an arrow along each value that slides, a ring about
//! each one that turns (`joint.freedoms`). A drag on one runs `joint.drive` frame by frame, so
//! limits, contact sets and closed loops stop it; its readout gives the travel since the drag
//! began. The drag is one undo step, and Escape puts the part back where it began.

use std::sync::Arc;

use egui::Pos2;
use serde_json::{Value, json};
use solidcraft_engine::doc::{Document, apply_point, joints, mat_inverse};
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::gizmo::{self, Change, Handle};
use crate::theme::Tokens;
use crate::viewport::{Hit, Proj};

/// A drag under way.
#[derive(Clone, Debug)]
pub struct Drag {
    occurrence: u64,
    /// The point grabbed, in the occurrence's own coordinates.
    local: Vec3,
    /// The plane the pointer moves the point in: through where it was grabbed, facing the view.
    origin: Vec3,
    normal: Vec3,
    /// The drag's undo step is on top of the history (it went in with the first frame that moved).
    pushed: bool,
}

/// Begin a drag on the part under `p` when joints place it and leave it free to move (it is
/// neither grounded nor fixed to what is). True when one began.
pub fn start(app: &mut SolveApp, proj: &Proj, p: Pos2) -> bool {
    // The face under the pointer, past the part's own edges and vertices (not past sketches or
    // construction geometry in front of it).
    let hits = crate::viewport::pick(app, proj, p);
    let Some((body, at)) =
        hits.into_iter().take_while(|h| matches!(h, Hit::Face { .. } | Hit::Edge { .. } | Hit::Vertex { .. })).find_map(|h| match h {
            Hit::Face { body, point, .. } => Some((body, point)),
            _ => None,
        })
    else {
        return false;
    };
    let s = &app.session;
    let Some(occurrence) = crate::dialogs_assembly::occurrence_of_body(s, &body).filter(|o| *o != 0) else { return false };
    if !s.doc.assembly.joints.iter().any(|j| !j.suppressed && (j.a.occurrence == occurrence || j.b.occurrence == occurrence)) {
        return false;
    }
    let sol = joints::solve(&s.doc, &s.model.state());
    if !sol.world.contains_key(&occurrence) || sol.dof.get(&occurrence).is_none_or(|d| *d == 0) {
        return false;
    }
    let Some(local) = mat_inverse(&joints::occurrence_world(&s.doc, occurrence)).map(|inv| apply_point(&inv, at)) else { return false };
    let normal = proj.ray(p).1;
    app.viewport.joint_drag = Some(Drag { occurrence, local, origin: at, normal, pushed: false });
    true
}

pub fn active(app: &SolveApp) -> bool {
    app.viewport.joint_drag.is_some()
}

/// Reach for the point under `p`.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(d) = app.viewport.joint_drag.clone() else { return };
    let (o, dir) = proj.ray(p);
    let facing = dir.dot(d.normal);
    if facing.abs() < 1e-9 {
        return;
    }
    let target = o + dir * ((d.origin - o).dot(d.normal) / facing);
    if !target.is_finite() {
        return;
    }
    let at = apply_point(&joints::occurrence_world(&app.session.doc, d.occurrence), d.local);
    let params =
        json!({"occurrence": d.occurrence, "at": [at.x, at.y, at.z], "point": [target.x, target.y, target.z], "quiet": true, "continuous": true});
    // One undo step for the whole drag: the first frame's, kept on top while the others run (not
    // truncated by depth, which a full history would make lose its oldest steps frame by frame).
    let own = if d.pushed { app.session.undo.pop() } else { None };
    // A frame that changes the design adds an undo step (a full history drops its oldest for it,
    // so its length says nothing).
    let before = app.session.doc.clone();
    let pushed = app.run("joint.reach", params).is_ok() && !std::sync::Arc::ptr_eq(&before, &app.session.doc);
    match own {
        Some(step) => {
            if pushed {
                app.session.undo.pop();
            }
            app.session.undo.push(step);
        }
        None if pushed => {
            if let Some(d) = app.viewport.joint_drag.as_mut() {
                d.pushed = true;
            }
        }
        None => {}
    }
}

pub fn end(app: &mut SolveApp) {
    app.viewport.joint_drag = None;
}

/// Assemble ▸ Move's handles on a part that joints place (#6679).
#[derive(Clone, Debug)]
pub struct Handles {
    occurrence: u64,
    joint: u64,
    name: String,
    /// The joint's B origin, and its A frame's axes (world): where the handles are.
    centre: Vec3,
    frame: [Vec3; 3],
    freedoms: Vec<Freedom>,
    /// The design the freedoms were read from: they are read again when it changes.
    seen: Arc<Document>,
    drag: Option<HandleDrag>,
    /// The last drag's travel along its handle since it began (deg or mm, signed), whether it
    /// turned, and why it stopped short (a message id); shown while the design is the one that
    /// drag left (`shown_on`: not after an undo).
    readout: Option<(f64, bool, Option<&'static str>)>,
    shown_on: Option<Arc<Document>>,
}

/// A value free to drag.
#[derive(Clone, Debug, PartialEq)]
struct Freedom {
    index: usize,
    turn: bool,
    /// The frame axis it moves along or turns about.
    axis: usize,
    /// -1: the part is the joint's A side, so it moves against the value.
    sign: f64,
    /// Now (deg or mm).
    value: f64,
}

#[derive(Clone, Debug)]
struct HandleDrag {
    index: usize,
    start: f64,
    /// The value the last frame asked for: a frame asking the same runs nothing (a handle
    /// pressed and held still leaves no undo step).
    asked: f64,
    /// What Escape goes back to (`edit.checkpoint`).
    checkpoint: Option<u64>,
    /// The drag's undo step, once a frame moved the part (`occurrence_drag::run_frame`).
    step: Option<Arc<Document>>,
    /// The design as the drag began: until it has a step, a design changed under it (an undo)
    /// ends the drag.
    began: Arc<Document>,
}

const HANDLES: &str = "sc_joint_move";

fn vec3(v: &Value) -> Option<Vec3> {
    Some(Vec3::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?)).filter(|v| v.is_finite())
}

/// The handles for `occurrence` from `joint.freedoms`' answer `r` (`None`: no freedom to drag).
fn handles(app: &SolveApp, occurrence: u64, r: &Value) -> Option<Handles> {
    let f = &r["frame"];
    let freedoms: Vec<Freedom> = r["freedoms"]
        .as_array()?
        .iter()
        .filter_map(|f| {
            Some(Freedom {
                index: usize::try_from(f["index"].as_u64()?).ok()?,
                turn: f["turn"].as_bool()?,
                axis: ["x", "y", "z"].iter().position(|a| f["axis"] == *a)?,
                sign: f["sign"].as_f64()?.signum(),
                value: f["value"].as_f64()?,
            })
        })
        .collect();
    if freedoms.is_empty() {
        return None;
    }
    Some(Handles {
        occurrence,
        joint: r["joint"].as_u64()?,
        name: r["name"].as_str().unwrap_or_default().to_string(),
        centre: vec3(&f["origin"])?,
        frame: [vec3(&f["x"])?, vec3(&f["y"])?, vec3(&f["z"])?],
        freedoms,
        seen: app.session.doc.clone(),
        drag: None,
        readout: None,
        shown_on: None,
    })
}

fn freedoms(app: &mut SolveApp, occurrence: u64) -> Option<Value> {
    app.session.execute("joint.freedoms", &json!({"occurrence": occurrence})).ok()
}

/// Assemble ▸ Move (or the context menu's Move) on `occurrence`: handles for the freedoms its
/// joint allows, or a warning, its joints kept as they are, when it has none. False when no
/// joint places it (a free part, or a grounded one joined to nothing): the usual Move goes ahead.
pub fn open(app: &mut SolveApp, occurrence: u64) -> bool {
    close(app);
    if !app.preferences.assembly_move_freedoms {
        return false;
    }
    let Some(r) = freedoms(app, occurrence) else { return false };
    let jointed = app.session.doc.assembly.joints.iter().any(|j| !j.suppressed && (j.a.occurrence == occurrence || j.b.occurrence == occurrence));
    if r["placed"] != true && !(jointed && r["grounded"] == true) {
        return false;
    }
    // Move takes over from the tool or dialog open, as starting any command does.
    if app.dialog.is_some() {
        crate::dialogs::cancel(app);
    }
    app.tool = None;
    app.tree.occurrence_move = None;
    match handles(app, occurrence, &r) {
        Some(h) => app.viewport.joint_move = Some(h),
        None => {
            let part = app.session.doc.occurrences.iter().find(|o| o.id == occurrence).map(|o| o.name.clone()).unwrap_or_default();
            let why = r["reason"].as_str().unwrap_or_default();
            app.set_status(crate::i18n::format("assembly_move.held", &[("part", part.as_str()), ("reason", why)]), true);
        }
    }
    true
}

/// Move with only faces, edges or bodies of one placed part picked: [`open`] on that part. False
/// for any other pick, and for the active component's own part (editing it moves its bodies).
pub fn open_for_selection(app: &mut SolveApp) -> bool {
    use solidcraft_engine::Sel;
    let s = &app.session;
    let mut picked = None;
    for sel in &s.selection {
        let o = match sel {
            Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Body { name: body } => crate::dialogs_assembly::occurrence_of_body(s, body),
            _ => None,
        };
        match o.filter(|o| *o != 0) {
            Some(o) if picked.is_none_or(|p| p == o) => picked = Some(o),
            _ => return false,
        }
    }
    let Some(o) = picked else { return false };
    if s.doc.occurrences.iter().any(|x| x.id == o && x.component == s.active_component) {
        return false;
    }
    open(app, o)
}

/// Close the handles; a drag under way stays where it is.
pub fn close(app: &mut SolveApp) {
    if let Some(Handles { drag: Some(HandleDrag { checkpoint: Some(checkpoint), .. }), .. }) = app.viewport.joint_move.take() {
        let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": checkpoint, "forget": true}));
    }
}

/// Escape: a drag on the handles puts the part back where it began, leaving no undo step;
/// without one, the handles close. True when there was either.
pub fn cancel(app: &mut SolveApp, ctx: &egui::Context) -> bool {
    let Some(mut h) = app.viewport.joint_move.take() else { return false };
    if let Some(d) = h.drag.take() {
        gizmo::cancel(ctx, egui::Id::new(HANDLES));
        match d.checkpoint {
            Some(checkpoint) => drop(app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": checkpoint}))),
            None if d.step.is_some() => drop(app.session.execute("edit.undo", &json!({}))),
            None => {}
        }
        h.readout = None;
        app.viewport.joint_move = Some(h);
    }
    true
}

/// The handles as a triad (for hit tests before navigation too).
pub(crate) fn triad(app: &SolveApp) -> Option<gizmo::Triad> {
    let h = app.viewport.joint_move.as_ref()?;
    let on = |turn: bool| {
        let mut m = [false; 3];
        for f in h.freedoms.iter().filter(|f| f.turn == turn) {
            if let Some(x) = m.get_mut(f.axis) {
                *x = true;
            }
        }
        m
    };
    let (arrows, rings) = (on(false), on(true));
    let p = &app.preferences;
    Some(gizmo::Triad {
        center: h.centre,
        translate: arrows.contains(&true),
        rotate: rings.contains(&true),
        joint: Some(gizmo::Freedoms { frame: h.frame, arrows, rings, turn_steps: (f64::from(p.joint_turn_step), f64::from(p.joint_turn_fine_step)) }),
    })
}

/// Draw the handles and their readout, and run a drag on them (no tool or dialog open).
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    let Some(h) = app.viewport.joint_move.as_ref() else { return };
    // The design changed (a frame of the drag, undo, an edit): read the freedoms again.
    if !Arc::ptr_eq(&h.seen, &app.session.doc) {
        let (occurrence, drag, readout, shown_on) = (h.occurrence, h.drag.clone(), h.readout, h.shown_on.clone());
        match freedoms(app, occurrence).and_then(|r| handles(app, occurrence, &r)) {
            Some(fresh) => app.viewport.joint_move = Some(Handles { drag, readout, shown_on, ..fresh }),
            None => return close(app),
        }
    }
    let Some(t) = triad(app) else { return };
    let step = crate::canvas::snap_step(app.cam.half_height());
    if let Some(g) = gizmo::show(ui, painter, proj, egui::Id::new(HANDLES), &t, step) {
        drag(app, g);
    }
    readout(app, painter, proj);
}

/// One frame of a drag on a handle: its joint value goes to where it began plus the travel.
fn drag(app: &mut SolveApp, g: gizmo::Drag) {
    let Some(mut h) = app.viewport.joint_move.clone() else { return };
    let (turn, axis) = match g.handle {
        Handle::Axis(i) => (false, i),
        Handle::Ring(i) => (true, i),
        _ => return,
    };
    let Some(f) = h.freedoms.iter().find(|f| f.turn == turn && f.axis == axis).cloned() else { return };
    // A drag begins on its first frame only (not on a gesture left over from handles closed
    // mid-drag).
    if g.started {
        let checkpoint = app.session.execute("edit.checkpoint", &json!({})).ok().and_then(|r| r["checkpoint"].as_u64());
        let began = app.session.doc.clone();
        h.drag = Some(HandleDrag { index: f.index, start: f.value, asked: f.value, checkpoint, step: None, began });
        h.readout = Some((0.0, f.turn, None));
        h.shown_on = Some(app.session.doc.clone());
    }
    let Some(mut d) = h.drag.take().filter(|d| d.index == f.index) else { return };
    let travel = match g.change {
        Change::Translate(v) => h.frame.get(f.axis).map_or(0.0, |a| v.dot(*a)),
        Change::Rotate { angle, .. } => angle.to_degrees(),
    };
    let target = d.start + f.sign * travel;
    if d.step.is_none() && !Arc::ptr_eq(&d.began, &app.session.doc) {
        h.readout = None;
        app.viewport.joint_move = Some(Handles { drag: Some(d), ..h });
        return close_drag(app);
    }
    // A frame asking what the last one asked runs nothing (a handle held still leaves no undo step).
    if target != d.asked {
        d.asked = target;
        let params = json!({"joint": h.joint, "index": f.index, "value": target, "quiet": true, "continuous": true});
        match crate::occurrence_drag::run_frame(app, &mut d.step, "joint.drive", params) {
            // Undo took the drag's step: the drag ends there.
            None => {
                h.readout = None;
                app.viewport.joint_move = Some(Handles { drag: Some(d), ..h });
                return close_drag(app);
            }
            Some(Ok(r)) => {
                let got = r["values"].get(f.index).and_then(|v| v["value"].as_f64()).unwrap_or(d.start);
                let stop = if (got - target).abs() <= 1e-6 {
                    None
                } else if r["stopped_by_contact"].is_object() {
                    Some("assembly_move.contact")
                } else if r["stopped_by_loop"].is_object() {
                    Some("assembly_move.loop")
                } else {
                    Some("assembly_move.limit")
                };
                h.readout = Some((f.sign * (got - d.start), f.turn, stop));
                h.shown_on = Some(app.session.doc.clone());
            }
            Some(Err(_)) => {}
        }
    }
    h.drag = Some(d);
    app.viewport.joint_move = Some(h);
    if g.done {
        close_drag(app);
    }
}

/// The drag ends: the part stays where it is.
fn close_drag(app: &mut SolveApp) {
    let Some(h) = app.viewport.joint_move.as_mut() else { return };
    if let Some(HandleDrag { checkpoint: Some(checkpoint), .. }) = h.drag.take() {
        let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": checkpoint, "forget": true}));
    }
}

/// The readout shown: during a drag, and after it until the design changes (an undo).
fn shown_readout(app: &SolveApp) -> Option<(f64, bool, Option<&'static str>)> {
    let h = app.viewport.joint_move.as_ref()?;
    let current = h.shown_on.as_ref().is_some_and(|d| Arc::ptr_eq(d, &app.session.doc));
    h.readout.filter(|_| h.drag.is_some() || current)
}

/// The travel since the drag began, beside the handles, and why it stopped short.
fn readout(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some(h) = app.viewport.joint_move.as_ref() else { return };
    let (Some((travel, turn, stop)), Some(at)) = (shown_readout(app), proj.to_screen(h.centre)) else { return };
    let sign = if travel > 0.0 { "+" } else { "" };
    let value = if turn {
        format!("{sign}{:.*}°", usize::from(app.preferences.angle_decimals.min(6)), travel)
    } else {
        format!("{sign}{}", crate::prefs::show_mm(app, travel, 1))
    };
    let text = match stop {
        Some(id) => format!("{} · {value} · {}", h.name, crate::i18n::tr(id)),
        None => format!("{} · {value}", h.name),
    };
    let t = Tokens::get();
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(t.font_label), t.text);
    let rect = egui::Rect::from_min_size(at + egui::vec2(gizmo::LEN_PX as f32 + 16.0, -12.0), galley.size() + egui::vec2(10.0, 6.0));
    crate::scenario::publish_target("joint-move:readout", rect);
    painter.rect(rect, t.radius_sm.saturating_sub(1), t.overlay, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
    painter.galley(rect.min + egui::vec2(5.0, 3.0), galley, t.text);
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcraft_engine::doc::{apply_point, joints};
    use solidcraft_engine::geom::Vec3;

    use crate::scenario::Harness;
    use crate::test_support::play;

    fn run(h: &mut Harness, id: &str, p: Value) -> Value {
        h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn screen(h: &mut Harness, p: Vec3) -> egui::Pos2 {
        let [x, y] = h.at(&json!({"world": [p.x, p.y, p.z]})).expect("on screen");
        egui::pos2(x as f32, y as f32)
    }

    /// Where the arm's point `local` (its own coordinates) is.
    fn arm_at(h: &Harness, local: Vec3) -> Vec3 {
        let s = &h.app.session;
        let c = s.doc.find_component("Arm").unwrap_or(0);
        let o = s.doc.occurrence_of(c).map(|o| o.id).unwrap_or(0);
        apply_point(&joints::occurrence_world(&s.doc, o), local)
    }

    /// An arm hinged about +Z on a grounded base (seen from the top): its far end, and the screen
    /// points of that end and of the end turned a quarter round.
    fn hinged_arm() -> (Harness, Vec3, egui::Pos2, egui::Pos2) {
        let mut h = Harness::new();
        h.app.home.open = false;
        for (name, corner, size) in [("Base", [-5, -5, 0], [10, 10, 5]), ("Arm", [-3, -3, 5], [63, 6, 5])] {
            run(&mut h, "component.activate", json!({"component": "root"}));
            run(&mut h, "component.create", json!({"name": name}));
            run(&mut h, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner}));
        }
        run(&mut h, "component.activate", json!({"component": "root"}));
        let base = h.app.session.doc.find_component("Base").and_then(|c| h.app.session.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
        run(&mut h, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
        let at = |o: &str| json!({"occurrence": o, "point": [0, 0, 5], "z": [0, 0, 1]});
        run(&mut h, "joint.create", json!({"type": "revolute", "a": at("Base"), "b": at("Arm"), "flip": true, "name": "Hinge"}));
        h.call("ui.view", json!({"view": "top"}));
        h.app.cam.distance = 200.0;
        h.frames(3);
        let end = Vec3::new(55.0, 0.0, 10.0);
        let (a, b) = (screen(&mut h, end), screen(&mut h, Vec3::new(0.0, 55.0, 10.0)));
        (h, end, a, b)
    }

    /// Press at `a` and move to `b` in 12 frames, holding the button there.
    fn press_and_move(h: &mut Harness, a: egui::Pos2, b: egui::Pos2) {
        h.app.synthetic.push(egui::Event::PointerMoved(a));
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: a,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.app.synthetic.extend((1..=12).map(|k| egui::Event::PointerMoved(a + (b - a) * (k as f32 / 12.0))));
        play(h);
    }

    fn release(h: &mut Harness, b: egui::Pos2) {
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: b,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        play(h);
    }

    /// Dragged by its far end a quarter turn round, the arm turns there in one undo step, and undo
    /// turns it back.
    #[test]
    fn dragging_a_jointed_part_turns_it_about_its_hinge() {
        let (mut h, end, a, b) = hinged_arm();
        let undo = h.app.session.undo.len();
        press_and_move(&mut h, a, b);
        assert!(h.app.viewport.joint_drag.is_some() && h.app.viewport.boxsel.is_none(), "the drag moves the arm, not a selection box");
        release(&mut h, b);
        assert!(h.app.viewport.joint_drag.is_none());
        assert!(arm_at(&h, end).dist(Vec3::new(0.0, 55.0, 10.0)) < 0.5, "{:?}", arm_at(&h, end));
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the drag");
        assert!(!h.app.session.log.iter().any(|l| l.contains("Reach")), "frames are not echoed: {:?}", h.app.session.log);
        run(&mut h, "edit.undo", json!({}));
        assert!(arm_at(&h, end).dist(end) < 1e-9, "{:?}", arm_at(&h, end));
    }

    /// #5137: while a part is dragged along its joint, nothing under the pointer highlights; hover
    /// comes back once the drag ends.
    #[test]
    fn dragging_a_jointed_part_highlights_nothing_under_the_pointer() {
        let (mut h, _, a, b) = hinged_arm();
        h.app.synthetic.push(egui::Event::PointerMoved(a));
        play(&mut h);
        assert!(h.app.viewport.hover.is_some(), "the arm under the pointer must highlight before the drag");
        press_and_move(&mut h, a, b);
        assert!(h.app.viewport.joint_drag.is_some(), "the drag must move the arm");
        assert!(h.app.viewport.hover.is_none(), "dragging a part must not highlight what is under the pointer");
        h.frames(2);
        assert!(h.app.viewport.hover.is_none(), "a drag held still must not highlight either");
        release(&mut h, b);
        h.frames(2);
        assert!(h.app.viewport.hover.is_some(), "hover must return after the drag");
    }

    /// With the undo history full, a drag still adds one step: it drops the oldest once, not one
    /// per frame.
    #[test]
    fn a_drag_on_a_full_undo_history_drops_one_step() {
        let (mut h, _, a, b) = hinged_arm();
        let step = h.app.session.undo.last().cloned().expect("an undo step");
        while h.app.session.undo.len() < 200 {
            let mut older = step.clone();
            older.label = format!("step {}", h.app.session.undo.len());
            h.app.session.undo.insert(0, older);
        }
        let second = h.app.session.undo.get(1).map(|s| s.label.clone());
        press_and_move(&mut h, a, b);
        release(&mut h, b);
        assert_eq!(h.app.session.undo.len(), 200);
        assert_eq!(h.app.session.undo.first().map(|s| s.label.clone()), second, "only the oldest step went");
        assert_eq!(h.app.session.undo.last().map(|s| s.label.as_str()), Some("Reach Point"), "the drag's step is on top");
    }

    fn occ(h: &Harness, part: &str) -> u64 {
        let s = &h.app.session;
        s.doc.find_component(part).and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0)
    }

    /// A part `name` (a box), joined to Base by a `kind` joint at `at` with its z along `z`.
    fn add(h: &mut Harness, name: &str, corner: [f64; 3], size: [f64; 3], kind: &str, at: [f64; 3], z: [f64; 3]) {
        run(h, "component.activate", json!({"component": "root"}));
        run(h, "component.create", json!({"name": name, "grounded": false}));
        run(h, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner}));
        run(h, "component.activate", json!({"component": "root"}));
        let side = |o: &str| json!({"occurrence": o, "point": at, "z": z});
        run(h, "joint.create", json!({"type": kind, "a": side("Base"), "b": side(name), "flip": true, "name": format!("{name} joint")}));
    }

    /// The hinged arm (`hinged_arm`), with Rod on a slider along +X, Shaft on a cylindrical joint
    /// about +Z and Plate welded to Base (#6679).
    fn mechanism() -> (Harness, Vec3, egui::Pos2, egui::Pos2) {
        let (mut h, end, a, b) = hinged_arm();
        add(&mut h, "Rod", [20.0, 20.0, 0.0], [10.0, 4.0, 4.0], "slider", [20.0, 22.0, 2.0], [1.0, 0.0, 0.0]);
        add(&mut h, "Shaft", [-30.0, 20.0, 0.0], [6.0, 6.0, 10.0], "cylindrical", [-27.0, 23.0, 0.0], [0.0, 0.0, 1.0]);
        add(&mut h, "Plate", [-30.0, -30.0, 0.0], [5.0, 5.0, 5.0], "rigid", [-30.0, -30.0, 0.0], [0.0, 0.0, 1.0]);
        h.frames(2);
        (h, end, a, b)
    }

    /// Pick a face of `part` and start Move.
    fn move_part(h: &mut Harness, part: &str) {
        super::close(&mut h.app);
        let o = occ(h, part);
        let body = h.app.session.occurrence_bodies(&[o], true).ok().and_then(|b| b.first().cloned()).expect("a body");
        run(h, "select.set", json!({"items": [{"type": "face", "body": body, "index": 0, "point": [0, 0, 0]}]}));
        h.frames(2);
        assert!(h.app.viewport.joint_move.is_none() && h.app.dialog.is_none(), "picking a face starts nothing");
        h.app.start("solid.move");
        h.frames(2);
    }

    /// The handles shown: (arrows, rings), and the frame axis of the first freedom.
    fn shown(h: &Harness) -> Option<([bool; 3], [bool; 3], Vec3)> {
        let f = super::triad(&h.app)?.joint?;
        let m = h.app.viewport.joint_move.as_ref()?;
        Some((f.arrows, f.rings, f.frame[m.freedoms.first()?.axis]))
    }

    fn readout(h: &Harness) -> Option<(f64, bool, Option<&'static str>)> {
        super::shown_readout(&h.app)
    }

    fn screen_of(h: &mut Harness, handle: &str, by: Vec3, turn: f64) -> egui::Pos2 {
        let p = crate::gizmo::handle_point(handle, by, turn).expect("the handles are drawn");
        screen(h, p)
    }

    /// Move the held pointer to `b` in 12 frames.
    fn move_to(h: &mut Harness, a: egui::Pos2, b: egui::Pos2) {
        h.app.synthetic.extend((1..=12).map(|k| egui::Event::PointerMoved(a + (b - a) * (k as f32 / 12.0))));
        play(h);
    }

    fn escape(h: &mut Harness) {
        for pressed in [true, false] {
            h.app.synthetic.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            });
        }
        play(h);
    }

    /// #6679: Move on a face of a part that joints place shows only what its joint allows: a ring
    /// for a revolute, an arrow for a slider, both for a cylindrical joint. A welded or grounded
    /// part gets a warning and keeps its joints; a free part gets the usual Move; Measure closes
    /// the handles.
    #[test]
    fn move_shows_only_the_freedoms_its_joint_allows() {
        let (mut h, ..) = mechanism();
        for (part, arrows, rings, axis) in [
            ("Arm", [false; 3], [false, false, true], Vec3::Z),
            ("Rod", [false, false, true], [false; 3], Vec3::X),
            ("Shaft", [false, false, true], [false, false, true], Vec3::Z),
        ] {
            move_part(&mut h, part);
            let (a, r, f) = shown(&h).unwrap_or_else(|| panic!("{part}: no handles ({:?})", h.app.status));
            assert_eq!((a, r), (arrows, rings), "{part}");
            assert!(f.dist(axis) < 1e-9, "{part}: {f:?}");
            assert!(h.app.dialog.is_none() && h.app.tree.occurrence_move.is_none(), "{part}: no free Move");
        }
        let joints = serde_json::to_value(&h.app.session.doc.assembly.joints).unwrap();
        for part in ["Plate", "Base"] {
            h.app.status = None;
            move_part(&mut h, part);
            assert!(h.app.viewport.joint_move.is_none() && h.app.dialog.is_none(), "{part}");
            let (text, _, warned) = h.app.status.clone().expect("a warning");
            assert!(warned && text.contains(part), "{part}: {text}");
        }
        assert_eq!(serde_json::to_value(&h.app.session.doc.assembly.joints).unwrap(), joints, "the joints are kept");
        // The browser's Move/Copy on the arm opens the same handles.
        let target = crate::context_menu::Target::Occurrence { path: vec![occ(&h, "Arm")] };
        let item = crate::context_menu::items(&h.app, &target).into_iter().find(|i| i.id == "ui.moveOccurrence").expect("Move/Copy");
        crate::context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
        h.frames(2);
        assert!(shown(&h).is_some_and(|(_, r, _)| r == [false, false, true]) && h.app.tree.occurrence_move.is_none());
        h.app.start("inspect.measure");
        h.frames(2);
        assert!(h.app.viewport.joint_move.is_none(), "Measure closes the handles");
        crate::dialogs::cancel(&mut h.app);
        // A part joined to nothing moves as before.
        run(&mut h, "component.activate", json!({"component": "root"}));
        run(&mut h, "component.create", json!({"name": "Loose", "grounded": false}));
        run(&mut h, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [40, -30, 0]}));
        run(&mut h, "component.activate", json!({"component": "root"}));
        move_part(&mut h, "Loose");
        assert!(h.app.viewport.joint_move.is_none() && h.app.dialog.is_some());
        crate::dialogs::cancel(&mut h.app);
        // With the preference off, Move on a jointed part is the usual Move.
        h.app.preferences.assembly_move_freedoms = false;
        move_part(&mut h, "Arm");
        assert!(h.app.viewport.joint_move.is_none() && h.app.dialog.is_some());
        crate::dialogs::cancel(&mut h.app);
        h.app.preferences.assembly_move_freedoms = true;
        // The browser's Move/Copy on the welded plate warns and closes another part's free Move.
        let menu = |h: &mut Harness, part: &str| {
            let target = crate::context_menu::Target::Occurrence { path: vec![occ(h, part)] };
            let item = crate::context_menu::items(&h.app, &target).into_iter().find(|i| i.id == "ui.moveOccurrence").expect("Move/Copy");
            crate::context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
            h.frames(2);
        };
        menu(&mut h, "Loose");
        assert!(h.app.tree.occurrence_move.is_some(), "a free part gets the free Move");
        menu(&mut h, "Plate");
        assert!(h.app.tree.occurrence_move.is_none() && h.app.viewport.joint_move.is_none());
    }

    /// Turning the arm's ring a quarter round turns it about its hinge in one undo step, the
    /// readout giving the turn; undo turns it back.
    #[test]
    fn a_ring_drag_turns_the_joint_in_one_undo_step() {
        let (mut h, end, ..) = hinged_arm();
        move_part(&mut h, "Arm");
        let undo = h.app.session.undo.len();
        let (a, b) = (screen_of(&mut h, "rz", Vec3::ZERO, 0.0), screen_of(&mut h, "rz", Vec3::ZERO, 90.0));
        press_and_move(&mut h, a, b);
        release(&mut h, b);
        assert!(arm_at(&h, end).dist(Vec3::new(0.0, 55.0, 10.0)) < 1e-6, "{:?}", arm_at(&h, end));
        let (travel, turn, stop) = readout(&h).expect("a readout");
        assert!((travel - 90.0).abs() < 1e-6 && turn && stop.is_none(), "{travel} {turn} {stop:?}");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the drag");
        assert_eq!(h.app.session.undo.last().map(|s| s.label.as_str()), Some("Drive Joints"));
        assert!(!h.app.session.log.iter().any(|l| l.contains("Drive Joints")), "frames are not echoed: {:?}", h.app.session.log);
        assert!(h.app.viewport.joint_move.is_some(), "the handles stay for the next drag");
        run(&mut h, "edit.undo", json!({}));
        assert!(arm_at(&h, end).dist(end) < 1e-9, "{:?}", arm_at(&h, end));
        h.frames(2);
        assert!(readout(&h).is_none(), "undo takes the readout away");
    }

    /// Near the ring the turn goes in 10° steps; past the dashed guide in 0.1° steps, counted on
    /// from where it was, so crossing the guide does not move the part.
    #[test]
    fn past_the_guide_the_turn_is_fine_without_a_jump() {
        let (mut h, ..) = hinged_arm();
        move_part(&mut h, "Arm");
        let centre = crate::gizmo::handle_point("ball", Vec3::ZERO, 0.0).expect("handles");
        let out = |turn: f64| {
            let p = crate::gizmo::handle_point("rz", Vec3::ZERO, turn).unwrap_or(centre);
            centre + (p - centre) * 2.0
        };
        assert!(out(0.0).dist(centre) > crate::gizmo::guide_radius().unwrap_or(f64::MAX), "beyond the guide");
        let a = screen_of(&mut h, "rz", Vec3::ZERO, 0.0);
        let b = screen_of(&mut h, "rz", Vec3::ZERO, 33.0);
        press_and_move(&mut h, a, b);
        let near = readout(&h).map(|r| r.0);
        assert!(near.is_some_and(|t| (t - 30.0).abs() < 1e-6), "{near:?}");
        let c = screen(&mut h, out(33.0));
        move_to(&mut h, b, c);
        let crossed = readout(&h).map(|r| r.0);
        assert!(crossed.is_some_and(|t| (t - 30.0).abs() < 1e-6), "no jump crossing the guide: {crossed:?}");
        let d = screen(&mut h, out(40.0));
        move_to(&mut h, c, d);
        let fine = readout(&h).map(|r| r.0).unwrap_or(f64::NAN);
        assert!((fine - 37.0).abs() < 0.11 && ((fine * 10.0).round() - fine * 10.0).abs() < 1e-4, "{fine}");
        release(&mut h, d);
    }

    /// A slide drag stops at its limit and at a contact, the readout saying which; Escape puts
    /// the part back where the drag began, leaving no undo step.
    #[test]
    fn a_slide_stops_at_limits_and_contacts_and_escape_puts_it_back() {
        let (mut h, ..) = mechanism();
        run(&mut h, "joint.limits", json!({"joint": "Rod joint", "min": -5, "max": 5}));
        let rod = |h: &Harness| apply_point(&joints::occurrence_world(&h.app.session.doc, occ(h, "Rod")), Vec3::new(20.0, 20.0, 0.0));
        let start = rod(&h);
        move_part(&mut h, "Rod");
        let undo = h.app.session.undo.len();
        let a = screen_of(&mut h, "z", Vec3::ZERO, 0.0);
        let b = screen_of(&mut h, "z", Vec3::X * 20.0, 0.0);
        press_and_move(&mut h, a, b);
        assert!(rod(&h).dist(start + Vec3::X * 5.0) < 1e-6, "{:?}", rod(&h));
        let (travel, turn, stop) = readout(&h).expect("a readout");
        assert!((travel - 5.0).abs() < 1e-6 && !turn && stop == Some("assembly_move.limit"), "{travel} {stop:?}");
        escape(&mut h);
        assert!(rod(&h).dist(start) < 1e-9, "Escape puts it back: {:?}", rod(&h));
        release(&mut h, b);
        assert!(rod(&h).dist(start) < 1e-9, "the release after Escape moves nothing");
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(h.app.viewport.joint_move.is_some(), "the handles stay");
        // A block 3 mm ahead in a contact set with the rod stops it there.
        run(&mut h, "joint.limits", json!({"joint": "Rod joint"}));
        run(&mut h, "component.activate", json!({"component": "root"}));
        run(&mut h, "component.create", json!({"name": "Stop", "grounded": true}));
        run(&mut h, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [33, 20, 0]}));
        run(&mut h, "component.activate", json!({"component": "root"}));
        run(&mut h, "contact.create", json!({"occurrences": ["Rod", "Stop"]}));
        move_part(&mut h, "Rod");
        let a = screen_of(&mut h, "z", Vec3::ZERO, 0.0);
        let b = screen_of(&mut h, "z", Vec3::X * 20.0, 0.0);
        press_and_move(&mut h, a, b);
        release(&mut h, b);
        let (travel, _, stop) = readout(&h).expect("a readout");
        assert!(travel > 2.5 && travel < 3.01 && stop == Some("assembly_move.contact"), "{travel} {stop:?}");
        assert!(rod(&h).dist(start + Vec3::X * travel) < 1e-6);
    }

    /// Undo pressed during a gesture that has not moved the part yet ends that gesture: it does
    /// not carry on from the pose undo took away.
    #[test]
    fn undo_during_a_gesture_ends_it() {
        let (mut h, end, ..) = hinged_arm();
        run(&mut h, "joint.drive", json!({"joint": "Hinge", "value": 30}));
        move_part(&mut h, "Arm");
        let centre = crate::gizmo::handle_point("ball", Vec3::ZERO, 0.0).expect("handles");
        let out = |turn: f64| {
            let p = crate::gizmo::handle_point("rz", Vec3::ZERO, turn).unwrap_or(centre);
            centre + (p - centre) * 1.5
        };
        let a = screen_of(&mut h, "rz", Vec3::ZERO, 0.0);
        let b = screen(&mut h, out(0.0));
        // Out from the ring along a radius: the angle stays, so nothing moves yet.
        press_and_move(&mut h, a, b);
        run(&mut h, "edit.undo", json!({}));
        assert!(arm_at(&h, end).dist(end) < 1e-6, "undo takes the 30° back");
        let c = screen(&mut h, out(20.0));
        move_to(&mut h, b, c);
        release(&mut h, c);
        assert!(arm_at(&h, end).dist(end) < 1e-6, "the gesture ended with the undo: {:?}", arm_at(&h, end));
    }

    /// Escape with no drag under way closes the handles.
    #[test]
    fn escape_closes_the_handles() {
        let (mut h, ..) = hinged_arm();
        move_part(&mut h, "Arm");
        assert!(h.app.viewport.joint_move.is_some());
        escape(&mut h);
        assert!(h.app.viewport.joint_move.is_none());
    }
}
