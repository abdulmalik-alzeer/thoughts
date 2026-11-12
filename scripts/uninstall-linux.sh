#!/bin/sh
# Thoughts — clean up after uninstalling on Linux. UNTESTED: written on macOS.
#
#   sh uninstall-linux.sh            stop the background service, remove the start-at-login entry
#   sh uninstall-linux.sh --purge    …and also remove ~/.thoughts (settings, chats, customisations)
#                                    and the passwords and AI provider key saved in the keyring
#
# Your library (your notes folder) is never removed. The app itself is not removed either:
# use your package manager (for example: sudo apt remove thoughts) or delete the AppImage file.
set -eu
. "$(dirname "$0")/uninstall-common.sh"
parse_args "$@"

stop_daemon

# Start-at-login entry (XDG autostart).
case "${XDG_CONFIG_HOME:-}" in
  /*) CONFIG="$XDG_CONFIG_HOME" ;;
  *) CONFIG="$HOME/.config" ;;
esac
ENTRY="$CONFIG/autostart/thoughts-daemon.desktop"
if [ -f "$ENTRY" ]; then
  rm -f -- "$ENTRY"
  say "Start at login: removed."
else
  say "Start at login: nothing to remove."
fi

# An AppImage registers itself as the opener of thoughts:// links (a .deb package does that itself).
case "${XDG_DATA_HOME:-}" in
  /*) DATA_HOME="$XDG_DATA_HOME" ;;
  *) DATA_HOME="$HOME/.local/share" ;;
esac
HANDLER="$DATA_HOME/applications/thoughts-handler.desktop"
if [ -f "$HANDLER" ]; then
  rm -f -- "$HANDLER"
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$DATA_HOME/applications" >/dev/null 2>&1 || true
  say "Links (thoughts://): removed."
fi

if [ "$PURGE" -eq 1 ]; then
  purge_data
  # Saved passwords in the desktop keyring (Secret Service): sync passwords ("thoughts-git") and
  # the AI provider key ("thoughts-ai"). UNTESTED: assumes the keyring stores the service name
  # under the "service" attribute, as the keyring library the app uses does.
  if command -v secret-tool >/dev/null 2>&1; then
    for service in thoughts-git thoughts-ai; do secret-tool clear service "$service" >/dev/null 2>&1 || true; done
    say "Keyring: removed the saved sync passwords and AI provider key (thoughts-git, thoughts-ai)."
  else
    say "Saved passwords in your system keyring, named \"thoughts-git\" (sync) and \"thoughts-ai\" (AI provider key),"
    say "can be removed with your keyring app (for example Passwords and Keys)."
  fi
else
  say "Data folder: kept ($DATA_DIR). Run again with --purge to remove it."
fi

say "Done. To remove the app, use your package manager (sudo apt remove thoughts) or delete the AppImage."
