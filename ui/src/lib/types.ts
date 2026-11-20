// Shapes of what the background service sends, written by hand from its Rust types and `json!`
// replies (crates/thoughtsd). Keep a field here in step with its source, named beside each type.
//
// Rust `Option<T>` that is always written arrives as `T | null`; one written only when set
// (`skip_serializing_if`) is an optional field here. Times are ISO 8601 strings (chrono, UTC).

/** An ISO 8601 date-time, e.g. "2026-11-17T09:30:00Z". */
export type IsoTime = string;

// ---------- chats (chats.rs, api.rs `chat_get` / `chat_list`) ----------

export type MsgRole = 'user' | 'assistant' | 'tool' | 'tool_result' | 'system' | 'error';

/** One message of a chat (`chats::Msg`). */
export type ChatMessage = {
  id: string;
  role: MsgRole | (string & {});
  text: string;
  /** Tool calls and results: what the assistant program reported about it (name, input, …). */
  tool?: Record<string, any>;
  /** Attachment file names (`/api/chats/:id/attachments/:name`). */
  attachments?: string[];
  ts: IsoTime;
};

/** What a chat runs with (`chats::RunOptions`). */
export type RunOptions = {
  model: string | null;
  effort: string | null;
  permissionMode: string | null;
  /** The assistant may search the web and read web pages without asking. */
  web: boolean;
};

/** "chat" | "wrap" | "customize" | "setup" | "module" | "reorganize" */
export type ChatKind = 'chat' | 'wrap' | 'customize' | 'setup' | 'module' | 'reorganize' | (string & {});

/** What the assistant has written so far in a running answer (`chats::Partial`, sent by `chat_get`
 *  only while the chat is running). `seq` counts the `chat.delta` pieces this run produced,
 *  `thinkingSeq` the `chat.thinking` ones. */
export type ChatPartial = { text: string; seq: number; thinking: string; thinkingSeq?: number };

/** One chat in full: `GET /api/chats/:id` (`chats::Chat`). */
export type Chat = {
  schemaVersion: number;
  id: string;
  title: string;
  /** The title the app last gave it itself; while `title` still equals this, the app may improve it. */
  autoTitle?: string;
  pinned: boolean;
  archived: boolean;
  /** null = the library itself. */
  projectId: string | null;
  kind: ChatKind;
  /** "claude" | "codex" */
  provider: string;
  sessionId: string | null;
  sessionIds?: string[];
  created: IsoTime;
  updated: IsoTime;
  messages: ChatMessage[];
  options: RunOptions;
  miniWrap: string | null;
  miniWrapAt: IsoTime | null;
  wrappedAt: IsoTime | null;
  wrappedSummary?: string;
  /** The library folder it belongs to; absent when tied to none (customising the app). */
  library?: string;
  dirty: boolean;
  running: boolean;
  /** The last run was cut off by a restart and the user has not seen that yet. */
  interrupted: boolean;
  costUsd: number;
  /** Kind-specific data (wrap: chat ids and `phase`; reorganize: branches). */
  meta: Record<string, any> | null;
  /** The predicted next message after the last answer. */
  suggestion?: string;
  /** Tokens used, where the assistant reports tokens but no cost (Codex). */
  usageTokens?: number;
  /** Only while `running`. */
  partial?: ChatPartial;
};

/** One item of `GET /api/chats`, and the `chat` of `chat.updated` / `chat.done` (`ChatMeta::summary`). */
export type ChatSummary = {
  id: string;
  title: string;
  projectId: string | null;
  kind: ChatKind;
  provider: string;
  created: IsoTime;
  updated: IsoTime;
  running: boolean;
  interrupted: boolean;
  /** Counts toward the next Wrap. */
  unwrapped: boolean;
  hasMiniWrap: boolean;
  /** The person's messages and the replies (tool calls and results not counted). */
  messageCount: number;
  pinned: boolean;
  archived: boolean;
  /** Where a wrap or reorganize stands (preview, applying, review, interrupted…); null for others. */
  phase: string | null;
  /** Every permission check is off in this chat. */
  bypass: boolean;
};

// ---------- settings (settings.rs, `GET /api/settings`) ----------

export type Project = { id: string; name: string; path: string };

