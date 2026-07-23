use std::path::Path;

use anyhow::{Context, Result, bail};
use url::Url;

use super::settings::{Profile, TransformStep};

#[derive(Debug, PartialEq, Eq)]
pub struct TransformResult {
    pub output: String,
    pub path_count: usize,
}

pub fn transform_text(profile: &Profile, input: &str) -> Result<Option<TransformResult>> {
    let lines = split_lines(input);
    let path_count = lines
        .iter()
        .filter(|line| !line.content.is_empty())
        .count();

    if path_count == 0
        || lines
            .iter()
            .filter(|line| !line.content.is_empty())
            .any(|line| !is_windows_absolute_path(line.content))
    {
        return Ok(None);
    }

    let mut output = String::with_capacity(input.len());
    for line in lines {
        if line.content.is_empty() {
            output.push_str(line.content);
        } else {
            output.push_str(&apply_profile(profile, line.content)?);
        }
        output.push_str(line.ending);
    }

    Ok(Some(TransformResult { output, path_count }))
}

pub fn transform_files(profile: &Profile, paths: &[String]) -> Result<TransformResult> {
    if paths.is_empty() {
        bail!("clipboard file list is empty");
    }

    let output = paths
        .iter()
        .map(|path| apply_profile(profile, path))
        .collect::<Result<Vec<_>>>()?
        .join("\r\n");

    Ok(TransformResult {
        output,
        path_count: paths.len(),
    })
}

fn apply_profile(profile: &Profile, input: &str) -> Result<String> {
    let mut current = input.to_string();
    for step in &profile.steps {
        current = match step {
            TransformStep::Regex { regex, replacement } => {
                regex.replace_all(&current, replacement).into_owned()
            }
            TransformStep::ForwardSlash => to_forward_slash(&current),
            TransformStep::Wsl => to_wsl(&current)
                .with_context(|| format!("profile `{}` failed to convert a WSL path", profile.name))?,
            TransformStep::FileUri => to_file_uri(&current).with_context(|| {
                format!("profile `{}` failed to convert a file URI", profile.name)
            })?,
        };
    }
    Ok(current)
}

fn to_forward_slash(input: &str) -> String {
    normalize_extended_path(input).replace('\\', "/")
}

fn to_wsl(input: &str) -> Result<String> {
    let normalized = normalize_extended_path(input);

    if let Some(path) = strip_wsl_unc_prefix(&normalized) {
        let mut components = path.split(['\\', '/']);
        let distro = components.next().unwrap_or_default();
        if distro.is_empty() {
            bail!("WSL UNC path does not contain a distribution name");
        }
        let remainder = components.collect::<Vec<_>>().join("/");
        return Ok(if remainder.is_empty() {
            "/".to_string()
        } else {
            format!("/{remainder}")
        });
    }

    if let Some((drive, remainder)) = split_drive_path(&normalized) {
        let drive = drive.to_ascii_lowercase();
        let remainder = remainder.replace('\\', "/");
        return Ok(format!("/mnt/{drive}{remainder}"));
    }

    if is_unc_path(&normalized) {
        bail!("ordinary UNC paths do not have a reliable WSL mount mapping");
    }

    bail!("input is not an absolute Windows drive path");
}

fn to_file_uri(input: &str) -> Result<String> {
    let normalized = normalize_extended_path(input);
    Url::from_file_path(Path::new(&normalized))
        .map(|url| url.to_string())
        .map_err(|_| anyhow::anyhow!("input is not a valid absolute file path"))
}

fn normalize_extended_path(input: &str) -> String {
    if input
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\UNC\"))
    {
        return format!(r"\\{}", &input[8..]);
    }
    if input
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\"))
    {
        return input[4..].to_string();
    }
    input.to_string()
}

fn strip_wsl_unc_prefix(input: &str) -> Option<&str> {
    [r"\\wsl$\", r"\\wsl.localhost\"]
        .into_iter()
        .find(|prefix| {
            input
                .get(..prefix.len())
                .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
        })
        .map(|prefix| &input[prefix.len()..])
}

fn split_drive_path(input: &str) -> Option<(char, &str)> {
    let bytes = input.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
    {
        return None;
    }
    Some((bytes[0] as char, &input[2..]))
}

fn is_windows_absolute_path(input: &str) -> bool {
    if input.contains('\0') {
        return false;
    }

    let candidate = strip_outer_quotes(input);
    let normalized = normalize_extended_path(candidate);
    split_drive_path(&normalized).is_some() || is_unc_path(&normalized)
}

fn strip_outer_quotes(input: &str) -> &str {
    input
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(input)
}

fn is_unc_path(input: &str) -> bool {
    let Some(remainder) = input
        .strip_prefix(r"\\")
        .or_else(|| input.strip_prefix("//"))
    else {
        return false;
    };

    let mut components = remainder.split(['\\', '/']);
    matches!(
        (components.next(), components.next()),
        (Some(server), Some(share)) if !server.is_empty() && !share.is_empty()
    )
}

#[derive(Debug)]
struct Line<'a> {
    content: &'a str,
    ending: &'a str,
}

