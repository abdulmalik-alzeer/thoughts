use crate::{chats::ChatStore, settings::Settings};
use rand::Rng;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, Notify, RwLock};

pub struct AppState {
    pub token: String,
    pub settings: RwLock<Settings>,
    pub chats: ChatStore,
    pub events: broadcast::Sender<Value>,
    /// chat id -> kill switch for the running AI process
    pub runs: Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
    pub shutdown: Arc<Notify>,
    /// Serialises git operations on the library.
    pub git_lock: Mutex<()>,
    /// Chats whose run was cut off by a daemon restart: (chat id, title). Cleared when the chat runs again.
    pub interrupted: Mutex<Vec<(String, String)>>,
    /// AI spend per calendar month ("YYYY-MM" -> USD), kept in `usage.json`.
    pub usage: Mutex<BTreeMap<String, f64>>,
    /// One lock per library (`library_lock.of(&lib)`), held while a wrap, reorganize, sync, undo
    /// or repair is changing that library (apply through commit), so nothing else edits, syncs or
    /// shuts the daemon down halfway. Libraries are told apart by their real folder, so a wrap
    /// still running in the library the user left never turns away edits in the one now open.
    ///
    /// Lock order, everywhere: a library's lock → `sync_files` → `git_lock`. Nothing that holds
    /// `sync_files` or `git_lock` ever waits for a library's lock (they only `try_lock` it).
    pub library_lock: LibraryLocks,
    /// Taken for writing by a sync while git changes the library's files (recording them, taking in
    /// what arrived), and for reading by a short edit while it writes, so the two never overlap.
    /// While a sync waits on the network it holds neither, and edits go through (see `api::library_edit`).
    /// Whatever takes a library's lock to change it then waits for the edits already under way to
    /// finish (`AppState::drain_edits`), so none of them is caught up in its undo.
    pub sync_files: Arc<RwLock<()>>,
    /// settings.json was damaged at startup and has been moved aside.
    pub settings_recovered: AtomicBool,
    /// Automatic mini-wraps are paused after a sign-in or network failure, until any run succeeds.
    pub mini_wraps_blocked: AtomicBool,
    /// chat id -> retry state for failed automatic mini-wraps.
    pub mini_wrap_backoff: Mutex<HashMap<String, crate::wraps::Backoff>>,
    /// settings.json was damaged or missing at startup and the backup copy was put back.
    pub settings_restored: AtomicBool,
    /// settings.json was written by a newer version of the app: settings are read-only.
    pub settings_newer: AtomicBool,
    /// chat id -> the folder its assistant run is working in, while it runs.
    pub run_dirs: std::sync::Mutex<HashMap<String, std::path::PathBuf>>,
    /// Stands in for the assistant program (see `claude::RunHook`). Tests set it; the app never does.
    pub run_hook: std::sync::Mutex<Option<crate::claude::RunHook>>,
    /// Stands in for the assistant's one-shot answers (see `claude::CompleteHook`). Tests set it; the app never does.
    pub complete_hook: std::sync::Mutex<Option<crate::claude::CompleteHook>>,
    /// The assistant's last run was refused for lack of a valid sign-in, whatever the CLI's own
    /// status says (a sign-in that expired still reads as "logged in" there). Cleared by any run
    /// that works, a fresh sign-in, or "check again".
    pub auth_failed: AtomicBool,
    /// When the CLI last said "signed in", so a chat does not ask it again before every message.
    pub auth_ok_at: std::sync::Mutex<Option<std::time::Instant>>,
    /// The daemon is stopping: runs that end from here on were cut off by that, not finished.
    pub stopping: AtomicBool,
    /// Ends the sign-in attempt in progress (`claude auth login`), so there is only ever one.
    pub login: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Background summaries in flight (chat ids), plus `"sweep"` while a "new chat" pass is queued
    /// or running, so the same chat is never summarised twice at once.
    pub summarising: std::sync::Mutex<std::collections::HashSet<String>>,
    /// The spending level already announced this month: (month, level).
    pub spend_told: std::sync::Mutex<(String, crate::spend::Level)>,
    /// A limit on the plan the assistant reported, and when it was seen (seconds since 1970).
    pub plan_limit: std::sync::Mutex<Option<(crate::spend::PlanLimit, i64)>>,
    /// Access keys for other programs on this computer (`tokens.rs`).
    pub keys: crate::tokens::Store,
    /// What the assistant asks the user from inside a chat, and what was allowed there (`approvals.rs`).
    pub approvals: crate::approvals::Approvals,
    /// Codex's sign-in, as far as the app knows it (`codex.rs`).
    pub codex: crate::codex::CodexState,
}

