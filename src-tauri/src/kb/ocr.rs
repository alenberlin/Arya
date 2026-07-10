//! On-device OCR for image files and scanned PDFs.
//!
//! Uses `ocrs` on the pure-Rust `rten` runtime — no external binary (no
//! Tesseract), so it ships cleanly inside the `.app` bundle. The two detection
//! and recognition models (~12 MB total) are downloaded on first use and
//! SHA-256 pinned, mirroring the speech model catalog. When the models can't be
//! fetched (e.g. offline first run), OCR returns a clear error and the document
//! is marked `failed` rather than crashing.

use std::path::{Path, PathBuf};

use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use sha2::{Digest, Sha256};

use super::extract::{Extracted, Page};

/// A pinned, downloadable OCR model.
struct OcrModelSpec {
    file: &'static str,
    url: &'static str,
    /// Lower-hex SHA-256 computed from the official ocrs-models S3 artifact.
    sha256: &'static str,
}

const DETECTION: OcrModelSpec = OcrModelSpec {
    file: "text-detection.rten",
    url: "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten",
    sha256: "f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca",
};
const RECOGNITION: OcrModelSpec = OcrModelSpec {
    file: "text-recognition.rten",
    url: "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten",
    sha256: "e484866d4cce403175bd8d00b128feb08ab42e208de30e42cd9889d8f1735a6e",
};

/// Image file extensions OCR can read.
pub fn is_image_ext(ext: &str) -> bool {
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "tiff" | "tif" | "bmp" | "webp" | "gif"
    )
}

/// Where the OCR models are cached: `<app_data>/models/ocr/`.
pub fn models_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("models").join("ocr")
}

