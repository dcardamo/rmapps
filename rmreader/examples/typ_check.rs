//! Dev-only: compile a .typ file with the rmreader World and print each
//! diagnostic with its line + surrounding snippet. Usage: typ_check <file.typ>
use rmreader::render::RmWorld;
use typst::{World, WorldExt};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .expect("usage: typ_check <file.typ>");
    let src = std::fs::read_to_string(&path)?;
    let world = RmWorld::new(&src, &[]);
    let warned = typst::compile::<typst::layout::PagedDocument>(&world);
    let diags = match warned.output {
        Ok(_) => {
            println!("compiled OK");
            return Ok(());
        }
        Err(d) => d,
    };
    let main = world.main();
    let source = typst::World::source(&world, main).unwrap();
    for d in &diags {
        let mut where_ = "?".to_string();
        let mut snippet = String::new();
        if let Some(range) = world.range(d.span) {
            let s = source.text();
            let line = s[..range.start].matches('\n').count() + 1;
            where_ = format!("line {line} (byte {})", range.start);
            let lo = range.start.saturating_sub(80);
            let hi = (range.end + 80).min(s.len());
            snippet = s[lo..hi].replace('\n', "\\n");
        }
        println!(
            "[{}] {} :: {where_}\n    …{snippet}…",
            d.severity_label(),
            d.message
        );
    }
    Ok(())
}

trait SevLabel {
    fn severity_label(&self) -> &'static str;
}
impl SevLabel for typst::diag::SourceDiagnostic {
    fn severity_label(&self) -> &'static str {
        match self.severity {
            typst::diag::Severity::Error => "ERROR",
            typst::diag::Severity::Warning => "WARN",
        }
    }
}
