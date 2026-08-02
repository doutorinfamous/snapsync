use crate::models::{PrinterConfig, TimelapseInstance};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde::Deserialize;
use std::time::Duration;

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

pub(crate) fn encode_url_path(path: &str) -> String {
    path.split('/')
        .map(|part| utf8_percent_encode(part, PATH_SEGMENT_ENCODE_SET).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Deserialize)]
struct HttpEnvelope<T> {
    result: T,
}

#[derive(Deserialize)]
struct CameraFile {
    path: String,
    modified: f64,
    size: u64,
}

pub async fn list_camera_files_http(
    printer: &PrinterConfig,
) -> anyhow::Result<Vec<TimelapseInstance>> {
    validate_host(&printer.host)?;
    let authority = if printer.host.contains(':') && !printer.host.starts_with('[') {
        format!("[{}]", printer.host)
    } else {
        printer.host.clone()
    };
    let url = format!(
        "http://{authority}:{}/server/files/list?root=camera",
        printer.http_port
    );
    let response = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()?
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json::<HttpEnvelope<Vec<CameraFile>>>()
        .await?;

    Ok(response
        .result
        .into_iter()
        .filter(|file| {
            let extension = std::path::Path::new(&file.path)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4" | "mpeg" | "mpg" | "mov"
            )
        })
        .map(|file| {
            let encoded_path = encode_url_path(&file.path);
            let gcode_name = std::path::Path::new(&file.path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("timelapse")
                .to_string();
            TimelapseInstance {
                date_index: String::new(),
                gcode_name,
                thumbnail_path: std::path::Path::new(&file.path)
                    .with_extension("jpg")
                    .to_string_lossy()
                    .to_string(),
                timelapse_dir: "camera".into(),
                video_path: format!("camera/{}", file.path),
                video_local_url_suffix: format!("/server/files/camera/{encoded_path}"),
                video_file_size: file.size,
                unix_timestamp_s: file.modified as i64,
                ..TimelapseInstance::default()
            }
        })
        .collect())
}

pub async fn verify_http_connection(printer: &PrinterConfig) -> anyhow::Result<()> {
    let _ = list_camera_files_http(printer).await?;
    Ok(())
}

fn validate_host(host: &str) -> anyhow::Result<()> {
    let host = host.trim();
    if host.is_empty()
        || host.len() > 253
        || host.contains('/')
        || host.contains('\\')
        || host.contains('@')
        || host.chars().any(char::is_whitespace)
    {
        anyhow::bail!("invalid printer address");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_url_instead_of_host() {
        assert!(validate_host("http://192.168.1.2").is_err());
        assert!(validate_host("192.168.1.2").is_ok());
        assert!(validate_host("U1-ABC.local").is_ok());
    }

    #[test]
    fn encodes_spaces_as_url_path_not_form_data() {
        assert_eq!(
            encode_url_path("Goku Hair Left/file+name.mp4"),
            "Goku%20Hair%20Left/file+name.mp4"
        );
    }
}