fn split_lines(input: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        let ending_len = match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
            b'\r' | b'\n' => 1,
            _ => {
                index += 1;
                continue;
            }
        };

        lines.push(Line {
            content: &input[start..index],
            ending: &input[index..index + ending_len],
        });
        index += ending_len;
        start = index;
    }

    if start < input.len() || input.is_empty() {
        lines.push(Line {
            content: &input[start..],
            ending: "",
        });
    }
    lines
}

#[cfg(test)]
mod tests {
    use regex::Regex;

    use super::{Profile, TransformStep, transform_files, transform_text};

    fn profile(name: &str, steps: Vec<TransformStep>) -> Profile {
        Profile {
            name: name.to_string(),
            hotkey: None,
            steps,
        }
    }

    #[test]
    fn converts_drive_and_unc_paths_to_forward_slashes() {
        let profile = profile("slash", vec![TransformStep::ForwardSlash]);
        let result = transform_text(&profile, "C:\\a\\b\r\n\\\\server\\share\\c")
            .unwrap()
            .unwrap();

        assert_eq!(result.output, "C:/a/b\r\n//server/share/c");
        assert_eq!(result.path_count, 2);
    }

    #[test]
    fn rejects_mixed_path_and_plain_text() {
        let profile = profile("slash", vec![TransformStep::ForwardSlash]);
        assert!(
            transform_text(&profile, "C:\\a\\b\r\nnot a path")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn ignores_non_ascii_plain_text_without_panicking() {
        let profile = profile("slash", vec![TransformStep::ForwardSlash]);
        assert!(transform_text(&profile, "这不是路径文本").unwrap().is_none());
    }

    #[test]
    fn preserves_blank_lines_and_line_endings() {
        let profile = profile("slash", vec![TransformStep::ForwardSlash]);
        let result = transform_text(&profile, "C:\\a\n\nD:\\b\r")
            .unwrap()
            .unwrap();
        assert_eq!(result.output, "C:/a\n\nD:/b\r");
    }

    #[test]
    fn regex_steps_run_in_order() {
        let profile = profile(
            "slash",
            vec![
                TransformStep::Regex {
                    regex: Regex::new(r#"^"(.*)"$"#).unwrap(),
                    replacement: "$1".to_string(),
                },
                TransformStep::ForwardSlash,
            ],
        );
        let result = transform_text(&profile, r#""C:\a\b""#).unwrap().unwrap();
        assert_eq!(result.output, "C:/a/b");
    }

    #[test]
    fn converts_drive_and_wsl_unc_paths_to_wsl() {
        let profile = profile("wsl", vec![TransformStep::Wsl]);
        let drive = transform_text(&profile, "D:\\Work\\a")
            .unwrap()
            .unwrap();
        let unc = transform_text(&profile, r"\\wsl.localhost\Ubuntu\home\me")
            .unwrap()
            .unwrap();

        assert_eq!(drive.output, "/mnt/d/Work/a");
        assert_eq!(unc.output, "/home/me");
    }

    #[test]
    fn rejects_ordinary_unc_for_wsl() {
        let profile = profile("wsl", vec![TransformStep::Wsl]);
        assert!(transform_text(&profile, r"\\server\share\a").is_err());
    }

    #[test]
    fn converts_extended_paths() {
        let profile = profile("slash", vec![TransformStep::ForwardSlash]);
        let result = transform_text(&profile, r"\\?\C:\long\path")
            .unwrap()
            .unwrap();
        assert_eq!(result.output, "C:/long/path");
    }

    #[test]
    fn converts_file_uri_with_encoding() {
        let profile = profile("uri", vec![TransformStep::FileUri]);
        let result = transform_text(&profile, r"C:\Program Files\a.txt")
            .unwrap()
            .unwrap();
        assert_eq!(result.output, "file:///C:/Program%20Files/a.txt");
    }

    #[test]
    fn converts_unc_file_uri() {
        let profile = profile("uri", vec![TransformStep::FileUri]);
        let result = transform_text(&profile, r"\\server\share\a.txt")
            .unwrap()
            .unwrap();
        assert_eq!(result.output, "file://server/share/a.txt");
    }

    #[test]
    fn transforms_all_files_or_returns_an_error() {
        let profile = profile("wsl", vec![TransformStep::Wsl]);
        let paths = vec!["C:\\a".to_string(), r"\\server\share\b".to_string()];
        assert!(transform_files(&profile, &paths).is_err());
    }
}
