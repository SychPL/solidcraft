//! Fork-only probe for #2147: reads a failing SmApiBoolean's dumped inputs, writes them as ASCII
//! `.smb`, and replays the union both ways.
use solidcraft_smlib::{BooleanOp, Brep};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (a, b, out) = (Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]));
    let ba = Brep::read(a, false).expect("read a");
    let bb = Brep::read(b, false).expect("read b");
    for (name, body) in [("a", &ba), ("b", &bb)] {
        println!("{name}: counts {:?} volume {:?}", body.counts(), body.volume(1e-8));
        body.write(&out.join(format!("{name}.ascii.smb")), true).expect("write ascii");
    }
    println!("a union b: {:?}", ba.boolean(&bb, BooleanOp::Union).map(|r| r.map(|r| (r.counts(), r.volume(1e-8)))));
    println!("b union a: {:?}", bb.boolean(&ba, BooleanOp::Union).map(|r| r.map(|r| (r.counts(), r.volume(1e-8)))));
}
