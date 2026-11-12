//! `POST /api/images`: a picture pasted or dropped on a terminal pane, for
//! agent CLIs such as Claude Code and Codex. Only the page has the picture,
//! and the program in the pane reads files. The server saves the picture in
//! its state directory and answers with the path; the page pastes that path
//! into the terminal, as a native terminal does for a dropped file
//! (src/terminal/image-input.ts).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use serde_json::json;

use super::{host_allowed, is_json, no_store, origin_allowed, Shared};

pub(super) const PATH: &str = "/api/images";
/// Keep in step with MAX_IMAGE_BYTES in src/terminal/image-input.ts.
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
/// Base64 is a third larger than the picture; the rest is the JSON around it.
pub(super) const MAX_BODY_BYTES: usize = MAX_IMAGE_BYTES.div_ceil(3) * 4 + 1024;
/// A picture older than this goes when the next one comes.
const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// The folder in the state directory.
const FOLDER: &str = "images";

#[derive(Debug, PartialEq, Eq)]
enum ImageError {
    BadBody,
    TooLarge,
    NotImage,
    Io,
}

impl IntoResponse for ImageError {
    fn into_response(self) -> Response {
        match self {
            ImageError::BadBody => StatusCode::BAD_REQUEST,
            ImageError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ImageError::NotImage => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ImageError::Io => StatusCode::INTERNAL_SERVER_ERROR,
        }
        .into_response()
    }
}

pub(super) fn routes() -> Router<Shared> {
    Router::new().route(PATH, post(upload).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)))
}

/// The file extension of a picture, read from its first bytes: the page's
/// word for the type is not trusted.
fn extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Deletes the pictures in `folder` that are older than KEEP.
fn prune(folder: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age > KEEP));
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Saves a picture in the state directory, for the user only. Returns its path.
fn save(state_dir: &Path, bytes: &[u8], now: SystemTime) -> Result<PathBuf, ImageError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageError::TooLarge);
    }
    let extension = extension(bytes).ok_or(ImageError::NotImage)?;
    let folder = state_dir.join(FOLDER);
    fs::create_dir_all(&folder).map_err(|_| ImageError::Io)?;
    prune(&folder, now);
    // The time sorts the files; the random part keeps two pictures of one
    // millisecond apart.
    let mut random = [0u8; 4];
    getrandom::fill(&mut random).map_err(|_| ImageError::Io)?;
    let millis = now.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis());
    let name = format!("image-{millis}-{:08x}.{extension}", u32::from_be_bytes(random));
    let path = folder.join(name);
    let mut file = crate::state_dir::private_file().write(true).create_new(true).open(&path).map_err(|_| ImageError::Io)?;
    file.write_all(bytes).map_err(|_| ImageError::Io)?;
    // The program in the pane runs in its own folder: it needs the full path.
    Ok(std::path::absolute(&path).unwrap_or(path))
}

#[derive(Deserialize)]
struct ImageBody {
    /// The picture's bytes in base64.
    data: String,
}

