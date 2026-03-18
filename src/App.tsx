import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { startTransition, useEffect, useState } from "react";
import "./App.css";
import { fallbackEnvironment, historyData, settingsGroups } from "./mock";
import type {
  AppView,
  AuthMode,
  CookieBrowser,
  DownloadMode,
  DownloadTask,
  EnvironmentCheck,
  EnvironmentSnapshot,
  MediaPreview,
  ParseUrlPayload,
  PlaylistScope,
  StartDownloadPayload,
  TaskStatus,
} from "./types";

const modeOptions: Array<{ value: DownloadMode; label: string; hint: string }> = [
  { value: "video", label: "视频", hint: "下载所选媒体格式" },
  { value: "audio", label: "音频", hint: "对所选源格式提取音频" },
  { value: "subtitles", label: "字幕", hint: "只下载字幕，不下载媒体" },
  { value: "video+subtitles", label: "视频 + 字幕", hint: "下载媒体并附带字幕" },
];

const authOptions: Array<{ value: AuthMode; label: string; hint: string }> = [
  { value: "none", label: "不使用 Cookie", hint: "适合公开可下载内容" },
  { value: "browser", label: "从浏览器读取", hint: "适合 YouTube 登录态场景" },
  { value: "file", label: "导入 Cookie 文件", hint: "适合 Netscape 格式文件" },
];

const browserOptions: Array<{ value: CookieBrowser; label: string }> = [
  { value: "chrome", label: "Chrome" },
  { value: "chromium", label: "Chromium" },
  { value: "edge", label: "Edge" },
  { value: "firefox", label: "Firefox" },
  { value: "safari", label: "Safari" },
  { value: "brave", label: "Brave" },
  { value: "opera", label: "Opera" },
  { value: "vivaldi", label: "Vivaldi" },
  { value: "whale", label: "Whale" },
];

const playlistScopeOptions: Array<{
  value: PlaylistScope;
  label: string;
  hint: string;
}> = [
  { value: "video", label: "当前视频", hint: "优先只解析并下载当前播放的视频" },
  { value: "playlist", label: "整个播放列表", hint: "按列表容器解析和下载" },
];

