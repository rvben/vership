# Releases

Vership is the release control plane for `vership`. GitHub Actions is the
execution plane for cross-platform builds, publication, provenance, and
downstream packaging.

## Create a release

Start from a clean, up-to-date `main` branch and run:

```sh
vership preflight patch # use the same patch/minor/major level as the bump
vership bump patch --prepare # or minor/major
git show --stat
git show -- CHANGELOG.md
# After reviewing and approving the prepared release:
vership release
tarry cmd --timeout 20m -- vership verify --skip homebrew
```

Vership runs `make check`, updates the package version and changelog, and creates
the Conventional Commit release commit. `--prepare` never creates or pushes a
tag. After review, `vership release` tags and publishes that same version. The
tag starts the release workflow; do not bump again to finish a preparation.

Cargo and Python package versions are kept synchronized as one release unit.

`vership release` converges from the reviewed, explicitly marked commit and
performs the remaining tag and atomic push steps. If the first push attempt
fails before origin receives the tag,
rerun the same command; Vership recovers its unpublished local tag and retries
the same version.

## Homebrew handoff

The formula is managed by [the shared tap updater](https://github.com/rvben/homebrew-tap/actions/workflows/update.yml).
The upstream release workflow publishes artifacts and records this handoff in
its summary; it does not generate formulas or write to the tap. No upstream tap
deploy key is needed for this path.

The tap updater runs daily and can also be started manually after the release
is available. It pins the macOS archives and the Linux source crate, validates
their checksums and contents, and prepares a review branch with native CI on
both architectures on macOS and Linux. Linux builds from source because the
published Linux binaries require glibc 2.39.

Review and integrate that branch after CI passes, then delete the consumed
review branch so later updates can proceed. Keep repository protections enabled;
the updater does not integrate its own changes. If another update is awaiting
review, integrate it before starting another run.

After the tap integration, run the complete verification:

```sh
vership verify
```

Homebrew is explicitly configured in `vership.toml`, so it remains part of full
verification even though this repository no longer owns the formula publisher.

## Failure policy

If a release fails before any release, artifact, package, checksum, or
attestation becomes public, delete and recreate the brief tag and retry the
same version. Once anything was published, preserve the tag and issue a patch
release for release-content corrections. Registry and downstream-packaging
jobs are separate recovery domains; retry only the failed job when possible.

## Dry runs

The Release workflow can be dispatched from `main` with `dry_run` enabled. It
uses the package version unless an explicit `version` is supplied, builds the
real artifacts, and exercises registry validation without publishing packages,
creating a GitHub release, generating attestations, or updating downstream
packaging.
