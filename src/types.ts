export type AppView = "download" | "tasks" | "history" | "settings";

export type DownloadMode = "video" | "audio" | "subtitles" | "video+subtitles";

export type QualityPreset = "best" | "balanced" | "compact";

export type TaskStatus = "queued" | "running" | "done" | "failed" | "cancelled";

export type EnvironmentState = "ready" | "missing" | "warning";

export type FormatKind = "combined" | "audio" | "video";

export type AuthMode = "none" | "browser" | "file";

export type PlaylistScope = "video" | "playlist";

export type CookieBrowser =
  | "chrome"
  | "chromium"
  | "edge"
  | "firefox"
  | "safari"
  | "brave"
  | "opera"
  | "vivaldi"
  | "whale";

export interface EnvironmentCheck {
  id: string;
  label: string;
  status: EnvironmentState;
  version: string | null;
  detail: string;
  required: boolean;
  autoInstallAvailable: boolean;
  autoInstallLabel: string | null;
  manualInstallHint: string | null;
}

export interface EnvironmentSnapshot {
  checks: EnvironmentCheck[];
  recommendedOutputDir: string;
  note: string;
  installerAvailable: boolean;
  installerName: string | null;
}

export interface PreviewFormat {
  formatId: string;
  label: string;
  detail: string;
  size: string;
  kind: FormatKind;
}

export interface PreviewSubtitle {
  language: string;
  type: string;
  format: string;
}

export interface PlaylistEntry {
  index: number;
  title: string;
  duration: string;
}

export interface MediaPreview {
  title: string;
  creator: string;
  duration: string;
  platform: string;
  publishedAt: string;
  thumbnail: string;
  formats: PreviewFormat[];
  subtitles: PreviewSubtitle[];
  playlistEntries: PlaylistEntry[];
  sourceUrl: string;
  isPlaylist: boolean;
  totalEntries: number;
}

export interface DownloadTask {
  id: string;
  title: string;
  status: TaskStatus;
  progress: number;
  speed: string;
  eta: string;
  output: string;
  profile: string;
  sourceUrl: string;
  error: string | null;
}

export interface AuthPayload {
  authMode: AuthMode;
  browser: CookieBrowser;
  cookieFile: string;
}

export interface ParseUrlPayload extends AuthPayload {
  url: string;
  playlistScope: PlaylistScope;
}

export interface StartDownloadPayload extends AuthPayload {
  url: string;
  mode: DownloadMode;
  formatId: string | null;
  outputDir: string;
  playlistScope: PlaylistScope;
}

export interface HistoryItem {
  title: string;
  finishedAt: string;
  profile: string;
  output: string;
}

export interface SettingsGroup {
  title: string;
  description: string;
  items: Array<{
    label: string;
    value: string;
  }>;
}