/// Ensure one model is present under `dir`, downloading + verifying it if not.
/// Blocking (called from the ingest worker thread).
fn ensure_model(dir: &Path, spec: &OcrModelSpec) -> Result<PathBuf, String> {
    let target = dir.join(spec.file);
    if target.exists() {
        return Ok(target);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let bytes = crate::http::blocking_client()
        .get(spec.url)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .map_err(|e| format!("could not download the OCR model: {e}"))?
        .error_for_status()
        .map_err(|e| format!("could not download the OCR model: {e}"))?
        .bytes()
        .map_err(|e| e.to_string())?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != spec.sha256 {
        return Err(format!(
            "OCR model {} failed its integrity check; not using it",
            spec.file
        ));
    }
    // Write to .partial then rename so a torn download is never mistaken for a
    // complete, verified model.
    let partial = target.with_extension("partial");
    std::fs::write(&partial, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Load the OCR engine, downloading + verifying the models on first use. Loading
/// the ~12 MB models is not free, so callers build one engine per document and
/// reuse it across that document's pages/images.
pub fn load_engine(app_data_dir: &Path) -> Result<OcrEngine, String> {
    let dir = models_dir(app_data_dir);
    let detection = ensure_model(&dir, &DETECTION)?;
    let recognition = ensure_model(&dir, &RECOGNITION)?;
    let detection_model = Model::load(std::fs::read(&detection).map_err(|e| e.to_string())?)
        .map_err(|e| format!("could not load OCR detection model: {e}"))?;
    let recognition_model = Model::load(std::fs::read(&recognition).map_err(|e| e.to_string())?)
        .map_err(|e| format!("could not load OCR recognition model: {e}"))?;
    OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
    .map_err(|e| e.to_string())
}

/// OCR raw image bytes (any format the `image` crate decodes) into text.
pub fn ocr_image_bytes(engine: &OcrEngine, bytes: &[u8]) -> Result<String, String> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("could not decode image: {e}"))?
        .into_rgb8();
    let dimensions = img.dimensions();
    let source = ImageSource::from_bytes(img.as_raw(), dimensions).map_err(|e| e.to_string())?;
    let input = engine.prepare_input(source).map_err(|e| e.to_string())?;
    engine.get_text(&input).map_err(|e| e.to_string())
}

/// OCR a single image file into a one-page [`Extracted`].
pub fn ocr_image_file(app_data_dir: &Path, bytes: &[u8]) -> Result<Extracted, String> {
    let engine = load_engine(app_data_dir)?;
    let text = ocr_image_bytes(&engine, bytes)?;
    if text.trim().is_empty() {
        return Err("no text was found in this image".into());
    }
    Ok(Extracted {
        pages: vec![Page { page: None, text }],
        extractor: "ocr",
        page_count: 1,
    })
}

/// OCR a scanned PDF by extracting its embedded page images and running OCR on
/// each. Only DCTDecode (JPEG) images are handled — the dominant encoding for
/// scanned documents; other encodings are skipped rather than mis-decoded.
pub fn ocr_pdf_images(app_data_dir: &Path, pdf_bytes: &[u8]) -> Result<Extracted, String> {
    let images = extract_pdf_jpeg_images(pdf_bytes)?;
    if images.is_empty() {
        return Err(
            "this looks like a scanned PDF, but no OCR-able page images were found in it".into(),
        );
    }
    let engine = load_engine(app_data_dir)?;
    let mut pages = Vec::new();
    for (page, jpeg) in images {
        if let Ok(text) = ocr_image_bytes(&engine, &jpeg) {
            if !text.trim().is_empty() {
                pages.push(Page {
                    page: Some(page),
                    text,
                });
            }
        }
    }
    if pages.is_empty() {
        return Err("could not read any text from the scanned PDF".into());
    }
    let page_count = pages
        .iter()
        .filter_map(|p| p.page)
        .max()
        .unwrap_or(pages.len() as i64);
    Ok(Extracted {
        pages,
        extractor: "ocr",
        page_count,
    })
}

/// Pull DCTDecode (JPEG) page images out of a PDF as `(page_number, jpeg_bytes)`.
fn extract_pdf_jpeg_images(pdf_bytes: &[u8]) -> Result<Vec<(i64, Vec<u8>)>, String> {
    let doc =
        lopdf::Document::load_mem(pdf_bytes).map_err(|e| format!("could not parse PDF: {e}"))?;
    let mut out = Vec::new();
    for (page_num, page_id) in doc.get_pages() {
        let Ok(images) = doc.get_page_images(page_id) else {
            continue;
        };
        for img in images {
            let is_jpeg = img
                .filters
                .as_ref()
                .map(|f| f.iter().any(|name| name == "DCTDecode"))
                .unwrap_or(false);
            if is_jpeg {
                out.push((page_num as i64, img.content.to_vec()));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_image_extensions() {
        for ext in ["png", "jpg", "jpeg", "tiff", "tif", "bmp", "webp", "gif"] {
            assert!(is_image_ext(ext), "{ext} should be an image");
        }
        assert!(!is_image_ext("pdf"));
        assert!(!is_image_ext("txt"));
    }

    #[test]
    fn model_specs_are_pinned_and_distinct() {
        for spec in [&DETECTION, &RECOGNITION] {
            assert_eq!(
                spec.sha256.len(),
                64,
                "{} needs a 64-char sha256",
                spec.file
            );
            assert!(spec.url.starts_with("https://"));
        }
        assert_ne!(DETECTION.file, RECOGNITION.file);
        assert_ne!(DETECTION.sha256, RECOGNITION.sha256);
    }

    #[test]
    fn models_dir_is_under_app_data() {
        let dir = models_dir(Path::new("/data"));
        assert!(dir.ends_with("models/ocr"));
    }

    #[test]
    fn extract_pdf_images_rejects_non_pdf_gracefully() {
        assert!(extract_pdf_jpeg_images(b"not a pdf at all").is_err());
    }

    /// Exercises the whole on-device OCR stack for real: download + SHA-verify
    /// the models, load them into the `rten` runtime, build the `ocrs` engine,
    /// and run detection+recognition inference on an image without crashing.
    /// (Recognition accuracy on real scans is validated by eye; this proves the
    /// integration is wired and executes.) Downloads ~12 MB on first run.
    /// Run with: `cargo test -p arya --lib -- --ignored real_ocr`.
    #[ignore = "downloads ~12MB OCR models and runs inference"]
    #[test]
    fn real_ocr_engine_loads_and_runs_inference() {
        use std::io::Cursor;

        let dir = std::env::temp_dir().join(format!("arya-ocr-test-{}", uuid::Uuid::new_v4()));
        let engine = load_engine(&dir).expect("engine loads (downloads + verifies models)");

        // Encode a small blank image and run it through the full pipeline; a
        // blank image yields empty text, but the inference path must execute.
        let img = image::RgbImage::from_pixel(96, 48, image::Rgb([255, 255, 255]));
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let text = ocr_image_bytes(&engine, png.get_ref()).expect("inference runs");
        assert!(text.trim().is_empty(), "blank image should yield no text");

        std::fs::remove_dir_all(&dir).ok();
    }
}
