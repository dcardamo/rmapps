//! Non-destructive read-back inspector: print the plan rmreader WOULD apply for a
//! downloaded `.rmdoc`, without calling Readwise. Use to verify detection before a
//! real sync. Usage: `cargo run --example readback_inspect -- <bundle.rmdoc>`
use rmreader::readback;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .expect("usage: readback_inspect <bundle.rmdoc>");
    let plan = readback::detect(std::path::Path::new(&path))?;
    println!("actions ({}):", plan.actions.len());
    for (id, kind) in &plan.actions {
        println!("  {kind:?}  doc={id}");
    }
    println!("content highlights ({}):", plan.highlights.len());
    for h in &plan.highlights {
        println!("  source_url={}  text={:?}", h.source_url, h.text);
    }
    println!("warnings ({}):", plan.warnings.len());
    for w in &plan.warnings {
        println!("  {w}");
    }
    Ok(())
}
