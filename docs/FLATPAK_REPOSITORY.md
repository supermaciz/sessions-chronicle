# Self-Hosted Flatpak Repository

Sessions Chronicle can publish its own signed Flatpak repository under the website's GitHub Pages deployment. This gives users a real Flatpak remote, so installs can be updated with `flatpak update` instead of manually downloading each `.flatpak` bundle from GitHub releases.

The workflow is `.github/workflows/flatpak-repository.yml`. It runs when a non-prerelease GitHub release is published and can also be started manually with `workflow_dispatch`.

## What Gets Published

The public install URL is `https://sessions-chronicle.maciz.dev/flatpak/`.

The Pages site contains:

- the Astro landing site at `https://sessions-chronicle.maciz.dev/`
- the OSTree Flatpak repository under `https://sessions-chronicle.maciz.dev/flatpak/`
- `sessions-chronicle.flatpakrepo` for adding the remote
- `dev.maciz.sessionschronicle.flatpakref` for one-command install
- a small `flatpak/index.html` with the install command

The repository currently builds the stable manifest for `x86_64` only:

```bash
build-aux/dev.maciz.sessionschronicle.json
```

The Flatpak branch is `master` to match the branch used by the existing release bundles.
The generated OSTree payload is stored in the Git branch `flatpak-repo-data`, then copied into `website/public/flatpak/` during website builds so Pages always serves one combined site.

Add an `aarch64` job later if there is a native or emulated ARM runner available.

## Required GitHub Setup

Enable GitHub Pages for the repository:

1. Open **Settings > Pages**.
2. Set **Source** to **GitHub Actions**.

Create a dedicated GPG signing key for this Flatpak repository. Do not reuse a personal or package-signing key.

```bash
gpg --quick-generate-key "Sessions Chronicle Flatpak <maciz@outlook.fr>" ed25519 sign never
gpg --list-secret-keys --keyid-format=long
```

Use the long key ID from the `sec` line, then export the private key:

```bash
gpg --armor --export-secret-keys <key-id> > sessions-chronicle-flatpak-private.asc
```

Add these repository secrets under **Settings > Secrets and variables > Actions**:

| Secret | Value |
|--------|-------|
| `FLATPAK_GPG_KEY_ID` | Long GPG key ID used to sign the repository |
| `FLATPAK_GPG_PRIVATE_KEY` | Full contents of `sessions-chronicle-flatpak-private.asc` |

The workflow expects a CI signing key that can sign non-interactively after import. For this repository, that means using a dedicated Flatpak signing key with no passphrase. If a passphrase is required later, add explicit GPG agent presetting to the workflow before the Flatpak build step.

## Publishing

Publishing a non-prerelease GitHub release triggers:

1. import the Flatpak GPG signing key
2. build `dev.maciz.sessionschronicle` from the stable manifest
3. export the app into `public/` as branch `master`
4. sign the repository summary and generate static deltas
5. publish the generated repository payload to the `flatpak-repo-data` branch
6. build the Astro website with that payload mounted under `website/public/flatpak/`
7. publish the combined artifact through GitHub Pages

The website deployment workflow also fetches `flatpak-repo-data` before each Pages deploy. That preserves `/flatpak/` when website-only changes are pushed to `main`.

The existing `release.yml` workflow still uploads a standalone `.flatpak` bundle to the release. Keep it for users who prefer manual downloads or as a fallback if the Flatpak remote is unavailable.

Pre-releases intentionally do not publish this repository. Keep them as standalone release bundles or add a separate Flatpak branch such as `beta` if pre-release updates should become installable later.

## User Install Commands

One-command install:

```bash
flatpak install --user https://sessions-chronicle.maciz.dev/flatpak/dev.maciz.sessionschronicle.flatpakref
```

This App ID replaces the earlier `io.github.supermaciz.sessionschronicle` self-hosted build. Existing installs do not migrate automatically; reinstall the app under the new ID.

The GSettings schema path also changed (from `/io/github/supermaciz/sessionschronicle/` to `/dev/maciz/sessionschronicle/`), so user preferences (window size, maximized state) stored under the old path are not carried over.

Manual remote setup:

```bash
flatpak remote-add --user --if-not-exists sessions-chronicle \
  https://sessions-chronicle.maciz.dev/flatpak/sessions-chronicle.flatpakrepo
flatpak install --user sessions-chronicle dev.maciz.sessionschronicle//master
```

Updates then arrive through:

```bash
flatpak update
```

## Verification

After the first successful deployment:

```bash
curl -I https://sessions-chronicle.maciz.dev/flatpak/summary
flatpak remote-add --user --if-not-exists sessions-chronicle \
  https://sessions-chronicle.maciz.dev/flatpak/sessions-chronicle.flatpakrepo
flatpak remote-ls sessions-chronicle
flatpak install --user sessions-chronicle dev.maciz.sessionschronicle//master
flatpak run dev.maciz.sessionschronicle
```

If install metadata fails to load, check that `sessions-chronicle.flatpakrepo` and `dev.maciz.sessionschronicle.flatpakref` include the generated `GPGKey=` value and that the Pages URL matches `FLATPAK_REPO_URL` in the workflow.

## References

- Flatpak hosting guide: https://docs.flatpak.org/en/latest/hosting-a-repository.html
- Flatpak builder action: https://github.com/flatpak/flatpak-github-actions
- GitHub Pages deploy action: https://github.com/actions/deploy-pages
