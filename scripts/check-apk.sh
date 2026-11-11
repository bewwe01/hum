#!/usr/bin/env bash
# Verifies the privacy promises of an APK: zero permissions, arm64 only.
# Usage: scripts/check-apk.sh path/to/app.apk
set -euo pipefail

apk="${1:?usage: check-apk.sh <apk>}"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
bt="${ANDROID_BUILD_TOOLS:-}"
if [ -n "$bt" ] && [ -x "$sdk/build-tools/$bt/aapt2" ]; then
  aapt2="$sdk/build-tools/$bt/aapt2"
else
  aapt2="$(ls "$sdk"/build-tools/*/aapt2 | sort -V | tail -n 1)"
fi

echo "== aapt2 dump permissions $apk"
perms="$("$aapt2" dump permissions "$apk")"
echo "$perms"
if echo "$perms" | grep -q "permission"; then
  echo "ERROR: the APK declares permissions; Hum must request none." >&2
  exit 1
fi
echo "OK: no permissions declared."

echo "== aapt2 dump badging (summary)"
"$aapt2" dump badging "$apk" | grep -E "^(package|sdkVersion|targetSdkVersion|native-code|uses-feature|application-label):" || true

echo "== native libraries"
libs="$(unzip -Z1 "$apk" | grep '^lib/' || true)"
echo "$libs"
if [ "$libs" != "lib/arm64-v8a/libhum_app.so" ]; then
  echo "ERROR: expected exactly lib/arm64-v8a/libhum_app.so" >&2
  exit 1
fi
ls -l "$apk"