function App() {
  const [activeView, setActiveView] = useState<AppView>("download");
  const [downloadMode, setDownloadMode] = useState<DownloadMode>("video");
  const [playlistScope, setPlaylistScope] = useState<PlaylistScope>("video");
  const [authMode, setAuthMode] = useState<AuthMode>("none");
  const [browser, setBrowser] = useState<CookieBrowser>("chrome");
  const [cookieFile, setCookieFile] = useState("");
  const [urlInput, setUrlInput] = useState("");
  const [saveDirectory, setSaveDirectory] = useState(
    fallbackEnvironment.recommendedOutputDir,
  );
  const [environment, setEnvironment] =
    useState<EnvironmentSnapshot>(fallbackEnvironment);
  const [preview, setPreview] = useState<MediaPreview | null>(null);
  const [selectedFormatId, setSelectedFormatId] = useState<string | null>(null);
  const [tasks, setTasks] = useState<DownloadTask[]>([]);
  const [isParsing, setIsParsing] = useState(false);
  const [isStartingDownload, setIsStartingDownload] = useState(false);
  const [isInstallingAll, setIsInstallingAll] = useState(false);
  const [parseError, setParseError] = useState("");
  const [downloadError, setDownloadError] = useState("");
  const [installError, setInstallError] = useState("");

  useEffect(() => {
    let mounted = true;

    async function loadInitialState() {
      try {
        const [snapshot, existingTasks] = await Promise.all([
          invoke<EnvironmentSnapshot>("detect_environment"),
          invoke<DownloadTask[]>("get_tasks"),
        ]);

        if (!mounted) {
          return;
        }

        setEnvironment(snapshot);
        setSaveDirectory(snapshot.recommendedOutputDir);
        setTasks(existingTasks);
      } catch {
        if (mounted) {
          setEnvironment(fallbackEnvironment);
        }
      }
    }

    void loadInitialState();

    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => void) | undefined;

    async function bindTaskEvents() {
      cleanup = await listen<DownloadTask>("download-task-updated", (event) => {
        if (cancelled) {
          return;
        }

        setTasks((current) => upsertTask(current, event.payload));
      });
    }

    void bindTaskEvents();

    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, []);

  const normalizedUrls = urlInput
    .split("\n")
    .map((url) => url.trim())
    .filter(Boolean);

  const firstUrl = normalizedUrls[0] ?? "";
  const playlistMode = detectPlaylistMode(firstUrl);
  const selectedFormat =
    preview?.formats.find((format) => format.formatId === selectedFormatId) ?? null;
  const setupChecks = buildSetupChecks(environment);
  const readySetupChecks = setupChecks.filter((item) => item.status === "ready").length;
  const missingSetupChecks = setupChecks.filter((item) => item.status !== "ready");
  const visibleFormats = preview?.formats.slice(0, 6) ?? [];
  const visibleSubtitles = preview?.subtitles.slice(0, 4) ?? [];
  const visiblePlaylistEntries = preview?.playlistEntries.slice(0, 4) ?? [];

  const statusCounts = tasks.reduce<Record<TaskStatus, number>>(
    (accumulator, task) => {
      accumulator[task.status] += 1;
      return accumulator;
    },
    {
      queued: 0,
      running: 0,
      done: 0,
      failed: 0,
      cancelled: 0,
    },
  );

  useEffect(() => {
    setPlaylistScope(playlistMode.defaultScope);
  }, [playlistMode.defaultScope, firstUrl]);

  async function handleInstallAllMissing() {
    setInstallError("");
    setIsInstallingAll(true);

    try {
      const snapshot = await invoke<EnvironmentSnapshot>("install_missing_dependencies");
      setEnvironment(snapshot);
      setSaveDirectory(snapshot.recommendedOutputDir);
    } catch (error) {
      setInstallError(stringifyError(error));
    } finally {
      setIsInstallingAll(false);
    }
  }

  async function handleParse() {
    setParseError("");
    setDownloadError("");

    if (!firstUrl) {
      setParseError("请先输入一个可解析的链接。");
      return;
    }

    setIsParsing(true);

    try {
      const nextPreview = await invoke<MediaPreview>("parse_url", {
        payload: {
          url: firstUrl,
          playlistScope,
          authMode,
          browser,
          cookieFile,
        } satisfies ParseUrlPayload,
      });

      setPreview(nextPreview);
      setSelectedFormatId(defaultFormatId(nextPreview));
    } catch (error) {
      setPreview(null);
      setSelectedFormatId(null);
      setParseError(stringifyError(error));
    } finally {
      setIsParsing(false);
    }
  }

  async function handleStartDownload() {
    setDownloadError("");

    const targetUrl = preview?.sourceUrl ?? firstUrl;

    if (!targetUrl) {
      setDownloadError("请先输入链接并完成解析。");
      return;
    }

    if (downloadMode !== "subtitles" && !selectedFormatId) {
      setDownloadError("请先从解析结果中选择一个可下载格式。");
      return;
    }

    setIsStartingDownload(true);

    try {
      const task = await invoke<DownloadTask>("start_download", {
        payload: {
          url: targetUrl,
          mode: downloadMode,
          formatId: downloadMode === "subtitles" ? null : selectedFormatId,
          outputDir: saveDirectory,
          playlistScope,
          authMode,
          browser,
          cookieFile,
        } satisfies StartDownloadPayload,
      });

      setTasks((current) => upsertTask(current, task));
      startTransition(() => setActiveView("tasks"));
    } catch (error) {
      setDownloadError(stringifyError(error));
    } finally {
      setIsStartingDownload(false);
    }
  }

  return (
    <main className="app-shell">
      <section className="hero-card">
        <div className="hero-topbar">
          <div className="hero-heading">
            <p className="eyebrow">首次安装</p>
            <h2>当前依赖信息</h2>
          </div>

          <div className="hero-summary">
            <div className="metric-chip">
              <span>安装器</span>
              <strong>{environment.installerAvailable ? "Homebrew" : "手动安装"}</strong>
            </div>
            <div className="metric-chip">
              <span>依赖就绪</span>
              <strong>
                {readySetupChecks}/{setupChecks.length}
              </strong>
            </div>
            <div className="metric-chip">
              <span>任务数</span>
              <strong>{tasks.length}</strong>
            </div>
          </div>

          {missingSetupChecks.length > 0 ? (
            <div className="setup-list compact hero-setup-list">
              {missingSetupChecks.map((check) => (
                <div key={check.id} className="setup-row compact">
                  <div className="setup-title-row">
                    <strong>{check.label}</strong>
                    <small>{check.version ?? "未检测到"}</small>
                  </div>
                  <span className={`status-badge ${check.status}`}>
                    {check.status === "missing" ? "缺失" : "提示"}
                  </span>
                </div>
              ))}
            </div>
          ) : (
            <div className="setup-ready-note">当前依赖已全部就绪，无需额外安装。</div>
          )}

          <div className="setup-action-row compact">
            <button
              type="button"
              className="secondary-action"
              onClick={() => void handleInstallAllMissing()}
              disabled={
                isInstallingAll ||
                !environment.installerAvailable ||
                missingSetupChecks.length === 0
              }
            >
              {isInstallingAll ? "安装中..." : "一键安装全部缺失依赖"}
            </button>
            {installError ? <p className="error-banner compact inline">{installError}</p> : null}
            {!environment.installerAvailable ? (
              <small className="supporting-copy">未检测到 Homebrew，当前只能手动安装。</small>
            ) : null}
          </div>
        </div>
      </section>

      <nav className="top-nav" aria-label="Primary">
        {[
          { id: "download", label: "下载" },
          { id: "tasks", label: "任务" },
          { id: "history", label: "历史" },
          { id: "settings", label: "设置" },
        ].map((tab) => (
          <button
            key={tab.id}
            type="button"
            className={tab.id === activeView ? "nav-pill active" : "nav-pill"}
            onClick={() => {
              startTransition(() => setActiveView(tab.id as AppView));
            }}
          >
            {tab.label}
          </button>
        ))}
      </nav>

      {activeView === "download" ? (
        <section className="dashboard-grid dashboard-grid-wide">
          <article className="panel composer-panel">
            <div className="panel-header">
              <div>
                <p className="eyebrow">链接输入</p>
                <h2>先解析，再选格式下载</h2>
              </div>
              <span className="panel-tag">当前解析第一条链接</span>
            </div>

            <label className="field-label" htmlFor="urls">
              视频或播放列表 URL
            </label>
            <textarea
              id="urls"
              className="url-input"
              value={urlInput}
              onChange={(event) => setUrlInput(event.currentTarget.value)}
              placeholder="粘贴一个或多个链接。当前会先解析第一条。"
            />

            <p className="helper-copy compact">
              已识别 {normalizedUrls.length} 条链接，当前解析目标：
              <strong>{firstUrl || " 未输入"}</strong>
            </p>

            {playlistMode.showScopeSelector ? (
              <div className="inline-card scope-card">
                <span className="field-label">解析范围</span>
                <div className="scope-grid">
                  {playlistScopeOptions.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      className={
                        option.value === playlistScope
                          ? "select-chip active"
                          : "select-chip"
                      }
                      onClick={() => setPlaylistScope(option.value)}
                    >
                      <strong>{option.label}</strong>
                      <span>{option.hint}</span>
                    </button>
                  ))}
                </div>
              </div>
            ) : null}

            <div className="chip-grid">
              {modeOptions.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className={
                    option.value === downloadMode ? "select-chip active" : "select-chip"
                  }
                  onClick={() => setDownloadMode(option.value)}
                >
                  <strong>{option.label}</strong>
                  <span>{option.hint}</span>
                </button>
              ))}
            </div>

            <div className="inline-card auth-card">
              <span className="field-label">认证方式</span>
              <div className="auth-grid">
                {authOptions.map((option) => (
                  <button
                    key={option.value}
                    type="button"
                    className={
                      option.value === authMode ? "select-chip active" : "select-chip"
                    }
                    onClick={() => setAuthMode(option.value)}
                  >
                    <strong>{option.label}</strong>
                    <span>{option.hint}</span>
                  </button>
                ))}
              </div>

              {authMode === "browser" ? (
                <div className="auth-detail-row">
                  <label className="field-label" htmlFor="cookie-browser">
                    浏览器
                  </label>
                  <select
                    id="cookie-browser"
                    value={browser}
                    onChange={(event) => setBrowser(event.currentTarget.value as CookieBrowser)}
                  >
                    {browserOptions.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </div>
              ) : null}

              {authMode === "file" ? (
                <div className="auth-detail-row">
                  <label className="field-label" htmlFor="cookie-file">
                    Cookie 文件路径
                  </label>
                  <input
                    id="cookie-file"
                    value={cookieFile}
                    onChange={(event) => setCookieFile(event.currentTarget.value)}
                    placeholder="例如 ~/Downloads/youtube-cookies.txt"
                  />
                </div>
              ) : null}
            </div>

            {parseError ? <p className="error-banner">{parseError}</p> : null}
            {downloadError ? <p className="error-banner">{downloadError}</p> : null}

            <div className="selection-summary">
              <span>当前选择</span>
              <strong>
                {selectedFormat
                  ? `${selectedFormat.label} / ${selectedFormat.detail}`
                  : "等待解析后选择格式"}
              </strong>
            </div>

            <div className="action-row">
              <button
                type="button"
                className="primary-action"
                onClick={() => void handleParse()}
                disabled={isParsing}
              >
                {isParsing ? "解析中..." : "解析链接"}
              </button>
              <button
                type="button"
                className="secondary-action"
                onClick={() => void handleStartDownload()}
                disabled={isStartingDownload}
              >
                {isStartingDownload ? "启动中..." : "下载所选格式"}
              </button>
            </div>
          </article>

          <article className="panel preview-panel preview-panel-wide">
            <div className="panel-header">
              <div>
                <p className="eyebrow">解析结果</p>
                <h2>{preview?.title ?? "等待解析"}</h2>
              </div>
              <span className="panel-tag">{preview?.platform ?? "yt-dlp"}</span>
            </div>

            {preview ? (
              <>
                <div
                  className="preview-cover"
                  style={{ backgroundImage: `url(${preview.thumbnail})` }}
                />

                <div className="meta-grid">
                  <div>
                    <span>作者</span>
                    <strong>{preview.creator}</strong>
                  </div>
                  <div>
                    <span>时长</span>
                    <strong>{preview.duration}</strong>
                  </div>
                  <div>
                    <span>发布日期</span>
                    <strong>{preview.publishedAt}</strong>
                  </div>
                  <div>
                    <span>内容类型</span>
                    <strong>
                      {playlistScope === "video"
                        ? "当前视频"
                        : preview.isPlaylist
                        ? `播放列表 (${preview.totalEntries} 项)`
                        : "单个媒体"}
                    </strong>
                  </div>
                </div>

                <div className="stack-section">
                  <div className="section-title-row">
                    <h3>可下载格式</h3>
                    <span className="text-meta">{preview.formats.length} 项</span>
                  </div>
                  <div className="list-stack format-grid">
                    {visibleFormats.length > 0 ? (
                      visibleFormats.map((format) => (
                        <button
                          key={format.formatId}
                          type="button"
                          className={
                            format.formatId === selectedFormatId
                              ? "list-card format-card active"
                              : "list-card format-card"
                          }
                          onClick={() => setSelectedFormatId(format.formatId)}
                        >
                          <div>
                            <strong>{format.label}</strong>
                            <p>{format.detail}</p>
                          </div>
                          <div className="format-side">
                            <span>{format.size}</span>
                            <small>{format.kind}</small>
                          </div>
                        </button>
                      ))
                    ) : (
                      <div className="empty-state">
                        当前结果没有可直接下载的媒体格式，常见于播放列表预览或站点限制。
                      </div>
                    )}
                  </div>
                </div>

                <div className="stack-section stack-two-column">
                  <section>
                    <div className="section-title-row">
                      <h3>字幕</h3>
                      <span className="text-meta">{preview.subtitles.length} 项</span>
                    </div>
                    <div className="list-stack compact">
                      {visibleSubtitles.length > 0 ? (
                        visibleSubtitles.map((subtitle) => (
                          <div
                            key={`${subtitle.language}-${subtitle.type}-${subtitle.format}`}
                            className="list-card"
                          >
                            <div>
                              <strong>{subtitle.language}</strong>
                              <p>{subtitle.type}</p>
                            </div>
                            <span>{subtitle.format}</span>
                          </div>
                        ))
                      ) : (
                        <div className="empty-state">当前内容没有可用字幕信息。</div>
                      )}
                    </div>
                  </section>

                  <section>
                    <div className="section-title-row">
                      <h3>播放列表预览</h3>
                      <span className="text-meta">
                        {preview.isPlaylist ? `${preview.totalEntries} 项` : "非播放列表"}
                      </span>
                    </div>
                    <div className="list-stack compact">
                      {visiblePlaylistEntries.length > 0 ? (
                        visiblePlaylistEntries.map((entry) => (
                          <div key={entry.index} className="list-card">
                            <div>
                              <strong>
                                #{entry.index} {entry.title}
                              </strong>
                            </div>
                            <span>{entry.duration}</span>
                          </div>
                        ))
                      ) : (
                        <div className="empty-state">
                          当前解析结果不是播放列表，或站点没有返回条目预览。
                        </div>
                      )}
                    </div>
                  </section>
                </div>
              </>
            ) : (
              <div className="empty-preview">
                <p>还没有解析结果。</p>
                <span>先解析链接，右侧会撑满展示所有可下载格式，再从中选择下载。</span>
              </div>
            )}
          </article>
        </section>
      ) : null}

      {activeView === "tasks" ? (
        <section className="content-grid">
          <article className="panel wide-panel">
            <div className="panel-header">
              <div>
                <p className="eyebrow">任务中心</p>
                <h2>下载队列总览</h2>
              </div>
              <span className="panel-tag">
                运行中 {statusCounts.running} / 失败 {statusCounts.failed}
              </span>
            </div>

            <div className="task-stats">
              <div className="metric-card small">
                <span>等待中</span>
                <strong>{statusCounts.queued}</strong>
              </div>
              <div className="metric-card small">
                <span>下载中</span>
                <strong>{statusCounts.running}</strong>
              </div>
              <div className="metric-card small">
                <span>已完成</span>
                <strong>{statusCounts.done}</strong>
              </div>
              <div className="metric-card small">
                <span>失败</span>
                <strong>{statusCounts.failed}</strong>
              </div>
            </div>

            <div className="list-stack">
              {tasks.length > 0 ? (
                tasks.map((task) => (
                  <div key={task.id} className="task-row">
                    <div className="task-main">
                      <div className="task-title-row">
                        <strong>{task.title}</strong>
                        <span className={`status-badge ${task.status}`}>
                          {task.status}
                        </span>
                      </div>
                      <div className="progress-track">
                        <span style={{ width: `${task.progress}%` }} />
                      </div>
                      <p>{task.output}</p>
                      <small className="task-profile">{task.profile}</small>
                      {task.error ? <small className="task-error">{task.error}</small> : null}
                    </div>
                    <div className="task-side">
                      <strong>{Math.round(task.progress)}%</strong>
                      <small>{task.speed}</small>
                      <small>{task.eta}</small>
                    </div>
                  </div>
                ))
              ) : (
                <div className="empty-state">还没有任务。先在下载页解析格式并发起下载。</div>
              )}
            </div>
          </article>
        </section>
      ) : null}

      {activeView === "history" ? (
        <section className="content-grid">
          <article className="panel wide-panel">
            <div className="panel-header">
              <div>
                <p className="eyebrow">历史记录</p>
                <h2>最近完成的任务</h2>
              </div>
              <span className="panel-tag">{historyData.length} 条示例记录</span>
            </div>

            <div className="list-stack">
              {historyData.map((item) => (
                <div key={`${item.title}-${item.finishedAt}`} className="history-row">
                  <div>
                    <strong>{item.title}</strong>
                    <p>{item.profile}</p>
                  </div>
                  <div className="history-meta">
                    <span>{item.finishedAt}</span>
                    <small>{item.output}</small>
                  </div>
                </div>
              ))}
            </div>
          </article>
        </section>
      ) : null}

      {activeView === "settings" ? (
        <section className="content-grid settings-grid">
          {settingsGroups.map((group) => (
            <article key={group.title} className="panel">
              <div className="panel-header">
                <div>
                  <p className="eyebrow">设置</p>
                  <h2>{group.title}</h2>
                </div>
              </div>

              <p className="supporting-copy">{group.description}</p>

              <div className="list-stack compact">
                {group.items.map((item) => (
                  <div key={item.label} className="setting-row">
                    <span>{item.label}</span>
                    <strong>{item.value}</strong>
                  </div>
                ))}
              </div>
            </article>
          ))}
        </section>
      ) : null}
    </main>
  );
}

