use anyhow::{Context, Result};
use std::path::Path;

/// Push a PDF preserving on-device ink (content-only) via the native `rm-cloud`
/// client — no `rmapi` CLI. Spike-grade: credentials come from the environment
/// (`RM_CLOUD_DEVICE_TOKEN`, or a valid `RM_CLOUD_USER_TOKEN`). The document is
/// named after the PDF's file stem under `folder`; a re-push of the same name is an
/// ink-preserving content-only swap.
pub fn push_content_only(pdf: &Path, folder: &str) -> Result<()> {
    let name = pdf
        .file_stem()
        .and_then(|s| s.to_str())
        .context("non-UTF-8 pdf stem")?
        .to_string();
    let bytes = std::fs::read(pdf).with_context(|| format!("read {}", pdf.display()))?;

    // Build a short-lived runtime: rm-cloud is async, this spike helper is sync.
    let rt = tokio::runtime::Runtime::new().context("build tokio runtime")?;
    rt.block_on(async move {
        let client = rm_cloud::Client::from_env()
            .context("rm-cloud credentials (RM_CLOUD_DEVICE_TOKEN / RM_CLOUD_USER_TOKEN)")?;
        let folder_id = client.mkdir_p(folder).await.context("mkdir_p")?;
        let existing = client
            .ls(&folder_id)
            .await
            .context("ls")?
            .into_iter()
            .find(|e| !e.is_folder && e.name == name);
        match existing {
            Some(e) => client
                .put_content_only(&e.id, bytes)
                .await
                .context("put_content_only")?,
            None => client
                .put(rm_cloud::DocFiles::new_pdf(&name, &folder_id, bytes))
                .await
                .context("put")?,
        }
        Ok(())
    })
}