/// Like the Git actions: JSON from the dashboard's own page only.
async fn upload(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    if !is_json(&headers) || !(state.trusted || (host_allowed(&state, &headers) && origin_allowed(&state, &headers))) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let state_dir = state.state_dir.clone();
    let saved = tokio::task::spawn_blocking(move || {
        let ImageBody { data } = serde_json::from_slice(&body).map_err(|_| ImageError::BadBody)?;
        let bytes = STANDARD.decode(data).map_err(|_| ImageError::BadBody)?;
        save(&state_dir, &bytes, SystemTime::now())
    });
    match saved.await.unwrap_or(Err(ImageError::Io)) {
        Ok(path) => no_store(Json(json!({ "path": super::docs::shown_path(&path) }))),
        Err(error) => error.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::server::tests::test_state;
    use crate::server::AppState;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const ORIGIN: &str = "http://127.0.0.1:4317";

    #[test]
    fn the_extension_comes_from_the_first_bytes() {
        assert_eq!(extension(PNG), Some("png"));
        assert_eq!(extension(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
        assert_eq!(extension(b"GIF89a.."), Some("gif"));
        assert_eq!(extension(b"RIFF\x10\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(extension(b"RIFF\x10\0\0\0WAVEfmt "), None);
        assert_eq!(extension(b"<svg onload=alert(1)>"), None);
        assert_eq!(extension(b""), None);
    }

    #[test]
    fn a_picture_is_saved_in_the_state_directory_under_a_new_name() {
        let state = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let first = save(state.path(), PNG, now).unwrap();
        let second = save(state.path(), PNG, now).unwrap();
        assert_ne!(first, second, "the same millisecond gives two files");
        assert!(first.is_absolute());
        assert_eq!(first.parent().unwrap(), std::path::absolute(state.path().join("images")).unwrap());
        assert_eq!(first.extension().unwrap(), "png");
        assert_eq!(fs::read(&first).unwrap(), PNG);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&first).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn what_is_not_a_picture_or_too_large_is_not_saved() {
        let state = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        assert_eq!(save(state.path(), b"#!/bin/sh\nrm -rf ~\n", now), Err(ImageError::NotImage));
        let mut large = PNG.to_vec();
        large.resize(MAX_IMAGE_BYTES + 1, 0);
        assert_eq!(save(state.path(), &large, now), Err(ImageError::TooLarge));
        assert!(!state.path().join("images").exists());
    }

    #[test]
    fn old_pictures_go_when_a_new_one_comes() {
        let state = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let saved_at = |age: Duration| {
            let path = save(state.path(), PNG, now).unwrap();
            fs::File::options().write(true).open(&path).unwrap().set_modified(now - age).unwrap();
            path
        };
        let old = saved_at(KEEP + Duration::from_secs(60));
        let recent = saved_at(KEEP - Duration::from_secs(60));
        let new = save(state.path(), PNG, now).unwrap();
        assert!(!old.exists());
        assert!(recent.exists() && new.exists());
    }

    fn app(state_dir: &Path, trusted: bool) -> Router {
        let state = AppState { state_dir: state_dir.to_owned(), trusted, ..test_state(&[ORIGIN]) };
        routes().with_state(Arc::new(state))
    }

    fn request(headers: &[(&str, &str)], body: String) -> Request<Body> {
        let mut builder = Request::builder().method("POST").uri(PATH);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(Body::from(body)).unwrap()
    }

    fn picture() -> String {
        format!(r#"{{"data":"{}"}}"#, STANDARD.encode(PNG))
    }

    const OWN_PAGE: [(&str, &str); 3] = [("host", "127.0.0.1:4317"), ("origin", ORIGIN), ("content-type", "application/json")];

    #[tokio::test]
    async fn the_dashboards_page_gets_the_saved_path() {
        let state = tempfile::tempdir().unwrap();
        let response = app(state.path(), false).oneshot(request(&OWN_PAGE, picture())).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let answer: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let path = PathBuf::from(answer["path"].as_str().unwrap());
        assert_eq!(fs::read(&path).unwrap(), PNG);
    }

    #[tokio::test]
    async fn another_site_cannot_save_a_file() {
        let state = tempfile::tempdir().unwrap();
        let refused: [&[(&str, &str)]; 4] = [
            &[("host", "127.0.0.1:4317"), ("origin", "http://evil.example"), ("content-type", "application/json")],
            &[("host", "evil.example"), ("origin", ORIGIN), ("content-type", "application/json")],
            // A form on another site can POST this without a preflight.
            &[("host", "127.0.0.1:4317"), ("origin", ORIGIN), ("content-type", "text/plain")],
            &[("host", "127.0.0.1:4317"), ("content-type", "application/json")],
        ];
        for headers in refused {
            let response = app(state.path(), false).oneshot(request(headers, picture())).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{headers:?}");
        }
        assert!(!state.path().join("images").exists());
    }

    #[tokio::test]
    async fn a_remote_windows_server_trusts_its_private_socket() {
        let state = tempfile::tempdir().unwrap();
        let headers = [("host", "westerm"), ("content-type", "application/json")];
        let response = app(state.path(), true).oneshot(request(&headers, picture())).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_bad_body_gets_its_own_status() {
        let state = tempfile::tempdir().unwrap();
        let status = |body: String| {
            let app = app(state.path(), false);
            async move { app.oneshot(request(&OWN_PAGE, body)).await.unwrap().status() }
        };
        assert_eq!(status("{}".to_owned()).await, StatusCode::BAD_REQUEST);
        assert_eq!(status(r#"{"data":"not base64!"}"#.to_owned()).await, StatusCode::BAD_REQUEST);
        assert_eq!(status(format!(r#"{{"data":"{}"}}"#, STANDARD.encode("plain text"))).await, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(status("x".repeat(MAX_BODY_BYTES + 1)).await, StatusCode::PAYLOAD_TOO_LARGE);
    }
}