function buildSetupChecks(environment: EnvironmentSnapshot) {
  const ytDlp = environment.checks.find((check) => check.id === "yt-dlp");
  const ffmpeg = environment.checks.find((check) => check.id === "ffmpeg");
  const runtime = environment.checks.find((check) => check.id === "runtime");

  const items: Array<EnvironmentCheck & { installTarget?: string }> = [];

  if (ytDlp) {
    items.push({ ...ytDlp, installTarget: "yt-dlp" });
  }

  if (ffmpeg) {
    items.push({ ...ffmpeg, installTarget: "ffmpeg" });
  }

  if (runtime) {
    items.push({
      ...runtime,
      label: "JavaScript Runtime",
      installTarget: runtime.status !== "ready" ? "node" : undefined,
    });
  }

  return items;
}

function defaultFormatId(preview: MediaPreview) {
  const combined = preview.formats.find((format) => format.kind === "combined");
  return combined?.formatId ?? preview.formats[0]?.formatId ?? null;
}

function upsertTask(tasks: DownloadTask[], nextTask: DownloadTask) {
  const existingIndex = tasks.findIndex((task) => task.id === nextTask.id);

  if (existingIndex === -1) {
    return [nextTask, ...tasks];
  }

  return tasks.map((task) => (task.id === nextTask.id ? nextTask : task));
}

function stringifyError(error: unknown) {
  if (typeof error === "string") {
    return error;
  }

  if (error instanceof Error) {
    return error.message;
  }

  return "发生了未知错误";
}

function detectPlaylistMode(url: string) {
  if (!url) {
    return { showScopeSelector: false, defaultScope: "video" as PlaylistScope };
  }

  try {
    const parsed = new URL(url);
    const hasVideo = parsed.searchParams.has("v");
    const hasPlaylist = parsed.searchParams.has("list");

    if (hasVideo && hasPlaylist) {
      return { showScopeSelector: true, defaultScope: "video" as PlaylistScope };
    }

    if (hasPlaylist) {
      return { showScopeSelector: false, defaultScope: "playlist" as PlaylistScope };
    }
  } catch {
    return { showScopeSelector: false, defaultScope: "video" as PlaylistScope };
  }

  return { showScopeSelector: false, defaultScope: "video" as PlaylistScope };
}

export default App;