/** Per-library settings kept while another library is open (`settings::LibrarySettings`). */
export type LibrarySettings = {
  sync: SyncSettings;
  keepHistory: boolean;
  privatePaths: string[];
  profession: string | null;
  modules: Record<string, boolean>;
  projects: Project[];
  lastWrap: IsoTime | null;
};

export type LibraryEntry = { id: string; name: string; path: string; parked?: LibrarySettings };

export type ClaudeSettings = {
  binary: string | null;
  model: string | null;
  effort: string | null;
  permissionMode: string | null;
  miniWrapModel: string | null;
  baseUrl: string | null;
  hasApiToken: boolean;
  suggestNext: boolean;
};

export type CodexSettings = { binary: string | null; model: string | null; effort: string | null; miniWrapModel: string | null };

export type AiSettings = { primary: string; secondary: string | null; claude: ClaudeSettings; codex: CodexSettings };

export type SyncSettings = { mode: string; credentialStore: string; mainLocation: string | null; allowCloud: boolean; target: string };

export type Settings = {
  schemaVersion: number;
  onboarded: boolean;
  userName: string;
  /** "en" | "ar" */
  language: string;
  theme: string;
  mode: string;
  libraryPath: string | null;
  ai: AiSettings;
  sync: SyncSettings;
  keepHistory: boolean;
  modules: Record<string, boolean>;
  daemon: { runAtLogin: boolean; keepRunning: boolean };
  notifications: boolean;
  chatSteps: boolean;
  idleMinutes: number;
  wrapNagThreshold: number;
  chatRetentionDays: number;
  autoSummaries: boolean;
  projects: Project[];
  lastWrap: IsoTime | null;
  search: { extensionRepo: string; fileThreshold: number; promptDismissed: boolean; installOffered: boolean };
  privatePaths: string[];
  strictPrivate?: boolean;
  usageSignals: boolean;
  profession: string | null;
  accessibility: { zoom: number; reduceMotion: boolean; underlineLinks: boolean; strongFocus: boolean; textSpacing: boolean };
  /** weekStart / weekend: day numbers; dates: how dates are written; alarm: "HH:MM" reminder time. */
  calendar: { weekStart: number | null; weekend: number[] | null; dates: string; alarm: string | null };
  releaseCheck: boolean;
  assistantReports: boolean;
  spending: { monthlyLimitUsd: number | null; warnPercent: number; wholeComputer: boolean };
  capture: { shortcutOn: boolean; shortcut: string };
  libraries: LibraryEntry[];
  /** Full access (`settings::FullAccess`): `on` and `since` change only through the shell's approval. */
  fullAccess?: { on: boolean; since: string | null; sourceDir: string | null };
};

// ---------- status (`GET /api/status`, api.rs `status`) ----------

/** `claude::status`: only `installed` when the program is not found. */
export type ClaudeStatus = {
  installed: boolean;
  path?: string;
  version?: string | null;
  loggedIn?: boolean;
  authFailed?: boolean;
  /** What `claude auth status --json` said (null when it said nothing readable). */
  auth?: Record<string, any> | null;
  /** Who answers: "account" | "anthropicKey" | "service" | "incomplete". */
  provider?: 'account' | 'anthropicKey' | 'service' | 'incomplete';
};

/** `codex::status`: only `installed` when the program is not found. */
export type CodexStatus = {
  installed: boolean;
  path?: string;
  version?: string | null;
  /** Could the sign-in state be read at all? */
  known?: boolean;
  loggedIn?: boolean;
  method?: string | null;
  authFailed?: boolean;
};

/** This month's spending (`spend::usage_json`, also the `usage.changed` event). */
export type Usage = { monthCostUsd: number; limitUsd: number | null; warnPercent: number; level: 'off' | 'ok' | 'warn' | 'reached'; percent: number | null };

/** The assistant plan's own limit (`spend::plan_limit_json`); null when there is nothing to say. */
export type PlanLimit = { reached: boolean; window: string | null; resetsAt: number | null } | null;