/// Longest a wrap, undo or repair waits for edits already under way to finish (`drain_edits`).
pub const EDIT_DRAIN: std::time::Duration = std::time::Duration::from_secs(20);

/// One library's lock, and whether the one holding it is a sync started from the window (edits
/// then wait for it instead of being turned away: `api::library_edit`).
#[derive(Default)]
pub struct LibrarySlot {
    pub lock: Arc<Mutex<()>>,
    pub syncing: AtomicBool,
}

/// The libraries' locks, by real folder (`paths::canonical_lenient`), made when first asked for.
#[derive(Default)]
pub struct LibraryLocks { slots: std::sync::Mutex<HashMap<PathBuf, Arc<LibrarySlot>>> }

impl LibraryLocks {
    fn slots(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Arc<LibrarySlot>>> {
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The lock and sync flag of the library at `lib`.
    pub fn slot(&self, lib: &Path) -> Arc<LibrarySlot> {
        let key = crate::paths::canonical_lenient(lib);
        self.slots().entry(key).or_default().clone()
    }

    /// The lock of the library at `lib`.
    pub fn of(&self, lib: &Path) -> Arc<Mutex<()>> { self.slot(lib).lock.clone() }

    /// Is the library at `lib` held right now?
    pub fn busy_at(&self, lib: &Path) -> bool {
        let key = crate::paths::canonical_lenient(lib);
        self.slots().get(&key).map_or(false, |s| s.lock.try_lock().is_err())
    }

    /// Is a sync started from the window what holds the library at `lib`?
    pub fn syncing_at(&self, lib: &Path) -> bool {
        let key = crate::paths::canonical_lenient(lib);
        self.slots().get(&key).map_or(false, |s| s.syncing.load(Ordering::SeqCst))
    }

    /// Is any library held right now?
    pub fn any_busy(&self) -> bool { self.slots().values().any(|s| s.lock.try_lock().is_err()) }
}

fn month_key() -> String { chrono::Local::now().format("%Y-%m").to_string() }

impl AppState {
    pub fn load() -> anyhow::Result<Self> {
        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(40)
            .map(char::from)
            .collect();
        let (tx, _) = broadcast::channel(1024);
        let chats = ChatStore::load()?;
        let _ = chats.events.set(tx.clone());
        // Runs cut off by a restart stay listed (the flag is saved in the chat) until acknowledged.
        let mut interrupted: Vec<(String, String)> = chats.chats.try_read().map(|m| m.values()
            .filter(|c| c.interrupted).map(|c| (c.id.clone(), c.title.clone())).collect()).unwrap_or_default();
        interrupted.sort();
        let loaded = Settings::load_full(&crate::paths::settings_file());
        let (settings, settings_recovered) = (loaded.settings, loaded.reset);
        // Never under test: unit tests share one process and one settings file.
        if !cfg!(test) { crate::settings::set_read_only(loaded.newer); }
        crate::tr::set_language(&settings.language);
        crate::claude::set_reports(settings.assistant_reports);
        let usage: BTreeMap<String, f64> = match std::fs::read_to_string(crate::paths::usage_file()) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
            // First run with the ledger: start this month from the chats created in it.
            Err(_) => {
                let now = chrono::Local::now();
                let spent: f64 = chats.chats.try_read().map(|m| m.values()
                    .filter(|c| c.created.with_timezone(&chrono::Local).format("%Y-%m").to_string() == now.format("%Y-%m").to_string())
                    .map(|c| c.cost_usd).sum()).unwrap_or(0.0);
                // An empty sum is -0.0, which shows as "-$0.00"; no amount is ever below zero.
                BTreeMap::from([(month_key(), if spent > 0.0 { spent } else { 0.0 })])
            }
        };
        crate::full_access::set_on(settings.full_access());
        Ok(Self {
            token,
            settings: RwLock::new(settings),
            chats,
            events: tx,
            runs: Mutex::new(HashMap::new()),
            shutdown: Arc::new(Notify::new()),
            git_lock: Mutex::new(()),
            interrupted: Mutex::new(interrupted),
            usage: Mutex::new(usage),
            library_lock: Default::default(),
            sync_files: Default::default(),
            settings_recovered: AtomicBool::new(settings_recovered),
            mini_wraps_blocked: AtomicBool::new(false),
            mini_wrap_backoff: Mutex::new(HashMap::new()),
            settings_restored: AtomicBool::new(loaded.restored),
            settings_newer: AtomicBool::new(loaded.newer),
            run_dirs: Default::default(),
            run_hook: Default::default(),
            complete_hook: Default::default(),
            auth_failed: AtomicBool::new(false),
            auth_ok_at: Default::default(),
            stopping: AtomicBool::new(false),
            login: Default::default(),
            summarising: Default::default(),
            spend_told: std::sync::Mutex::new((month_key(), crate::spend::Level::Off)),
            plan_limit: Default::default(),
            keys: crate::tokens::Store::load(crate::paths::tokens_file()),
            approvals: crate::approvals::Approvals::at(crate::paths::home().join("chat-approvals.json")),
            codex: Default::default(),
        })
    }

    /// A state of its own for a unit test: default settings and an empty chat store in a fresh
    /// temp folder, so tests running side by side never see each other's chats.
    #[cfg(test)]
    pub fn for_test() -> Self {
        let (tx, _) = broadcast::channel(1024);
        let dir = std::env::temp_dir().join(format!("thoughts-test-chats-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let chats = ChatStore::load_from(&dir);
        let _ = chats.events.set(tx.clone());
        Self {
            token: "test-token".into(),
            settings: RwLock::new(Settings::default().with_defaults()),
            chats,
            events: tx,
            runs: Mutex::new(HashMap::new()),
            shutdown: Arc::new(Notify::new()),
            git_lock: Mutex::new(()),
            interrupted: Mutex::new(vec![]),
            usage: Mutex::new(BTreeMap::new()),
            library_lock: Default::default(),
            sync_files: Default::default(),
            settings_recovered: AtomicBool::new(false),
            mini_wraps_blocked: AtomicBool::new(false),
            mini_wrap_backoff: Mutex::new(HashMap::new()),
            settings_restored: AtomicBool::new(false),
            settings_newer: AtomicBool::new(false),
            run_dirs: Default::default(),
            run_hook: Default::default(),
            complete_hook: Default::default(),
            auth_failed: AtomicBool::new(false),
            auth_ok_at: Default::default(),
            stopping: AtomicBool::new(false),
            login: Default::default(),
            summarising: Default::default(),
            spend_told: std::sync::Mutex::new((month_key(), crate::spend::Level::Off)),
            plan_limit: Default::default(),
            keys: crate::tokens::Store::load(dir.join("tokens.json")),
            approvals: crate::approvals::Approvals::at(dir.join("chat-approvals.json")),
            codex: Default::default(),
        }
    }

    /// Is an assistant run (a chat, a workflow…) working inside `dir` right now?
    pub fn run_active_in(&self, dir: &std::path::Path) -> bool {
        self.run_dirs.lock().unwrap_or_else(|e| e.into_inner()).values().any(|d| d == dir)
    }

    pub fn emit(&self, v: Value) {
        let _ = self.events.send(v);
    }

    /// Record what a finished run cost against the current month.
    pub async fn add_usage(&self, cost_usd: f64) {
        if !(cost_usd.is_finite() && cost_usd > 0.0) { return; }
        self.add_usage_to(cost_usd, crate::paths::usage_file()).await;
        // The window shows the new figure, and the spending guard may have something to say.
        crate::spend::after_cost(self).await;
    }

    /// Add to this month and save the ledger to `file`. The save (with its fsync) runs off the
    /// async runtime, and the ledger stays locked until it is written: two runs ending at once each
    /// save a figure that holds the other's cost or is followed by one that does, so none is lost.
    async fn add_usage_to(&self, cost_usd: f64, file: PathBuf) {
        let mut u = self.usage.lock().await;
        *u.entry(month_key()).or_insert(0.0) += cost_usd;
        let Ok(text) = serde_json::to_string_pretty(&*u) else { return };
        let saved = tokio::task::spawn_blocking(move || crate::paths::write_atomic(&file, text.as_bytes(), true)).await;
        if !matches!(saved, Ok(Ok(()))) { crate::log::warn("usage: could not save this month's spending"); }
        drop(u);
    }

    /// This calendar month's AI spend in USD.
    pub async fn month_cost(&self) -> f64 {
        // `+ 0.0` turns a stored -0.0 (written by 0.0.8 on a fresh install) into 0.
        self.usage.lock().await.get(&month_key()).copied().unwrap_or(0.0).max(0.0) + 0.0
    }

    /// Is a wrap, reorganize, sync, undo or repair changing any library right now? For what must
    /// not cut any of them off (shutting down) and for checks not about one library in particular.
    pub fn library_busy(&self) -> bool { self.library_lock.any_busy() }

    /// Is something holding the library at `lib` right now (see `library_lock`)?
    pub fn library_busy_at(&self, lib: &Path) -> bool { self.library_lock.busy_at(lib) }

    /// The lock of the library open now (none set up: the lock of no folder at all).
    pub async fn open_library_lock(&self) -> Arc<Mutex<()>> {
        let lib = self.settings.read().await.library().unwrap_or_default();
        self.library_lock.of(&lib)
    }

    /// With a library's lock just taken: wait for the short edits already admitted
    /// (`api::library_edit`) to finish writing, so a wrap's undo or an undo's reset can never take
    /// one of them with it. Edits that come later see the library held and are turned away. Gives
    /// up after `EDIT_DRAIN` with a plain "still saving, try again".
    pub async fn drain_edits(&self) -> anyhow::Result<()> { self.drain_edits_within(EDIT_DRAIN).await }

    pub(crate) async fn drain_edits_within(&self, wait: std::time::Duration) -> anyhow::Result<()> {
        match tokio::time::timeout(wait, self.sync_files.write()).await {
            Ok(_writing) => Ok(()),
            Err(_) => Err(crate::tr::err("busy.saving", &[])),
        }
    }

    /// Forget that a chat's run was cut off (the user has seen it, or the chat is running again).
    pub async fn ack_interrupted(&self, chat_id: &str) {
        self.interrupted.lock().await.retain(|(c, _)| c != chat_id);
        if self.chats.get(chat_id).await.map_or(false, |c| c.interrupted) {
            if let Some(ch) = self.chats.update(chat_id, |c| c.interrupted = false).await {
                self.emit(serde_json::json!({"type": "chat.updated", "chat": ch.summary()}));
            }
        }
    }

    /// A run finished successfully: sign-in and network evidently work again.
    pub fn note_run_ok(&self) {
        self.mini_wraps_blocked.store(false, Ordering::Relaxed);
        // Something went through, so a refusal for the plan's limit is over (a warning stays until its reset).
        if self.plan_limit.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(false, |(l, _)| l.reached) { crate::spend::set_plan_limit(self, None); }
        if self.auth_failed.swap(false, Ordering::Relaxed) { self.emit(serde_json::json!({"type": "auth.changed", "loggedIn": true})); }
    }

    /// A run was refused because the assistant is not (or no longer) signed in.
    pub fn note_auth_failed(&self) {
        *self.auth_ok_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if !self.auth_failed.swap(true, Ordering::Relaxed) { self.emit(serde_json::json!({"type": "auth.changed", "loggedIn": false})); }
    }

    /// Forget what is remembered about the sign-in, so the next look asks the CLI afresh.
    pub fn forget_auth(&self) {
        self.auth_failed.store(false, Ordering::Relaxed);
        *self.auth_ok_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn notify(&self, title: &str, body: &str) {
        self.emit(serde_json::json!({"type": "notify", "title": title, "body": body}));
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_month_with_nothing_spent_is_zero_never_minus_zero() {
        let st = AppState::for_test();
        // 0.0.8 stored the sum of no chats, which Rust makes -0.0 and the window then showed as "-$0.00".
        st.usage.lock().await.insert(month_key(), -0.0);
        let shown = serde_json::to_string(&st.month_cost().await).unwrap();
        assert_eq!(shown, "0.0");
        assert_eq!(crate::spend::usage_json(&st).await["monthCostUsd"].to_string(), "0.0");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn runs_ending_at_once_each_count_and_the_saved_ledger_has_them_all() {
        let st = Arc::new(AppState::for_test());
        let dir = std::env::temp_dir().join(format!("thoughts-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("usage.json");
        let mut tasks = vec![];
        for _ in 0..20 {
            let (st, file) = (st.clone(), file.clone());
            tasks.push(tokio::spawn(async move { st.add_usage_to(0.25, file).await }));
        }
        for t in tasks { t.await.unwrap(); }
        assert!((st.month_cost().await - 5.0).abs() < 1e-9);
        let saved: BTreeMap<String, f64> = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert!((saved[&month_key()] - 5.0).abs() < 1e-9, "the last save holds every run's cost: {saved:?}");
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir(&dir);
    }
}
