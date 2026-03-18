use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

#[derive(Default)]
struct AppState {
    tasks: Arc<Mutex<HashMap<String, DownloadTask>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentCheck {
    id: String,
    label: String,
    status: String,
    version: Option<String>,
    detail: String,
    required: bool,
    auto_install_available: bool,
    auto_install_label: Option<String>,
    manual_install_hint: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentSnapshot {
    checks: Vec<EnvironmentCheck>,
    recommended_output_dir: String,
    note: String,
    installer_available: bool,
    installer_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewFormat {
    format_id: String,
    label: String,
    detail: String,
    size: String,
    kind: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewSubtitle {
    language: String,
    #[serde(rename = "type")]
    subtitle_type: String,
    format: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistEntry {
    index: usize,
    title: String,
    duration: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaPreview {
    title: String,
    creator: String,
    duration: String,
    platform: String,
    published_at: String,
    thumbnail: String,
    formats: Vec<PreviewFormat>,
    subtitles: Vec<PreviewSubtitle>,
    playlist_entries: Vec<PlaylistEntry>,
    source_url: String,
    is_playlist: bool,
    total_entries: usize,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DownloadTask {
    id: String,
    title: String,
    status: String,
    progress: f32,
    speed: String,
    eta: String,
    output: String,
    profile: String,
    source_url: String,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParseUrlRequest {
    url: String,
    playlist_scope: String,
    auth_mode: String,
    browser: Option<String>,
    cookie_file: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartDownloadRequest {
    url: String,
    mode: String,
    format_id: Option<String>,
    output_dir: String,
    playlist_scope: String,
    auth_mode: String,
    browser: Option<String>,
    cookie_file: Option<String>,
}

struct ProgressUpdate {
    progress: f32,
    speed: String,
    eta: String,
}

struct DownloadAttemptResult {
    success: bool,
    error: Option<String>,
}

#[tauri::command]
fn detect_environment() -> EnvironmentSnapshot {
    build_environment_snapshot()
}

#[tauri::command]
fn install_dependency(dependency_id: String) -> Result<EnvironmentSnapshot, String> {
    if !command_exists("brew", &["--version"]) {
        return Err(
            "未检测到 Homebrew。请先手动安装 Homebrew，再按提示安装缺失依赖。".into(),
        );
    }

    let formula = match dependency_id.as_str() {
        "yt-dlp" => "yt-dlp",
        "ffmpeg" => "ffmpeg",
        "node" => "node",
        _ => return Err(format!("暂不支持自动安装 `{dependency_id}`")),
    };

    let output = Command::new("brew")
        .args(["install", formula])
        .output()
        .map_err(|error| format!("无法启动 Homebrew 安装 `{formula}`：{error}"))?;

    if !output.status.success() {
        return Err(command_error(&output.stderr, &output.stdout));
    }

    Ok(build_environment_snapshot())
}

#[tauri::command]
fn install_missing_dependencies() -> Result<EnvironmentSnapshot, String> {
    if !command_exists("brew", &["--version"]) {
        return Err(
            "未检测到 Homebrew。请先手动安装 Homebrew，再按提示安装缺失依赖。".into(),
        );
    }

    let snapshot = build_environment_snapshot();
    let mut formulas = Vec::new();

    if snapshot
        .checks
        .iter()
        .any(|check| check.id == "yt-dlp" && check.status != "ready")
    {
        formulas.push("yt-dlp");
    }

    if snapshot
        .checks
        .iter()
        .any(|check| check.id == "ffmpeg" && check.status != "ready")
    {
        formulas.push("ffmpeg");
    }

    let runtime_ready = snapshot
        .checks
        .iter()
        .any(|check| ["node", "deno", "bun", "qjs"].contains(&check.id.as_str()) && check.status == "ready");

    if !runtime_ready {
        formulas.push("node");
    }

    if formulas.is_empty() {
        return Ok(snapshot);
    }

    let output = Command::new("brew")
        .arg("install")
        .args(&formulas)
        .output()
        .map_err(|error| format!("无法启动 Homebrew 批量安装依赖：{error}"))?;

    if !output.status.success() {
        return Err(command_error(&output.stderr, &output.stdout));
    }

    Ok(build_environment_snapshot())
}

fn build_environment_snapshot() -> EnvironmentSnapshot {
    let brew_available = command_exists("brew", &["--version"]);
    let yt_dlp = binary_check(
        "yt-dlp",
        "yt-dlp",
        &["--version"],
        true,
        "下载内核。缺失时无法解析链接或执行下载。",
        if brew_available { Some("yt-dlp") } else { None },
        Some("手动安装：brew install yt-dlp"),
    );
    let ffmpeg = binary_check(
        "ffmpeg",
        "ffmpeg",
        &["-version"],
        true,
        "音视频合并、转码、封面嵌入依赖 ffmpeg。",
        if brew_available { Some("ffmpeg") } else { None },
        Some("手动安装：brew install ffmpeg"),
    );
    let ffprobe = binary_check(
        "ffprobe",
        "ffprobe",
        &["-version"],
        false,
        "用于媒体信息探测与部分后处理。",
        None,
        None,
    );

    let runtime_candidates = vec![
        binary_check(
            "node",
            "Node.js",
            &["--version"],
            false,
            "可作为 YouTube 支持的外部 JavaScript runtime。",
            if brew_available { Some("node") } else { None },
            Some("手动安装：brew install node"),
        ),
        binary_check(
            "deno",
            "Deno",
            &["--version"],
            false,
            "可作为 YouTube 支持的外部 JavaScript runtime。",
            None,
            Some("手动安装：参考 https://deno.com/"),
        ),
        binary_check(
            "bun",
            "Bun",
            &["--version"],
            false,
            "可作为 YouTube 支持的外部 JavaScript runtime。",
            None,
            Some("手动安装：参考 https://bun.sh/"),
        ),
        binary_check(
            "qjs",
            "QuickJS",
            &["--version"],
            false,
            "轻量级 JavaScript runtime，可用于 yt-dlp-ejs。",
            None,
            None,
        ),
    ];

    let runtime_ready = runtime_candidates.iter().any(|item| item.status == "ready");

    let mut checks = vec![
        yt_dlp,
        ffmpeg,
        ffprobe,
        EnvironmentCheck {
            id: "runtime".into(),
            label: "JS Runtime".into(),
            status: if runtime_ready {
                "ready".into()
            } else {
                "warning".into()
            },
            version: runtime_candidates
                .iter()
                .find(|item| item.status == "ready")
                .and_then(|item| item.version.clone()),
            detail: if runtime_ready {
                "已检测到至少一个 JavaScript runtime，可满足 YouTube 完整支持前置条件。"
                    .into()
            } else {
                "未检测到 Node.js / Deno / Bun / QuickJS。YouTube 部分能力可能不可用。"
                    .into()
            },
            required: false,
            auto_install_available: false,
            auto_install_label: None,
            manual_install_hint: Some("建议优先安装 Node.js：brew install node".into()),
        },
    ];

    checks.extend(runtime_candidates);

    EnvironmentSnapshot {
        checks,
        recommended_output_dir: recommended_output_dir(),
        note: if brew_available {
            "已检测到 Homebrew。缺失依赖可以直接在首次安装区自动安装。".into()
        } else {
            "未检测到 Homebrew。自动安装不可用，请按手动安装提示补齐依赖。".into()
        },
        installer_available: brew_available,
        installer_name: if brew_available {
            Some("Homebrew".into())
        } else {
            None
        },
    }
}

#[tauri::command]
fn parse_url(payload: ParseUrlRequest) -> Result<MediaPreview, String> {
    let normalized = normalize_url(&payload.url)?;
    let mut args = vec![
        "--dump-single-json".into(),
        "--skip-download".into(),
        "--no-warnings".into(),
        "--playlist-end".into(),
        "8".into(),
    ];

    apply_js_runtime_args(&mut args);
    apply_playlist_scope_args(&mut args, &payload.playlist_scope);
    apply_auth_args(
        &mut args,
        &payload.auth_mode,
        payload.browser.as_deref(),
        payload.cookie_file.as_deref(),
    )?;

    let output = Command::new("yt-dlp")
        .args(args)
        .arg(&normalized)
        .output()
        .map_err(|error| format!("无法启动 yt-dlp：{error}"))?;

    if !output.status.success() {
        return Err(command_error(&output.stderr, &output.stdout));
    }

    let json = String::from_utf8(output.stdout)
        .map_err(|error| format!("yt-dlp 返回了无效 JSON：{error}"))?;
    let root: Value =
        serde_json::from_str(&json).map_err(|error| format!("解析 yt-dlp JSON 失败：{error}"))?;

    Ok(build_preview(&root, normalized))
}

#[tauri::command]
fn get_tasks(state: State<AppState>) -> Vec<DownloadTask> {
    let tasks = state.tasks.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut values: Vec<DownloadTask> = tasks.values().cloned().collect();

    values.sort_by(|left, right| right.id.cmp(&left.id));
    values
}

#[tauri::command]
fn start_download(
    app: AppHandle,
    state: State<AppState>,
    payload: StartDownloadRequest,
) -> Result<DownloadTask, String> {
    let url = normalize_url(&payload.url)?;
    let output_dir = normalize_output_dir(&payload.output_dir);

    fs::create_dir_all(&output_dir)
        .map_err(|error| format!("无法创建下载目录 `{output_dir}`：{error}"))?;

    let generated_task_id = task_id();

    let task = DownloadTask {
        id: generated_task_id.clone(),
        title: infer_title_from_url(&url),
        status: "queued".into(),
        progress: 0.0,
        speed: "--".into(),
        eta: "--".into(),
        output: output_dir.clone(),
        profile: build_profile_label(
            &payload.mode,
            payload.format_id.as_deref(),
            &payload.auth_mode,
        ),
        source_url: url.clone(),
        error: None,
    };

    upsert_task(&state.tasks, &task);
    emit_task_update(&app, &task);

    let primary_args = build_download_args(&payload, &output_dir)?;
    let fallback_args = build_fallback_download_args(&payload, &output_dir)?;

    let app_handle = app.clone();
    let task_store = state.tasks.clone();
    let thread_url = url.clone();
    thread::spawn(move || {
        let mut current_task = task;
        current_task.status = "running".into();
        upsert_task(&task_store, &current_task);
        emit_task_update(&app_handle, &current_task);

        let first_attempt = run_download_attempt(
            &app_handle,
            &task_store,
            &mut current_task,
            &thread_url,
            primary_args,
        );

        let final_result = if should_retry_with_fallback(&first_attempt.error) {
            current_task.error = Some(
                "首选格式不可用，正在自动回退到兼容格式重新尝试。".into(),
            );
            upsert_task(&task_store, &current_task);
            emit_task_update(&app_handle, &current_task);

            current_task.progress = 0.0;
            current_task.speed = "--".into();
            current_task.eta = "--".into();

            run_download_attempt(
                &app_handle,
                &task_store,
                &mut current_task,
                &thread_url,
                fallback_args,
            )
        } else {
            first_attempt
        };

        if final_result.success {
            current_task.status = "done".into();
            current_task.progress = 100.0;
            current_task.speed = "--".into();
            current_task.eta = "00:00".into();
            current_task.error = None;
            upsert_task(&task_store, &current_task);
            emit_task_update(&app_handle, &current_task);
        } else {
            current_task.status = "failed".into();
            if current_task.error.is_none() {
                current_task.error = final_result.error;
            }
            upsert_task(&task_store, &current_task);
            emit_task_update(&app_handle, &current_task);
        }
    });

    Ok(get_task(&state.tasks, &generated_task_id).unwrap_or_else(|| DownloadTask {
        id: generated_task_id,
        title: infer_title_from_url(&url),
        status: "queued".into(),
        progress: 0.0,
        speed: "--".into(),
        eta: "--".into(),
        output: output_dir,
        profile: build_profile_label(
            &payload.mode,
            payload.format_id.as_deref(),
            &payload.auth_mode,
        ),
        source_url: url,
        error: None,
    }))
}

fn binary_check(
    id: &str,
    label: &str,
    args: &[&str],
    required: bool,
    detail: &str,
    auto_install_formula: Option<&str>,
    manual_install_hint: Option<&str>,
) -> EnvironmentCheck {
    match Command::new(id).args(args).output() {
        Ok(output) if output.status.success() => {
            let version = first_non_empty_line(&output.stdout)
                .or_else(|| first_non_empty_line(&output.stderr));

            EnvironmentCheck {
                id: id.into(),
                label: label.into(),
                status: "ready".into(),
                version,
                detail: detail.into(),
                required,
                auto_install_available: false,
                auto_install_label: auto_install_formula
                    .map(|formula| format!("brew install {formula}")),
                manual_install_hint: manual_install_hint.map(str::to_string),
            }
        }
        Ok(_) | Err(_) => EnvironmentCheck {
            id: id.into(),
            label: label.into(),
            status: if required {
                "missing".into()
            } else {
                "warning".into()
            },
            version: None,
            detail: detail.into(),
            required,
            auto_install_available: auto_install_formula.is_some(),
            auto_install_label: auto_install_formula
                .map(|formula| format!("brew install {formula}")),
            manual_install_hint: manual_install_hint.map(str::to_string),
        },
    }
}

fn build_preview(root: &Value, source_url: String) -> MediaPreview {
    let playlist_entries = root
        .get("entries")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .take(8)
                .enumerate()
                .map(|(index, entry)| PlaylistEntry {
                    index: index + 1,
                    title: string_from(
                        entry,
                        &["title", "id"],
                        "Untitled playlist item".into(),
                    ),
                    duration: duration_label(entry.get("duration")),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let is_playlist = !playlist_entries.is_empty();

    MediaPreview {
        title: string_from(root, &["title", "id"], "Untitled media".into()),
        creator: string_from(
            root,
            &["channel", "uploader", "playlist_uploader", "extractor"],
            "Unknown creator".into(),
        ),
        duration: duration_label(root.get("duration")),
        platform: string_from(
            root,
            &["extractor_key", "extractor", "webpage_url_domain"],
            "Unknown".into(),
        ),
        published_at: publish_label(root),
        thumbnail: thumbnail_url(root),
        formats: collect_formats(root),
        subtitles: collect_subtitles(root),
        playlist_entries,
        source_url,
        is_playlist,
        total_entries: root
            .get("playlist_count")
            .and_then(Value::as_u64)
            .map(|count| count as usize)
            .or_else(|| root.get("entries").and_then(Value::as_array).map(|entries| entries.len()))
            .unwrap_or(1),
    }
}

fn collect_formats(root: &Value) -> Vec<PreviewFormat> {
    root.get("formats")
        .and_then(Value::as_array)
        .map(|formats| {
            formats
                .iter()
                .filter(|item| item.get("ext").and_then(Value::as_str).is_some())
                .filter_map(|item| {
                    let format_id = item.get("format_id").and_then(Value::as_str)?.to_string();
                    let ext = item.get("ext").and_then(Value::as_str)?;
                    let acodec = item
                        .get("acodec")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let height = item.get("height").and_then(Value::as_u64);
                    let resolution = height
                        .map(|value| format!("{value}p"))
                        .or_else(|| {
                            item.get("format_note")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| "原始格式".into());
                    let vcodec = item
                        .get("vcodec")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let kind = if vcodec == "none" {
                        "audio"
                    } else if acodec == "none" {
                        "video"
                    } else {
                        "combined"
                    };

                    if vcodec == "images" || format_id.starts_with("sb") {
                        return None;
                    }

                    if kind == "video" {
                        return None;
                    }

                    let detail = match kind {
                        "audio" => format!("音频 / {}", acodec.to_uppercase()),
                        _ => format!("音画合流 / {}", ext.to_uppercase()),
                    };

                    Some(PreviewFormat {
                        format_id,
                        label: format!("{resolution} {}", ext.to_uppercase()),
                        detail,
                        size: byte_label(
                            item.get("filesize")
                                .and_then(Value::as_u64)
                                .or_else(|| item.get("filesize_approx").and_then(Value::as_u64)),
                        ),
                        kind: kind.into(),
                    })
                })
                .take(12)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn collect_subtitles(root: &Value) -> Vec<PreviewSubtitle> {
    let manual = root
        .get("subtitles")
        .and_then(Value::as_object)
        .map(|object| subtitle_entries(object, "manual"))
        .unwrap_or_default();
    let automatic = root
        .get("automatic_captions")
        .and_then(Value::as_object)
        .map(|object| subtitle_entries(object, "auto"))
        .unwrap_or_default();

    manual.into_iter().chain(automatic).take(8).collect()
}

fn subtitle_entries(
    map: &serde_json::Map<String, Value>,
    subtitle_type: &str,
) -> Vec<PreviewSubtitle> {
    map.iter()
        .filter_map(|(language, value)| {
            let first = value.as_array()?.first()?;
            let format = first
                .get("ext")
                .and_then(Value::as_str)
                .unwrap_or("vtt")
                .to_string();

            Some(PreviewSubtitle {
                language: language.to_string(),
                subtitle_type: subtitle_type.into(),
                format,
            })
        })
        .collect()
}

fn publish_label(root: &Value) -> String {
    ["upload_date", "release_date", "modified_date"]
        .iter()
        .find_map(|key| root.get(*key).and_then(Value::as_str))
        .map(format_date)
        .unwrap_or_else(|| "Unknown".into())
}

fn thumbnail_url(root: &Value) -> String {
    root.get("thumbnail")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            root.get("thumbnails")
                .and_then(Value::as_array)
                .and_then(|items| items.last())
                .and_then(|item| item.get("url"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            "https://images.unsplash.com/photo-1498050108023-c5249f4df085?auto=format&fit=crop&w=1200&q=80"
                .into()
        })
}

fn normalize_url(url: &str) -> Result<String, String> {
    let normalized = url.trim();

    if normalized.is_empty() {
        return Err("请先输入至少一个有效链接".into());
    }

    Ok(normalized.into())
}

fn normalize_output_dir(path: &str) -> String {
    let trimmed = path.trim();

    if trimmed.is_empty() {
        return recommended_output_dir();
    }

    expand_home_path(trimmed)
}

fn build_download_args(
    payload: &StartDownloadRequest,
    output_dir: &str,
) -> Result<Vec<String>, String> {
    let mut args = base_download_args(output_dir);

    apply_auth_args(
        &mut args,
        &payload.auth_mode,
        payload.browser.as_deref(),
        payload.cookie_file.as_deref(),
    )?;
    apply_playlist_scope_args(&mut args, &payload.playlist_scope);

    match payload.mode.as_str() {
        "audio" => {
            args.extend([
                "-f".into(),
                payload
                    .format_id
                    .clone()
                    .unwrap_or_else(|| "bestaudio/best".into()),
                "--extract-audio".into(),
                "--audio-format".into(),
                "mp3".into(),
                "--audio-quality".into(),
                "0".into(),
            ]);
        }
        "subtitles" => {
            args.extend([
                "--skip-download".into(),
                "--write-subs".into(),
                "--write-auto-subs".into(),
                "--sub-langs".into(),
                "all".into(),
                "--convert-subs".into(),
                "srt".into(),
            ]);
        }
        "video+subtitles" => {
            args.extend(selected_or_default_format(payload.format_id.as_deref(), "best"));
            args.extend([
                "--write-subs".into(),
                "--write-auto-subs".into(),
                "--sub-langs".into(),
                "all".into(),
                "--embed-subs".into(),
            ]);
        }
        _ => {
            args.extend(selected_or_default_format(payload.format_id.as_deref(), "best"));
        }
    }

    Ok(args)
}

fn build_fallback_download_args(
    payload: &StartDownloadRequest,
    output_dir: &str,
) -> Result<Vec<String>, String> {
    let mut args = base_download_args(output_dir);

    apply_auth_args(
        &mut args,
        &payload.auth_mode,
        payload.browser.as_deref(),
        payload.cookie_file.as_deref(),
    )?;
    apply_playlist_scope_args(&mut args, &payload.playlist_scope);

    match payload.mode.as_str() {
        "audio" => {
            args.extend([
                "-f".into(),
                "bestaudio/best".into(),
                "--extract-audio".into(),
                "--audio-format".into(),
                "mp3".into(),
                "--audio-quality".into(),
                "0".into(),
            ]);
        }
        "subtitles" => {
            args.extend([
                "--skip-download".into(),
                "--write-subs".into(),
                "--write-auto-subs".into(),
                "--sub-langs".into(),
                "all".into(),
                "--convert-subs".into(),
                "srt".into(),
            ]);
        }
        "video+subtitles" => {
            args.extend([
                "-f".into(),
                "best".into(),
                "--write-subs".into(),
                "--write-auto-subs".into(),
                "--sub-langs".into(),
                "all".into(),
            ]);
        }
        _ => {
            args.extend(["-f".into(), "best".into()]);
        }
    }

    Ok(args)
}

fn base_download_args(output_dir: &str) -> Vec<String> {
    let mut args = vec![
        "--newline".into(),
        "-P".into(),
        output_dir.into(),
        "-o".into(),
        "%(title)s [%(id)s].%(ext)s".into(),
        "--progress".into(),
        "--no-warnings".into(),
    ];

    apply_js_runtime_args(&mut args);
    args
}

fn selected_or_default_format(selected: Option<&str>, fallback: &str) -> Vec<String> {
    vec![
        "-f".into(),
        selected.unwrap_or(fallback).to_string(),
    ]
}

fn build_profile_label(mode: &str, format_id: Option<&str>, auth_mode: &str) -> String {
    let mode_label = match mode {
        "audio" => "仅音频",
        "subtitles" => "仅字幕",
        "video+subtitles" => "视频 + 字幕",
        _ => "视频",
    };
    let auth_label = match auth_mode {
        "browser" => "浏览器 Cookie",
        "file" => "Cookie 文件",
        _ => "无 Cookie",
    };
    let format_label = format_id.unwrap_or("自动最佳");

    format!("{mode_label} / {format_label} / {auth_label}")
}

fn run_download_attempt(
    app: &AppHandle,
    store: &Arc<Mutex<HashMap<String, DownloadTask>>>,
    task: &mut DownloadTask,
    url: &str,
    args: Vec<String>,
) -> DownloadAttemptResult {
    let mut command = Command::new("yt-dlp");
    command.args(args);
    command.arg(url);
    command.stdout(Stdio::null());
    command.stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return DownloadAttemptResult {
                success: false,
                error: Some(format!("无法启动下载任务：{error}")),
            };
        }
    };

    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            return DownloadAttemptResult {
                success: false,
                error: Some("无法读取下载输出流".into()),
            };
        }
    };

    let reader = BufReader::new(stderr);

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim().to_string();

        if trimmed.is_empty() {
            continue;
        }

        if let Some(path) = extract_output_path(&trimmed) {
            task.output = path;
            upsert_task(store, task);
            emit_task_update(app, task);
            continue;
        }

        if let Some(progress) = parse_progress(&trimmed) {
            task.progress = progress.progress;
            task.speed = progress.speed;
            task.eta = progress.eta;
            upsert_task(store, task);
            emit_task_update(app, task);
            continue;
        }

        if trimmed.contains("ERROR:") {
            task.error = Some(
                trimmed
                    .split("ERROR:")
                    .nth(1)
                    .unwrap_or(trimmed.as_str())
                    .trim()
                    .to_string(),
            );
            upsert_task(store, task);
            emit_task_update(app, task);
        }
    }

    match child.wait() {
        Ok(status) if status.success() => DownloadAttemptResult {
            success: true,
            error: None,
        },
        Ok(status) => DownloadAttemptResult {
            success: false,
            error: task
                .error
                .clone()
                .or_else(|| Some(format!("yt-dlp 退出码异常：{}", status.code().unwrap_or(-1)))),
        },
        Err(error) => DownloadAttemptResult {
            success: false,
            error: Some(format!("等待下载进程结束失败：{error}")),
        },
    }
}

fn should_retry_with_fallback(error: &Option<String>) -> bool {
    error
        .as_ref()
        .map(|message| {
            message.contains("Requested format is not available")
                || message.contains("requested format not available")
        })
        .unwrap_or(false)
}

fn parse_progress(line: &str) -> Option<ProgressUpdate> {
    if !line.starts_with("[download]") || !line.contains('%') || line.contains("Destination") {
        return None;
    }

    let after_prefix = line.trim_start_matches("[download]").trim();
    let progress_text = after_prefix.split('%').next()?.trim();
    let progress = progress_text.parse::<f32>().ok()?;

    let speed = if let Some(at_part) = after_prefix.split(" at ").nth(1) {
        at_part.split_whitespace().next().unwrap_or("--").to_string()
    } else {
        "--".into()
    };

    let eta = if let Some(eta_part) = after_prefix.split(" ETA ").nth(1) {
        eta_part.split_whitespace().next().unwrap_or("--").to_string()
    } else {
        "--".into()
    };

    Some(ProgressUpdate {
        progress,
        speed,
        eta,
    })
}

fn extract_output_path(line: &str) -> Option<String> {
    [
        "[download] Destination: ",
        "[ExtractAudio] Destination: ",
        "[Merger] Merging formats into ",
    ]
    .iter()
    .find_map(|prefix| line.strip_prefix(prefix))
    .map(|value| value.trim_matches('"').to_string())
}

fn duration_label(value: Option<&Value>) -> String {
    value
        .and_then(|item| item.as_f64())
        .map(|seconds| {
            let total = seconds.round() as u64;
            let hours = total / 3600;
            let minutes = (total % 3600) / 60;
            let secs = total % 60;

            if hours > 0 {
                format!("{hours:02}:{minutes:02}:{secs:02}")
            } else {
                format!("{minutes:02}:{secs:02}")
            }
        })
        .unwrap_or_else(|| "--:--".into())
}

fn byte_label(value: Option<u64>) -> String {
    value
        .map(|bytes| {
            const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
            let mut unit_index = 0usize;
            let mut size = bytes as f64;

            while size >= 1024.0 && unit_index < UNITS.len() - 1 {
                size /= 1024.0;
                unit_index += 1;
            }

            format!("{size:.1} {}", UNITS[unit_index])
        })
        .unwrap_or_else(|| "大小未知".into())
}

fn format_date(raw: &str) -> String {
    if raw.len() == 8 && raw.chars().all(|char| char.is_ascii_digit()) {
        return format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8]);
    }

    raw.into()
}

fn string_from(value: &Value, keys: &[&str], fallback: String) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or(fallback)
}

fn first_non_empty_line(buffer: &[u8]) -> Option<String> {
    String::from_utf8_lossy(buffer)
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
}

fn command_error(stderr: &[u8], stdout: &[u8]) -> String {
    first_non_empty_line(stderr)
        .or_else(|| first_non_empty_line(stdout))
        .unwrap_or_else(|| "yt-dlp 执行失败，但没有返回可读错误".into())
}

fn apply_auth_args(
    args: &mut Vec<String>,
    auth_mode: &str,
    browser: Option<&str>,
    cookie_file: Option<&str>,
) -> Result<(), String> {
    match auth_mode {
        "browser" => {
            let browser_name = browser
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "认证模式为浏览器 Cookie，但没有选择浏览器".to_string())?;

            args.push("--cookies-from-browser".into());
            args.push(browser_name.into());
            Ok(())
        }
        "file" => {
            let file_path = cookie_file
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(expand_home_path)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "认证模式为 Cookie 文件，但没有填写文件路径".to_string())?;

            args.push("--cookies".into());
            args.push(file_path);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn apply_playlist_scope_args(args: &mut Vec<String>, playlist_scope: &str) {
    if playlist_scope == "video" {
        args.push("--no-playlist".into());
    }
}

fn apply_js_runtime_args(args: &mut Vec<String>) {
    if let Some(runtime) = preferred_js_runtime() {
        args.push("--js-runtimes".into());
        args.push(runtime.into());
    }
}

fn preferred_js_runtime() -> Option<&'static str> {
    if command_exists("node", &["--version"]) {
        return Some("node");
    }

    if command_exists("bun", &["--version"]) {
        return Some("bun");
    }

    if command_exists("deno", &["--version"]) {
        return Some("deno");
    }

    if command_exists("qjs", &["--version"]) {
        return Some("qjs");
    }

    None
}

fn command_exists(binary: &str, args: &[&str]) -> bool {
    Command::new(binary)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn expand_home_path(path: &str) -> String {
    if path == "~" {
        return env::var("HOME").unwrap_or_else(|_| ".".into());
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return env::var("HOME")
            .map(|home| format!("{home}/{rest}"))
            .unwrap_or_else(|_| path.into());
    }

    path.into()
}

fn recommended_output_dir() -> String {
    match env::var("HOME") {
        Ok(home) => format!("{home}/Downloads/ytDownloader"),
        Err(_) => "./Downloads/ytDownloader".into(),
    }
}

fn task_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    format!("task-{now}")
}

fn infer_title_from_url(url: &str) -> String {
    url.split('/')
        .next_back()
        .filter(|segment| !segment.is_empty())
        .unwrap_or("New download")
        .to_string()
}

fn upsert_task(store: &Arc<Mutex<HashMap<String, DownloadTask>>>, task: &DownloadTask) {
    store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(task.id.clone(), task.clone());
}

fn get_task(store: &Arc<Mutex<HashMap<String, DownloadTask>>>, id: &str) -> Option<DownloadTask> {
    store
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(id)
        .cloned()
}

fn emit_task_update(app: &AppHandle, task: &DownloadTask) {
    let _ = app.emit("download-task-updated", task);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            detect_environment,
            install_dependency,
            install_missing_dependencies,
            parse_url,
            get_tasks,
            start_download
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