export type Status = {
  version: string;
  onboarded: boolean;
  library: string | null;
  claude: ClaudeStatus;
  libraryId: string | null;
  libraryName: string | null;
  /** How many libraries there are. */
  libraries: number;
  codex: CodexStatus;
  /** Which assistant answers new chats: "claude" | "codex". */
  primary: string;
  git: { installed: boolean; version?: string };
  /** Chats of the open library cut off by a restart. */
  interrupted: { chatId: string; title: string }[];
  usage: Usage;
  planLimit: PlanLimit;
  settingsRecovered: boolean;
  settingsRestored: boolean;
  settingsNewer: boolean;
  profession: string | null;
  /** The id of a cut-off wrap waiting for the user's decision. */
  wrapInterrupted: string | null;
  unreadableChats: number;
  busy: boolean;
  libraryMissing: boolean;
  /** "macos" | "linux" | "windows" */
  platform: string;
  home: string | null;
  newerVersion: { version: string; url: string } | null;
};

// ---------- todos (todos.rs `list` / `parse`, `GET /api/todos`) ----------

export type TodoItem = {
  /** Line number in the file, from 0. */
  line: number;
  /** The whole task text after the checkbox, with `@every(…)` / `@due(…)`. */
  text: string;
  title: string;
  /** YYYY-MM-DD */
  due: string | null;
  /** One of todoline's REPEATS. */
  every: string | null;
  done: boolean;
  /** The `##` group it is under; '' when none. */
  group: string;
};

export type TodoList = {
  /** Path in the library, e.g. "TODO.md" or "apps/x/todo.md". */
  file: string;
  /** "area/slug" for an idea's own list, else null. */
  idea: string | null;
  groups: string[];
  items: TodoItem[];
};

// ---------- library files (library.rs `read`, `GET /api/library/file`) ----------

export type LibraryFile = {
  path: string;
  content: string;
  /** The note details at the top, parsed (null when there are none). */
  frontmatter: Record<string, any> | null;
  body: string;
  /** Send back as `baseHash` when saving: the save is refused when the file changed meanwhile. */
  hash: string;
};

/** `PUT /api/library/file` */
export type SaveFileResult = { ok: true; hash: string };

// ---------- events (the service's event stream, plus the window's own) ----------

export type DaemonEvent =
  | { type: 'chat.delta'; chatId: string; text: string; seq: number }
  | { type: 'chat.thinking'; chatId: string; text: string; seq?: number }
  | { type: 'chat.message'; chatId: string; message: ChatMessage }
  | { type: 'chat.message.changed'; chatId: string; message: ChatMessage }
  | { type: 'chat.message.removed'; chatId: string; messageId: string }
  | { type: 'chat.suggestion'; chatId: string; text: string }
  | { type: 'chat.updated'; chat?: ChatSummary }
  | { type: 'chat.done'; chatId: string; ok: boolean; title: string; chat: ChatSummary }
  | { type: 'chat.removed'; chatIds: string[] }
  | { type: 'chat.persistError'; chatId: string; error: string }
  | { type: 'approval.asked'; chatId: string; id: string; title: string }
  | { type: 'approval.changed'; chatId: string }
  | { type: 'auth.changed'; provider?: string; loggedIn?: boolean }
  | { type: 'usage.changed'; usage: Usage }
  | { type: 'limit.changed'; planLimit: PlanLimit }
  | { type: 'wrap.changed'; chatId: string; phase: string | null }
  | { type: 'wrap.done'; chatId: string }
  | { type: 'wrap.failed'; chatId: string; rolledBack: boolean }
  | { type: 'wrap.review'; chatId: string }
  | { type: 'reorganize.ready'; chatId: string; ok: boolean }
  | { type: 'setup.finished'; chatId: string; ok: boolean }
  | { type: 'library.switched'; id: string | null }
  | { type: 'release.newer'; version: string; url: string }
  | { type: 'layer.drafts'; setAside: string; id: string }
  | { type: 'workflow.result'; file: string; hash: string; name: string; outcome: string; chatId: string | null; retry: boolean; at: string | null }
  | { type: 'notify'; title: string; body: string }
  | { type: 'history.changed' | 'layer.changed' | 'layer.pending' | 'libraries.changed' | 'library.changed' | 'meter.changed' | 'settings.changed' | 'todos.changed' | 'workflows.changed' | 'resync' }
  // Made by the window itself (api.ts), not by the service.
  | { type: 'connection'; online: boolean }
  | { type: 'daemon.stale'; stale: boolean };

export type DaemonEventType = DaemonEvent['type'];
