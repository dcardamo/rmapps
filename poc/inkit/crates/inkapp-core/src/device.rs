use crate::error::Result;
use crate::geometry::{DevicePoint, PdfPoint};
use crate::ink::Stroke;

/// The minimal device seam the harness substitutes ink at. Transport (sync) is
/// intentionally excluded — it is hardware and out of scope for the harness.
pub trait Device {
    /// Map a PDF-space point into this device's ink space.
    fn pdf_to_device(&self, p: PdfPoint, page_h_pt: f64) -> DevicePoint;
    /// Map a device-space point back to PDF space.
    fn device_to_pdf(&self, p: DevicePoint, page_h_pt: f64) -> PdfPoint;
    /// Parse native ink bytes into PDF-space strokes.
    fn read_ink(&self, bytes: &[u8], page_h_pt: f64) -> Result<Vec<Stroke>>;
    /// Synthesize native ink bytes from PDF-space strokes.
    fn write_ink(&self, strokes: &[Stroke], page_h_pt: f64) -> Result<Vec<u8>>;
}
