# Agent notes for broadcast

Operating knowledge for an AI agent working in this repo — release process,
publishing rules, and the manual steps a human has to do that an agent
can't skip past. For what the project *is*, see README.md; this file is
about how to work on it safely.

## Repo topology

- **This repo (`londospark/broadcast`)** is the single source of truth.
  Everything else below is generated or mirrored from it — never hand-edit
  the outputs directly; edit the source here and let the sync/release
  process regenerate them.
- **`londospark/broadcast-omarchy-plugin`** — a separate GitHub repo, auto-
  mirrored from this repo's `omarchy-plugin/io.github.londospark.broadcast/`
  folder. Exists because Omarchy's `omarchy plugin add <url>` clones a
  repo's root and requires `manifest.json` there — it has no support for
  installing from a subdirectory of a larger repo, which is what this repo
  is. See "Omarchy plugin sync" below.
- **AUR packages** (`broadcast-ctl-bin`, `broadcast-ctl-git`,
  `broadcast-gui-bin`, `broadcast-gui-git`) — each its own AUR git repo,
  not part of this repo at all. No `broadcast-core` package exists or
  should exist: it's an internal library crate with no binary, nothing
  would ever install it directly.
- **`omacom/omarchy-plugin-marketplace`** — third-party community repo.
  Issue [#3854](https://github.com/omacom/omarchy-plugin-marketplace/issues/3854)
  is this project's marketplace listing request; post a status comment
  there when a change affects the plugin repo's install instructions or
  addresses a review finding.
- **[TinyToolTown](https://github.com/shanselman/TinyToolTown)** — a
  third-party tools directory; this project is listed at
  `src/content/tools/broadcast.md` there. Its thumbnail is auto-scraped
  weekly from the first real image in *this* repo's README.md (see
  `scripts/fetch-thumbnails.mjs` in that repo) — not something to upload
  directly to their `public/thumbnails/`, it'll just get overwritten.
  Frontmatter fields `ai_summary`/`ai_features` are automation-generated;
  don't hand-edit those in a PR. Updates to the listing content go via a
  PR from a fork (`gh repo fork shanselman/TinyToolTown --clone`),
  editing that one file, after running their required
  `npm ci && npm test && npm run build` (needs Node 24 — `mise use -g node@24`).

## Release process

1. Bump `[workspace.package] version` in `Cargo.toml`, then run
   `cargo check` to refresh `Cargo.lock`'s workspace-member versions.
   Commit both together (`chore: bump version to X.Y.Z`).
2. Tag **lightweight**, not annotated: `git tag vX.Y.Z`. Annotated tags
   have historically failed to trigger `release.yml`'s `on: push: tags:`
   — root cause never fully proven, but every real release so far
   (v0.1.0–v0.5.1) has used a lightweight tag and worked.
3. `git push origin vX.Y.Z` triggers `.github/workflows/release.yml`:
   `test` (Arch, no Maxine) → `build` (Ubuntu 24.04, compiles the whole
   workspace **once** — see "Why one build job" below) → `build-deb` /
   `build-rpm` (packaging only, no compiler, no dev headers — just wrap
   `build`'s binaries) → `release` (needs all three, publishes via
   `softprops/action-gh-release`).
4. **If a run fails**, fix the issue, commit, push to `main`, then
   re-trigger by deleting and recreating the same tag:
   ```sh
   gh release delete vX.Y.Z --repo londospark/broadcast --yes   # only if a draft/partial release exists
   git push origin --delete vX.Y.Z
   git tag -d vX.Y.Z
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```
   Check `gh release view vX.Y.Z --json isDraft` first — a run that fails
   partway through the `release` job can still leave a **draft** release
   with some assets uploaded; safe to delete if it's a draft (nothing
   public was ever exposed).
5. **Don't re-tag for non-urgent fixes.** If a fix lands on `main` after
   a tag was already cut and the release published successfully, let it
   ride until the next version bump rather than cutting a new tag just
   for cleanup/optimization. Only re-tag immediately for something that
   actually broke the release itself.

### Why one build job, not three

`build-arch`/`build-deb`/`build-rpm` each used to run a full
`cargo build --release` in their own container. Consolidated to a single
`build` job on `ubuntu-24.04` because: shared-library ABI compatibility
only runs forward (a binary built against an older glibc/GTK4 runs fine
on a newer one, never the reverse), Ubuntu 24.04 has the oldest glibc of
the three targets, and `build-deb`/`build-rpm`'s packaging tools
(`cargo-deb --no-build`, `cargo-generate-rpm`) never auto-detect runtime
library versions here anyway — `depends`/`requires` are hardcoded in each
crate's `Cargo.toml`. Trade-off: Fedora and Arch no longer get their own
compile-time validation as part of the release; only the `test` job
(Arch) still compiles from source outside Ubuntu.

### GitHub Actions gotchas hit in this repo

- `secrets` is **not** a valid context inside a step's `if:` — only
  `env:`/`run:`. Mirror it into a job-level `env:` var and gate on that
  instead (`env.NGC_API_KEY != ''`, not `secrets.NGC_API_KEY != ''`).
  This silently broke `release.yml` for months before it was caught.
