# Releasing Hum

Releases are built, signed and published by
[`.github/workflows/release.yml`](../.github/workflows/release.yml).

## Quick release: the Actions tab

1. On GitHub, open **Actions → Release → Run workflow**.
2. Type a version such as `0.2.0`, or leave the field empty to bump the
   patch number of the latest release (`v0.1.0` becomes `v0.1.1`).
3. Click **Run workflow**. About five minutes later the release page has
   `hum-vX.Y.Z-arm64-v8a.apk` and its `.sha256`. The workflow creates the
   tag itself, on the commit it built.

Tick **Dry run** to build and sign without publishing anything. The APK is
then only kept as a workflow artifact.

### Temporary key or your own key

- **No signing secrets set (the current setup):** each release is signed with
  a temporary key made for that run only, and its release notes say so.
  Android will not install it over an earlier Hum build. **Uninstall Hum
  first**, which removes its saved settings and beats, then install the new
  APK.
- **Signing secrets set (steps 1 and 2 below):** every release is signed with
  your keystore and installs over the previous one, keeping your settings.
  The first keystore-signed build still needs one uninstall, because the
  installed app was signed with a temporary key.

You can also release by pushing a tag yourself (step 3). That route always
requires the signing secrets.

---

## 1. Optional, one-time: create a signing keystore (on your computer)

> **⚠️ Back up the keystore and its passwords.**
> **Android only installs an update if it is signed with the same key as the
> installed app. If you lose this keystore, future versions of Hum cannot be
> installed over the old one: you would have to uninstall Hum first (losing
> its saved settings and beats), then install the new build.**
> Keep a copy somewhere safe and offline, such as a password manager or an
> encrypted USB stick. **Never commit it.** `.gitignore` already excludes
> `*.jks` and `*.keystore`.

Generate the keystore. You need a JDK for `keytool`, and Android Studio
includes one. Pick your own passwords when it asks:

```sh
keytool -genkeypair -v \
  -keystore hum-release.jks \
  -storetype PKCS12 \
  -alias hum \
  -keyalg RSA -keysize 4096 \
  -validity 10000 \
  -dname "CN=Hum"
```

With a PKCS12 keystore the key password is the same as the keystore
password. Use that same value for both `ANDROID_KEYSTORE_PASSWORD` and
`ANDROID_KEY_PASSWORD` below.

Base64-encode it so it can be stored as a GitHub secret:

```sh
# Linux
base64 -w0 hum-release.jks > hum-release.jks.b64
# macOS
base64 -i hum-release.jks -o hum-release.jks.b64
```

## 2. Optional, one-time: add the four repository secrets

Using the [GitHub CLI](https://cli.github.com/), run these from inside your
clone of the repo:

```sh
gh secret set ANDROID_KEYSTORE_BASE64 < hum-release.jks.b64
gh secret set ANDROID_KEYSTORE_PASSWORD   # paste the keystore password when prompted
gh secret set ANDROID_KEY_ALIAS --body "hum"
gh secret set ANDROID_KEY_PASSWORD        # paste the same password again
rm hum-release.jks.b64                    # the .b64 copy is no longer needed
```

You can also add them on the web: **Settings → Secrets and variables →
Actions → New repository secret**.

If the secrets are set, every release uses them. If any is missing, a manual
run falls back to a temporary key, and a tag push fails at its first step,
naming the missing secret. The workflow never prints secret values. It decodes the
keystore into the runner's temp directory and deletes it at the end of the
job, even when the job fails.

## 3. Cut a release by pushing a tag (needs the secrets)

```sh
git checkout main && git pull
git tag v0.1.0 && git push origin v0.1.0
```

The workflow then:

1. checks that the tag looks like `vMAJOR.MINOR.PATCH`,
2. sets `versionName` to the tag without the `v` (`0.1.0`) and `versionCode`
   to `major*10000 + minor*100 + patch` (`v0.1.0` gives `100`, `v1.2.3`
   gives `10203`; minor and patch must stay at 99 or below),
3. builds the arm64-v8a release APK with `--locked`, then runs `zipalign`,
   `apksigner sign` and `apksigner verify`,
4. checks that the APK declares **zero permissions** (`aapt2 dump
   permissions`) and contains only `lib/arm64-v8a/libhum_app.so`,
5. creates a GitHub Release with generated notes and attaches:
   - `hum-v0.1.0-arm64-v8a.apk`
   - `hum-v0.1.0-arm64-v8a.apk.sha256`

Every new tag must be higher than the last one, because Android refuses
to install a lower `versionCode` over a higher one.

If a release run fails after the tag is pushed, fix the problem, delete the
tag and push it again:

```sh
git tag -d v0.1.0 && git push origin :refs/tags/v0.1.0
git tag v0.1.0 && git push origin v0.1.0
```

## 4. Install on the phone (Pixel, GrapheneOS)

Download the `.apk` (and optionally the `.sha256`) from the release page.

Check the download if you like:

```sh
sha256sum -c hum-v0.1.0-arm64-v8a.apk.sha256
```

**With adb** (enable *Developer options → USB debugging* on the phone):

```sh
adb install -r hum-v0.1.0-arm64-v8a.apk
```

`-r` replaces an existing install and keeps its data. This only works when
the new APK is signed with the same keystore as the installed one.

**Without a computer:** copy the APK to the phone, or download it there,
and open it in the Files app. GrapheneOS asks you to allow the app that
opens the APK (Files or your browser) to install unknown apps; allow it for
that app only. Updates install the same way, over the existing app.

Hum needs no Google Play Services and asks for no permissions, so nothing
needs approving on first launch.

### Debug builds

Every CI run on `main` and on pull requests uploads an unsigned-by-you
**debug** APK as the `hum-debug-arm64-v8a` artifact (on the run's page under
*Artifacts*). Debug builds are signed with a throwaway debug key, so they
cannot install over a release build, or the other way round. Uninstall
one before installing the other.
