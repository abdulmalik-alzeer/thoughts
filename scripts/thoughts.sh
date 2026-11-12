#!/bin/sh
# thoughts — add todos and notes to Thoughts, or read from it, from a terminal or a script.
#
#   thoughts.sh todo Call the bank tomorrow   add a todo (a date said at the end becomes its due date)
#   thoughts.sh todo --list projects/x/todo.md Send the draft
#                                             …to another task list
#   thoughts.sh note An idea for the shop     save a quick note in the inbox
#   echo "longer text" | thoughts.sh note     …or what is piped in
#   thoughts.sh todos                         the task lists
#   thoughts.sh search words to look for      search the library
#   thoughts.sh read path/to/note.md          one file of the library
#   thoughts.sh check                         see that Thoughts answers and the key works
#
# It needs an access key: in Thoughts, Settings → Privacy & cost → Keys for other programs. A key
# made for adding cannot read, and one made for reading cannot add; neither can change settings,
# start the assistant or open private folders. Give the key as THOUGHTS_KEY, or put it alone in
# ~/.config/thoughts/key (readable only by you: chmod 600). It is never put on a command line,
# where other accounts on the computer could see it.
#
# Thoughts must be running. The script reads its port from ~/.thoughts/port ($THOUGHTS_HOME/port)
# and, before it sends the key, asks the app to prove it is the one that made the key, so a program
# that took the port after Thoughts stopped never sees it. Needs curl and openssl. The answers are
# JSON (shown neatly when jq is installed).
set -eu

die() { printf 'thoughts: %s\n' "$*" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || die "curl is needed."
command -v openssl >/dev/null 2>&1 || die "openssl is needed to check who is answering, so the key is not sent."

key=${THOUGHTS_KEY:-}
keyfile=${XDG_CONFIG_HOME:-$HOME/.config}/thoughts/key
if [ -z "$key" ] && [ -r "$keyfile" ]; then key=$(head -n 1 "$keyfile" | tr -d ' \t\r\n'); fi
[ -n "$key" ] || die "No access key. Make one in Thoughts (Settings → Privacy & cost → Keys for other programs), then set THOUGHTS_KEY or save it in $keyfile."
id=$(printf '%s' "$key" | sed -n 's/^thoughts_\([0-9a-f]\{8\}\)_[A-Za-z0-9]\{40\}$/\1/p')
[ -n "$id" ] || die "That does not look like a Thoughts access key."

home=${THOUGHTS_HOME:-$HOME/.thoughts}
port=""
[ -r "$home/port" ] && port=$(tr -dc '0-9' < "$home/port")
[ -n "$port" ] || die "Thoughts is not running."
base="http://127.0.0.1:$port/api"

# The digest alone, from OpenSSL's "SHA2-256(stdin)= …" or LibreSSL's "(stdin)= …".
digest() { sed 's/^.*= *//'; }

# Is the program on the port the app that made this key? It answers the challenge with an HMAC
# under the key's SHA-256, which only that app has. The key itself is not sent for this.
nonce=$(openssl rand -hex 16)
hello=$(curl -sS --max-time 3 "$base/hello?nonce=$nonce&key=$id" 2>/dev/null) || die "Thoughts is not running."
case "$hello" in *'"proof":'*) ;; *) die "Something other than Thoughts is answering on its port, so the key was not sent. Is Thoughts running?" ;; esac
got=$(printf '%s' "$hello" | sed -n 's/.*"keyProof":"\([0-9a-f]*\)".*/\1/p')
[ -n "$got" ] || die "Thoughts does not know this key. It may have been removed: make a new one in Settings."
secret_hash=$(printf '%s' "$key" | openssl dgst -sha256 | digest)
want=$(printf '%s' "thoughts-hello-v1:$nonce" | openssl dgst -sha256 -hmac "$secret_hash" | digest)
[ "$got" = "$want" ] || die "Thoughts could not prove that it made this key, so the key was not sent. Check that the whole key was copied, or make a new one in Settings."

# The key goes in a header read from a private file, never on the command line.
tmp=$(mktemp -d)
trap 'rm -f "$tmp/header" "$tmp/body"; rmdir "$tmp"' EXIT
( umask 077; printf 'Authorization: Bearer %s\n' "$key" > "$tmp/header" )

# A JSON string holding stdin: control characters dropped, lines joined with \n.
json_string() {
  tr -d '\000-\010\013-\037' | awk '
    BEGIN { printf "\"" }
    {
      out = ""
      for (i = 1; i <= length($0); i++) {
        c = substr($0, i, 1)
        if (c == "\\") c = "\\\\"; else if (c == "\"") c = "\\\""; else if (c == "\t") c = "\\t"
        out = out c
      }
      if (NR > 1) printf "\\n"
      printf "%s", out
    }
    END { printf "\"" }'
}

show() { if command -v jq >/dev/null 2>&1; then jq .; else cat; echo; fi; }

# request METHOD PATH [curl options…]: prints the answer, or the app's message and exits 1.
request() {
  method=$1 path=$2; shift 2
  code=$(curl -sS --max-time 30 -o "$tmp/body" -w '%{http_code}' -X "$method" -H "@$tmp/header" "$@" "$base$path") || die "Thoughts did not answer."
  case "$code" in
    2*) show < "$tmp/body" ;;
    401) die "Thoughts does not accept this key. It may have been removed: make a new one in Settings." ;;
    *) msg=$(sed -n 's/.*"error":"\([^"]*\)".*/\1/p' "$tmp/body"); die "${msg:-the request failed ($code).}" ;;
  esac
}

capture() {
  kind=$1 list=$2 text=$3
  body="{\"kind\":\"$kind\",\"text\":$(printf '%s' "$text" | json_string)"
  [ -z "$list" ] || body="$body,\"file\":$(printf '%s' "$list" | json_string)"
  printf '%s}' "$body" | request POST /capture -H 'Content-Type: application/json' --data-binary @-
}

cmd=${1:-help}
[ $# -gt 0 ] && shift
case "$cmd" in
  todo)
    list=""
    if [ "${1:-}" = "--list" ]; then [ $# -ge 2 ] || die "--list needs a task list, for example projects/x/todo.md"; list=$2; shift 2; fi
    [ $# -gt 0 ] || die "What is the todo? For example: thoughts.sh todo Call the bank tomorrow"
    capture todo "$list" "$*" ;;
  note)
    if [ $# -gt 0 ]; then text=$*; else text=$(cat); fi
    [ -n "$text" ] || die "What is the note?"
    capture note "" "$text" ;;
  todos) request GET /todos ;;
  search)
    [ $# -gt 0 ] || die "What should be searched for?"
    request GET /library/search -G --data-urlencode "q=$*" ;;
  read)
    [ $# -eq 1 ] || die "Which file? For example: thoughts.sh read ideas/shop/README.md"
    request GET /library/file -G --data-urlencode "path=$1" ;;
  check) request GET /ping ;;
  help|-h|--help) sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//' ;;
  *) die "Unknown command \"$cmd\". Try: thoughts.sh help" ;;
esac