- GitHub rejects any single release asset over 2GB. The Maxine runtime
  bundle needed splitting into `-libs.tar.gz` and `-models.tar.gz` for
  exactly this reason — check `du -h` on anything that bundles binary
  blobs before assuming it'll fit.
- `actions/cache` only **saves** an updated cache on a miss, never on a
  hit. A fixed cache key covering content that later grows (e.g. going
  from 1 GPU architecture to 4) will keep serving the old, incomplete
  cache forever unless the key itself changes — the key must encode
  everything that gets written to that path.
- Always run `actionlint` (`mise use -g aqua:rhysd/actionlint@latest`) on
  any workflow change before pushing. **Also install `shellcheck`**
  (`mise use -g shellcheck@latest`) — actionlint shells out to it for
  embedded `run:` script linting and silently skips those checks
  entirely if it's not found on PATH. Both need to be present to get
  full coverage; one clean run with only actionlint installed is not a
  reliable signal.
- Keep every action on its latest major and off Node 20 — check
  `gh run view --log` for deprecation warnings, or an action's
  `action.yml` `runs.using` field directly (`node20` vs `node24` vs
  `composite`).

## Manual steps a human has to do — don't try to script around these

An agent cannot complete any of the following; the right move is to ask
the user to do it (often via the `! <command>` prefix for anything
interactive) and wait, not find a workaround:

- **Creating a new AUR account**, or a new GitHub account — needs the
  human's own email/browser signup.
- **Adding an SSH public key to an AUR account** — no API for this, only
  the web UI at `aur.archlinux.org/account/<user>/edit`.
- **`gh auth login`/`gh auth refresh`**, **`bw login`/`bw unlock`** — any
  interactive auth flow. Prefer non-interactive variants when they exist
  (e.g. `bw login --apikey` with `BW_CLIENTID`/`BW_CLIENTSECRET` instead
  of the interactive email/password/2FA prompt, which doesn't work well
  through a one-shot command bridge).
- **Accepting NVIDIA's Maxine SDK license** — has to happen once via the
  NGC resource page while logged in, before `NGC_API_KEY` will actually
  work for downloads.

## Credential hygiene

- Never echo a secret value (API keys, tokens, private key contents) in
  chat output — pass via env vars in tool calls, or `gh secret set` /
  `bw create item` piped directly, never printed first.
- When generating a new credential (SSH keypair, etc.), ask whether it
  should be backed up (this project's convention: Bitwarden, via the
  `bw` CLI once unlocked) **before** deleting any local copy.
- Once a private key has been uploaded to its destination (a GitHub
  secret, etc.), delete the local copy — don't leave working copies of
  key material lying around after they're no longer needed. But do this
  only *after* any requested backup is done; a secret store like GitHub
  Actions secrets is write-only, so a private key deleted before backup
  is gone for good, recoverable only by rotating to a new key.
- The AUR SSH key is **account-wide** — it can push to every package the
  account maintains, not just this project's four. Treat it accordingly;
  it's not scoped the way a GitHub deploy key is.
- A GitHub deploy key, by contrast, *is* scoped to one repo — prefer
  generating a dedicated one over reusing a broader personal token when
  a workflow needs push access to just one other repo (see
  `PLUGIN_SYNC_DEPLOY_KEY`, used only by the plugin sync workflow).

## Omarchy plugin sync

- Source of truth: `omarchy-plugin/io.github.londospark.broadcast/` in
  this repo (`manifest.json`, `*.qml`, `*.js`).
- `.github/workflows/sync-omarchy-plugin.yml` runs
  `scripts/sync-omarchy-plugin-repo.sh` on every push to `main` touching
  `omarchy-plugin/**`, `LICENSE`, or the sync script itself. It mirrors
  those files into `broadcast-omarchy-plugin` over SSH using
  `PLUGIN_SYNC_DEPLOY_KEY`, and re-pins the Maxine install snippet in
  that repo's README to this repo's current commit SHA via `sed`.
- The plugin repo's **README.md and LICENSE are not touched by the sync
  script's file copy** (`rsync --exclude`) — LICENSE is instead copied
  fresh from this repo's own `LICENSE` each run, but README.md is
  maintained by hand, directly in the plugin repo. To edit it: clone
  `broadcast-omarchy-plugin` separately, edit, commit, push there — not
  through this repo.
- The `sed`-based SHA re-pin only **substitutes** an already-present
  `git -C /tmp/broadcast checkout --detach <40-hex-chars>` line; it
  cannot add that line if it isn't already there. If the README's Maxine
  section is restructured, the first pinned-commit line has to be added
  by hand, once, directly in the plugin repo — after that, automated
  syncs keep it current.

## AUR package maintenance

Four packages, all pushed via `git push` to
`ssh://aur@aur.archlinux.org/<pkgname>.git` (needs an SSH key registered
on the maintaining AUR account first — see "Manual steps" above).
Regenerate `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO` before
every push; a push missing `.SRCINFO` in the same commit is rejected.

