#!/usr/bin/env bash
# Installs Linux system packages on a CI runner without letting a slow or unreachable package
# mirror hold the job hostage. Used by .github/workflows/ci.yml and release.yml.
#
#   bash scripts/ci/apt-install.sh <package>...
#
# Plain `apt-get update && apt-get install` waits a long time on every stalled connection: one
# run of the 0.0.8 release sat in this step for 87 minutes before failing, another took four
# minutes for what normally takes 40 seconds. Here:
#   - a stalled connection is given up after 15 seconds, and apt moves on to the next mirror;
#   - everything that needs the network (package lists, downloads) runs under a hard time limit
#     and is tried up to three times;
#   - unpacking the downloaded packages needs no network and is never interrupted half-way.
# The worst case is a clear failure after about nine minutes (the calling step also has a
# timeout). That failure is the runner's network or the Ubuntu mirror: re-run the job.
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "usage: apt-install.sh <package>..." >&2
  exit 2
fi

apt=(apt-get -q
  -o Acquire::Retries=3
  -o Acquire::http::Timeout=15
  -o Acquire::https::Timeout=15
  -o DPkg::Lock::Timeout=60
  -o Dpkg::Use-Pty=0)

download() {
  sudo timeout --kill-after=10 45 "${apt[@]}" update &&
    sudo DEBIAN_FRONTEND=noninteractive timeout --kill-after=10 120 "${apt[@]}" install -y --download-only "$@"
}

for try in 1 2 3; do
  if download "$@"; then
    sudo DEBIAN_FRONTEND=noninteractive "${apt[@]}" install -y "$@"
    exit 0
  fi
  echo "::warning::Linux packages: attempt $try of 3 failed (slow or unreachable package mirror)"
  if [ "$try" -lt 3 ]; then sleep $((try * 10)); fi
done

echo "::error::The Linux packages could not be downloaded after 3 attempts. This is the runner's network or the Ubuntu mirror, not the code: re-run this job."
exit 1
