#!/bin/sh
# Thoughts — clean up after uninstalling on macOS.
#
#   sh uninstall-macos.sh            stop the background service, remove the start-at-login entry
#   sh uninstall-macos.sh --purge    …and also remove ~/.thoughts (settings, chats, customisations)
#                                    and the passwords and AI provider key saved in the keychain
#
# Your library (your notes folder) is never removed. The app itself is not removed either:
# drag Thoughts from Applications to the Trash.
set -eu
. "$(dirname "$0")/uninstall-common.sh"
parse_args "$@"

stop_daemon

# Start-at-login entry (LaunchAgent).
PLIST="$HOME/Library/LaunchAgents/app.thoughts.daemon.plist"
launchctl bootout "gui/$(id -u)/app.thoughts.daemon" 2>/dev/null || true
if [ -f "$PLIST" ]; then
  rm -f -- "$PLIST"
  say "Start at login: removed."
else
  say "Start at login: nothing to remove."
fi

if [ "$PURGE" -eq 1 ]; then
  purge_data
  # Saved passwords in the login keychain, one entry per call: sync passwords (service
  # "thoughts-git", one per backup address) and the AI provider key (service "thoughts-ai").
  for service in thoughts-git thoughts-ai; do
    n=0
    while [ "$n" -lt 50 ] && security delete-generic-password -s "$service" >/dev/null 2>&1; do n=$((n + 1)); done
    if [ "$n" -gt 0 ]; then
      case "$service" in
        thoughts-git) say "Keychain: removed $n saved sync password(s)." ;;
        *) say "Keychain: removed the saved AI provider key." ;;
      esac
    fi
  done
else
  say "Data folder: kept ($DATA_DIR). Run again with --purge to remove it."
fi

say "Done. To remove the app, drag Thoughts from Applications to the Trash."