- **`broadcast-ctl-git` / `broadcast-gui-git`** — build from latest
  `main` via a `pkgver()` function (`git describe --long --tags`, the
  standard ArchWiki VCS-package pattern). No maintenance needed after
  the initial PKGBUILD; version resolves itself on every build.
- **`broadcast-ctl-bin` / `broadcast-gui-bin`** — pull the compiled
  binary straight from a tagged GitHub release. **These need a manual
  bump after every release**: `pkgver` to match the new tag, real
  `sha256sums` recomputed from the new release assets (never leave
  `SKIP` for a non-VCS source — compute the real hash), `pkgrel` reset
  to 1. There's no automation for this yet; check whether it's been
  done as part of any release-process checklist.
- Before pushing any PKGBUILD change, test-build it for real:
  `makepkg -s --noconfirm --nodeps` (the `--nodeps` is specific to dev
  machines where Rust comes from `mise`/`rustup` rather than pacman's
  `rust` package, so `makepkg`'s dependency check doesn't see it as
  satisfied — don't skip dependency checks like this when actually
  publishing user-facing guidance about what to install).

## Maxine SDK / GPU runtime bundling

- `scripts/install-maxine-sdk.sh` — single-architecture download + build,
  used by CI's `test`/`build` jobs and by developers with their own NGC
  key. Detects the local GPU's compute capability via `nvidia-smi`, or
  defaults to `rtx_pro_6000` when none is found (e.g. a GPU-less CI
  runner) so the build still gets real headers/libs to link against.
- `scripts/build-maxine-runtime-bundle.sh` — sources
  `install-maxine-sdk.sh`'s functions (guarded so `main` doesn't
  auto-run when sourced) and loops the download across all four desktop
  RTX architectures (`t4` Turing, `a10` Ampere, `l40` Ada, `rtx_pro_6000`
  Blackwell — datacenter-only architectures like `a100`/`h100` are
  intentionally excluded). Packages the result into two tarballs, split
  because a single combined one exceeds GitHub's 2GB asset limit:
  `-libs.tar.gz` (NVIDIA's CUDA/TensorRT runtime, architecture-
  independent, the large one) and `-models.tar.gz` (the per-arch
  `.trtpkg` files, much smaller). cuDNN is deliberately excluded from
  `-libs` — verified via `readelf -d` that nothing in this project's own
  libraries or their direct dependencies references it, and confirmed
  live (moved it aside, restarted PipeWire, the denoiser model loaded
  identically with or without it).
- `scripts/install-maxine-runtime.sh` — the end-user, no-NGC-key
  installer. Downloads both tarballs plus the compiled LADSPA plugin
  from the latest GitHub release via `gh release download` and installs
  them to the same paths `install-maxine-sdk.sh` would, so everything
  downstream (env detection, model lookup) works unchanged regardless
  of which installer a user ran.
- **Redistribution basis**: verified against the actual text of
  [NVIDIA's Maxine SDK license](https://developer.nvidia.com/downloads/maxine-sdk-license)
  (not a summary) — its distribution supplement permits redistributing
  any SDK portion other than its audio/video data samples, bundled into
  our own application. Don't assume this holds for a different NVIDIA
  SDK without checking that SDK's own license text.
- **GPU architecture auto-selection at runtime**: `broadcast-ctl`'s
  generated PipeWire env script (`generate_maxine_env.sh`, written by
  `install-config`) sets `NVAFX_SM` from `nvidia-smi --query-gpu=compute_cap`
  automatically, so the LADSPA plugin's existing `sm_<N>/` model
  directory lookup picks the architecture actually present rather than
  whichever one a directory scan happens to find first when several are
  bundled together. A manually-set `NVAFX_SM` in the environment always
  wins over the auto-detected value.
- **Known issue, not a bug to "fix" reflexively**: `dereverb_denoiser`
  fails to load with `NvAFX_CreateEffect` status 6
  (`EFFECT_NOT_AVAILABLE`) on the reference dev machine's Blackwell GPU.
  Suspected Blackwell-specific SDK gap, unconfirmed on other
  architectures. Left in the candidate chain rather than removed: the
  existing fallback in `maxine_ladspa.c` already degrades gracefully to
  the working `denoiser` model when it fails, so keeping it costs a
  modest amount of bundle size for zero functional risk, and removing
  it would presumptively break the feature for any hardware where it
  actually does work.

## General conventions

- Confirm with the user before: creating a new public repo, filing an
  issue/PR/comment on a third-party repo, deleting a GitHub
  release/tag, rotating an existing credential. All of these are visible
  or hard-to-reverse actions.
- When a change to this repo also requires a corresponding change to
  `broadcast-omarchy-plugin`'s README, the marketplace listing (issue
  #3854), or the TinyToolTown listing, make that follow-up explicitly
  rather than assuming automation covers it — the plugin sync only
  mirrors plugin *files*, never that repo's README, and nothing
  auto-updates the TinyToolTown listing's text content at all (only its
  thumbnail refreshes on its own, from this repo's README image).
- Keep this file and any screenshot(s) in `screenshots/` current when
  the feature set changes materially — both the plugin repo's README
  and the TinyToolTown listing draw on what's documented here.
