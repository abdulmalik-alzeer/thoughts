#!/bin/sh
# Thoughts — clean up after uninstalling on Linux. UNTESTED: written on macOS.
#
#   sh uninstall-linux.sh            stop the background service, remove the start-at-login entry
#   sh uninstall-linux.sh --purge    …and also remove ~/.thoughts (settings, chats, customisations)
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

if [ "$PURGE" -eq 1 ]; then
  purge_data
  say "Saved sync passwords in your system keyring (named \"thoughts-git\") can be removed with your keyring app."
else
  say "Data folder: kept ($DATA_DIR). Run again with --purge to remove it."
fi

say "Done. To remove the app, use your package manager (sudo apt remove thoughts) or delete the AppImage."
