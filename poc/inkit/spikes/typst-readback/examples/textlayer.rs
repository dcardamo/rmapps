use typst_readback::compile_pdf;
fn main() -> anyhow::Result<()> {
    let src = r#"#set page(width: 340pt, height: 600pt, margin: 16pt)
#set text(size: 9.5pt)
Italy has banned two concerts involving American rappers Kanye West and Travis Scott that were due to take place in July in the northern city of Reggio Emilia, authorities said on Saturday.

The local prefect, Salvatore Angieri, ordered the cancellation because of concerns over public order and security.
"#;
    let pdf = compile_pdf(src)?;
    std::fs::write("/tmp/typst.pdf", &pdf)?;
    println!("wrote /tmp/typst.pdf ({} bytes)", pdf.len());
    Ok(())
}
