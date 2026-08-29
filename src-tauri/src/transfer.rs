//! File drop events and native clipboard read/write.
//!
//! Paths stay in Rust. JS receives `{ name, mime, size, body, encoding }`
//! only, with bodies capped at 8 MiB. Finder-copied files are read from
//! the pasteboard the same way as a drop. Writes with files are staged in
//! a temp dir so the OS pasteboard can hold file URLs without JS seeing them.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, WebviewWindow, WindowEvent};

static CLIPBOARD_STAGE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Per-file cap on `body`. Oversize entries keep name/mime/size and set
/// `body` to null. Same bargain as `saveFile` being text-only: UTF-8 is
/// `encoding: "text"`, anything else is `"base64"`.
pub const MAX_FILE_BODY_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileEncoding {
    Text,
    Base64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferredFile {
    pub name: String,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub size: u64,
    pub body: Option<String>,
    pub encoding: Option<FileEncoding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileDropPayload {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub files: Vec<TransferredFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardPayload {
    pub text: Option<String>,
    pub html: Option<String>,
    pub files: Vec<TransferredFile>,
}

pub fn listen_file_drop(window: &WebviewWindow) {
    let app = window.app_handle().clone();
    window.on_window_event(move |event| {
        handle_drag_drop(&app, event);
    });
}

fn handle_drag_drop(app: &AppHandle, event: &WindowEvent) {
    let WindowEvent::DragDrop(drag) = event else {
        return;
    };
    match drag {
        DragDropEvent::Enter { paths, .. } => {
            emit_drop(app, "enter", transferred_files(paths, false));
        }
        DragDropEvent::Over { .. } => {
            emit_drop(app, "over", Vec::new());
        }
        DragDropEvent::Drop { paths, .. } => {
            let app = app.clone();
            let paths = paths.clone();
            std::thread::spawn(move || {
                emit_drop(&app, "drop", transferred_files(&paths, true));
            });
        }
        DragDropEvent::Leave => {
            emit_drop(app, "leave", Vec::new());
        }
        _ => {}
    }
}

fn emit_drop(app: &AppHandle, kind: &'static str, files: Vec<TransferredFile>) {
    let _ = app.emit_to("main", "shell://file-drop", FileDropPayload { kind, files });
}

pub fn transferred_files(paths: &[PathBuf], read_body: bool) -> Vec<TransferredFile> {
    paths
        .iter()
        .filter_map(|path| file_from_path(path, read_body))
        .collect()
}

pub fn file_from_path(path: &Path, read_body: bool) -> Option<TransferredFile> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if name.is_empty() {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let size = meta.len();
    let mime = mime_for_name(&name).to_string();
    if !read_body || size > MAX_FILE_BODY_BYTES {
        return Some(TransferredFile {
            name,
            mime,
            size,
            body: None,
            encoding: None,
        });
    }
    match std::fs::read(path) {
        Ok(bytes) => {
            let size = bytes.len() as u64;
            let (body, encoding) = classify_body(&bytes);
            Some(TransferredFile {
                name,
                mime,
                size,
                body: Some(body),
                encoding: Some(encoding),
            })
        }
        Err(_) => Some(TransferredFile {
            name,
            mime,
            size,
            body: None,
            encoding: None,
        }),
    }
}

fn classify_body(bytes: &[u8]) -> (String, FileEncoding) {
    match std::str::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => (text.to_string(), FileEncoding::Text),
        _ => (STANDARD.encode(bytes), FileEncoding::Base64),
    }
}

fn mime_for_name(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    match ext.as_deref() {
        Some("txt") => "text/plain",
        Some("csv") => "text/csv",
        Some("tsv") => "text/tab-separated-values",
        Some("json") => "application/json",
        Some("html") | Some("htm") => "text/html",
        Some("xml") => "application/xml",
        Some("md") => "text/markdown",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("zip") => "application/zip",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("xls") => "application/vnd.ms-excel",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("ofx") => "application/x-ofx",
        Some("qif") => "application/x-qif",
        _ => "application/octet-stream",
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

/// When the pasteboard lists file URLs (even directories we skip), drop
/// text/html so a Finder copy cannot leak filesystem paths through the
/// string flavors.
pub fn assemble_clipboard(
    text: Option<String>,
    html: Option<String>,
    file_paths: &[PathBuf],
    read_body: bool,
) -> ClipboardPayload {
    let files = transferred_files(file_paths, read_body);
    if file_paths.is_empty() {
        ClipboardPayload {
            text: nonempty(text),
            html: nonempty(html),
            files,
        }
    } else {
        ClipboardPayload {
            text: None,
            html: None,
            files,
        }
    }
}

fn read_clipboard() -> ClipboardPayload {
    let ctx = match ClipboardContext::new() {
        Ok(ctx) => ctx,
        Err(_) => {
            return ClipboardPayload {
                text: None,
                html: None,
                files: Vec::new(),
            };
        }
    };
    let text = ctx.get_text().ok();
    let html = ctx.get_html().ok();
    let file_paths: Vec<PathBuf> = ctx
        .get_files()
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect();
    assemble_clipboard(text, html, &file_paths, true)
}

/// Empty clipboard (and any pasteboard error) resolves with empty fields.
/// It does not reject.
#[tauri::command]
pub fn shell_read_clipboard() -> Result<ClipboardPayload, String> {
    Ok(read_clipboard())
}

fn validate_clipboard_filename(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return Err("invalid file name".into());
    }
    Ok(())
}

pub fn file_bytes(file: &TransferredFile) -> Result<Vec<u8>, String> {
    let body = file
        .body
        .as_ref()
        .ok_or_else(|| format!("file \"{}\" has no body", file.name))?;
    let bytes = match file.encoding {
        Some(FileEncoding::Base64) => STANDARD
            .decode(body)
            .map_err(|e| format!("file \"{}\" is not valid base64: {e}", file.name))?,
        Some(FileEncoding::Text) | None => body.as_bytes().to_vec(),
    };
    if bytes.len() as u64 > MAX_FILE_BODY_BYTES {
        return Err(format!("file \"{}\" exceeds 8 MiB", file.name));
    }
    Ok(bytes)
}

fn clipboard_stage_dir() -> Result<PathBuf, String> {
    let mut stage = CLIPBOARD_STAGE
        .lock()
        .map_err(|e| format!("clipboard stage: {e}"))?;
    if let Some(dir) = stage.as_ref() {
        return Ok(dir.clone());
    }
    let dir = std::env::temp_dir().join(format!("app-ly-clipboard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("clipboard stage: {e}"))?;
    *stage = Some(dir.clone());
    Ok(dir)
}

fn clear_clipboard_stage() {
    let Ok(stage) = CLIPBOARD_STAGE.lock() else {
        return;
    };
    let Some(dir) = stage.as_ref() else {
        return;
    };
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub fn stage_clipboard_files(dir: &Path, files: &[TransferredFile]) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("clipboard stage: {e}"))?;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let mut paths = Vec::new();
    let mut names = std::collections::HashSet::new();
    for file in files {
        validate_clipboard_filename(&file.name)?;
        if !names.insert(file.name.as_str()) {
            return Err(format!("duplicate file name \"{}\"", file.name));
        }
        let bytes = file_bytes(file)?;
        let path = dir.join(&file.name);
        std::fs::write(&path, &bytes).map_err(|e| format!("write clipboard file: {e}"))?;
        paths.push(path.to_string_lossy().into_owned());
    }
    Ok(paths)
}

fn write_clipboard(
    text: Option<String>,
    html: Option<String>,
    files: Vec<TransferredFile>,
) -> Result<(), String> {
    let text = nonempty(text);
    let html = nonempty(html);
    let ctx = ClipboardContext::new().map_err(|e| format!("clipboard: {e}"))?;

    if text.is_none() && html.is_none() && files.is_empty() {
        ctx.clear().map_err(|e| format!("clear clipboard: {e}"))?;
        clear_clipboard_stage();
        return Ok(());
    }

    let mut contents = Vec::new();
    if let Some(text) = text {
        contents.push(ClipboardContent::Text(text));
    }
    if let Some(html) = html {
        contents.push(ClipboardContent::Html(html));
    }
    if files.is_empty() {
        clear_clipboard_stage();
    } else {
        let dir = clipboard_stage_dir()?;
        let paths = stage_clipboard_files(&dir, &files)?;
        contents.push(ClipboardContent::Files(paths));
    }

    ctx.set(contents)
        .map_err(|e| format!("write clipboard: {e}"))
}

/// Replaces the OS pasteboard. Empty `{ text, html, files }` clears it.
/// File bodies are staged in a temp dir; JS never supplies or receives paths.
#[tauri::command]
pub fn shell_write_clipboard(
    text: Option<String>,
    html: Option<String>,
    files: Option<Vec<TransferredFile>>,
) -> Result<(), String> {
    write_clipboard(text, html, files.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "app-ly-transfer-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn utf8_file_is_text() {
        let dir = temp_dir();
        let path = dir.join("note.txt");
        std::fs::write(&path, "hello").unwrap();
        let file = file_from_path(&path, true).unwrap();
        assert_eq!(file.name, "note.txt");
        assert_eq!(file.mime, "text/plain");
        assert_eq!(file.size, 5);
        assert_eq!(file.body.as_deref(), Some("hello"));
        assert_eq!(file.encoding, Some(FileEncoding::Text));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn binary_file_is_base64() {
        let dir = temp_dir();
        let path = dir.join("blob.bin");
        std::fs::write(&path, [0u8, 159, 255]).unwrap();
        let file = file_from_path(&path, true).unwrap();
        assert_eq!(file.mime, "application/octet-stream");
        assert_eq!(file.encoding, Some(FileEncoding::Base64));
        assert_eq!(file.body.as_deref(), Some("AJ//"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversize_file_omits_body() {
        let dir = temp_dir();
        let path = dir.join("big.csv");
        let bytes = vec![b'a'; (MAX_FILE_BODY_BYTES as usize) + 1];
        std::fs::write(&path, &bytes).unwrap();
        let file = file_from_path(&path, true).unwrap();
        assert_eq!(file.mime, "text/csv");
        assert_eq!(file.size, MAX_FILE_BODY_BYTES + 1);
        assert!(file.body.is_none());
        assert!(file.encoding.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enter_skips_body() {
        let dir = temp_dir();
        let path = dir.join("note.txt");
        std::fs::write(&path, "hello").unwrap();
        let file = file_from_path(&path, false).unwrap();
        assert_eq!(file.size, 5);
        assert!(file.body.is_none());
        assert!(file.encoding.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn directories_are_skipped() {
        let dir = temp_dir();
        assert!(file_from_path(&dir, true).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_path_is_skipped() {
        assert!(file_from_path(Path::new("/no/such/app-ly-file"), true).is_none());
    }

    #[test]
    fn empty_file_is_empty_text() {
        let dir = temp_dir();
        let path = dir.join("empty.txt");
        std::fs::write(&path, "").unwrap();
        let file = file_from_path(&path, true).unwrap();
        assert_eq!(file.body.as_deref(), Some(""));
        assert_eq!(file.encoding, Some(FileEncoding::Text));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_on_clipboard_hide_text() {
        let dir = temp_dir();
        let path = dir.join("a.csv");
        std::fs::write(&path, "a,b").unwrap();
        let payload = assemble_clipboard(
            Some("/Users/me/secret.csv".into()),
            Some("<p>/Users/me/secret.csv</p>".into()),
            &[path],
            true,
        );
        assert!(payload.text.is_none());
        assert!(payload.html.is_none());
        assert_eq!(payload.files.len(), 1);
        assert_eq!(payload.files[0].name, "a.csv");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn directory_on_clipboard_hides_text() {
        let dir = temp_dir();
        let payload = assemble_clipboard(
            Some(dir.to_string_lossy().into_owned()),
            None,
            &[dir.clone()],
            true,
        );
        assert!(payload.text.is_none());
        assert!(payload.files.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_clipboard_keeps_flavors() {
        let payload = assemble_clipboard(Some("hi".into()), Some("<b>hi</b>".into()), &[], true);
        assert_eq!(payload.text.as_deref(), Some("hi"));
        assert_eq!(payload.html.as_deref(), Some("<b>hi</b>"));
        assert!(payload.files.is_empty());
    }

    #[test]
    fn empty_strings_become_null() {
        let payload = assemble_clipboard(Some("".into()), Some("".into()), &[], true);
        assert!(payload.text.is_none());
        assert!(payload.html.is_none());
    }

    #[test]
    fn write_decodes_text_and_base64() {
        let text = TransferredFile {
            name: "a.txt".into(),
            mime: String::new(),
            size: 0,
            body: Some("hi".into()),
            encoding: Some(FileEncoding::Text),
        };
        assert_eq!(file_bytes(&text).unwrap(), b"hi");

        let default_text = TransferredFile {
            name: "a.txt".into(),
            mime: String::new(),
            size: 0,
            body: Some("hi".into()),
            encoding: None,
        };
        assert_eq!(file_bytes(&default_text).unwrap(), b"hi");

        let binary = TransferredFile {
            name: "a.bin".into(),
            mime: String::new(),
            size: 0,
            body: Some("AJ//".into()),
            encoding: Some(FileEncoding::Base64),
        };
        assert_eq!(file_bytes(&binary).unwrap(), [0u8, 159, 255]);
    }

    #[test]
    fn write_rejects_missing_body_and_bad_name() {
        let missing = TransferredFile {
            name: "a.txt".into(),
            mime: String::new(),
            size: 0,
            body: None,
            encoding: None,
        };
        assert!(file_bytes(&missing).unwrap_err().contains("no body"));
        assert!(validate_clipboard_filename("../escape.txt").is_err());
        assert!(validate_clipboard_filename("ok.csv").is_ok());
    }

    #[test]
    fn write_rejects_oversize_body() {
        let file = TransferredFile {
            name: "big.txt".into(),
            mime: String::new(),
            size: 0,
            body: Some("a".repeat((MAX_FILE_BODY_BYTES as usize) + 1)),
            encoding: Some(FileEncoding::Text),
        };
        assert!(file_bytes(&file).unwrap_err().contains("8 MiB"));
    }

    #[test]
    fn write_stages_files_without_paths_in_name() {
        let dir = temp_dir();
        let files = vec![TransferredFile {
            name: "export.csv".into(),
            mime: "text/csv".into(),
            size: 3,
            body: Some("a,b".into()),
            encoding: Some(FileEncoding::Text),
        }];
        let paths = stage_clipboard_files(&dir, &files).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), "a,b");
        assert_eq!(Path::new(&paths[0]).file_name().unwrap(), "export.csv");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_rejects_duplicate_names() {
        let dir = temp_dir();
        let file = TransferredFile {
            name: "a.csv".into(),
            mime: String::new(),
            size: 0,
            body: Some("x".into()),
            encoding: None,
        };
        let err = stage_clipboard_files(&dir, &[file.clone(), file]).unwrap_err();
        assert!(err.contains("duplicate"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
