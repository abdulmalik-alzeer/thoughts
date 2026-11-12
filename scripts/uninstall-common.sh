# Shared by uninstall-macos.sh and uninstall-linux.sh (sourced, not run).
# Everything here works on explicit, fixed paths under the user's home. No wildcards.

DATA_DIR="$HOME/.thoughts"

say() { printf '%s\n' "$*"; }

parse_args() {
  PURGE=0
  for a in "$@"; do
    case "$a" in
      --purge) PURGE=1 ;;
      -h|--help)
        say "Usage: $0 [--purge]"
        say "  Stops the Thoughts background service and removes its start-at-login entry."
        say "  --purge  also removes $DATA_DIR (settings, chats, customisations) and the"
        say "           passwords and AI provider key Thoughts saved in the system keychain."
        say "  Your library (your notes folder) is never removed."
        exit 0 ;;
      *) say "Unknown option: $a (try --help)"; exit 2 ;;
    esac
  done
  if [ -z "${HOME:-}" ] || [ ! -d "$HOME" ]; then say "HOME is not set; nothing done."; exit 1; fi
}

# Stop the daemon recorded in ~/.thoughts/daemon.json, but only if that PID really is a
# Thoughts daemon (its command line ends in --daemon).
stop_daemon() {
  info="$DATA_DIR/daemon.json"
  [ -f "$info" ] || { say "Background service: not running."; return 0; }
  pid=$(sed -n 's/.*"pid"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' "$info" | head -n 1)
  [ -n "$pid" ] || { say "Background service: not running."; return 0; }
  cmd=$(ps -p "$pid" -o command= 2>/dev/null || true)
  case "$cmd" in
    *--daemon*)
      kill "$pid" 2>/dev/null || true
      i=0
      while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 25 ]; do sleep 0.2; i=$((i + 1)); done
      if kill -0 "$pid" 2>/dev/null; then kill -9 "$pid" 2>/dev/null || true; fi
      say "Background service: stopped." ;;
    *) say "Background service: not running." ;;
  esac
}

# The library path from settings.json, if one is set.
library_path() {
  [ -f "$DATA_DIR/settings.json" ] || return 0
  sed -n 's/.*"libraryPath"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$DATA_DIR/settings.json" | head -n 1
}

# Remove the data folder — never when the library lives inside it.
purge_data() {
  [ -d "$DATA_DIR" ] || { say "Data folder: nothing to remove."; return 0; }
  if [ -L "$DATA_DIR" ]; then say "Data folder: $DATA_DIR is a link; left alone. Remove it yourself if you want to."; return 0; fi
  lib=$(library_path)
  case "$lib" in
    "~/.thoughts"|"~/.thoughts/"*|"$DATA_DIR"|"$DATA_DIR/"*)
      say "Data folder: NOT removed, because your library ($lib) is inside it."
      say "Move your library somewhere else first, then run this again."
      return 0 ;;
  esac
  rm -rf -- "$HOME/.thoughts"
  say "Data folder: removed $DATA_DIR."
  [ -n "$lib" ] && say "Your library was left untouched: $lib"
  return 0
}
