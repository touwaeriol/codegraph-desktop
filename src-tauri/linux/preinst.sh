#!/bin/sh
# Package pre-install hook. Stop only the exact standard-install application.
# The application handles SIGTERM by stopping managed CodeGraph trees first.
set -eu
case "$(uname -s)" in
  Linux) application=/usr/bin/codegraph-desktop ;;
  Darwin) application='/Applications/CodeGraph Desktop.app/Contents/MacOS/codegraph-desktop' ;;
  *) echo 'Unsupported package upgrade platform.' >&2; exit 1 ;;
esac
pids=""
if [ "$(uname -s)" = Linux ]; then
  for executable in /proc/[0-9]*/exe; do
    resolved=$(readlink "$executable" 2>/dev/null || true)
    if [ "$resolved" = "$application" ]; then
      pid=${executable#/proc/}; pid=${pid%/exe}; pids="$pids $pid"
    fi
  done
else
  pids=$(ps -axo pid=,comm= | while read -r pid executable; do
    if [ "$executable" = "$application" ]; then printf '%s\n' "$pid"; fi
  done)
fi
[ -n "$pids" ] || exit 0
matches_application() {
  if [ "$(uname -s)" = Linux ]; then
    current=$(readlink "/proc/$1/exe" 2>/dev/null || true)
  else
    current=$(ps -p "$1" -o comm= 2>/dev/null || true)
  fi
  [ "$current" = "$application" ]
}
for pid in $pids; do
  if matches_application "$pid"; then
    kill -TERM "$pid" 2>/dev/null || {
      if matches_application "$pid"; then echo 'Cannot request CodeGraph Desktop shutdown.' >&2; exit 1; fi
    }
  fi
done
attempt=0
while [ "$attempt" -lt 40 ]; do
  remaining=0
  for pid in $pids; do
    if matches_application "$pid"; then remaining=1; fi
  done
  [ "$remaining" -ne 0 ] || exit 0
  sleep 1
  attempt=$((attempt + 1))
done
echo 'CodeGraph Desktop did not finish stopping its projects. Exit the application and retry; installed files were not replaced.' >&2
exit 1
