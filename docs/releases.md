# Releases

Vership is the release control plane for `vership`. GitHub Actions is the
execution plane for cross-platform builds, publication, provenance, and
downstream packaging.

## Create a release

Start from a clean, up-to-date `main` branch and run:

```sh
vership preflight patch # use the same patch/minor/major level as the bump
vership bump patch # or minor/major
tarry cmd --timeout 20m -- vership verify
```

Vership runs `make check`, updates the package version and changelog, creates
the Conventional Commit release commit and tag, and pushes both. The tag starts
the release workflow. Use `vership release` only when the on-disk version was
intentionally set in advance.

Cargo and Python package versions are kept synchronized as one release unit.

For a deliberate review checkpoint before the release becomes tag-visible,
prepare the release commit first:

```sh
vership bump patch --prepare
git show --stat
git show -- CHANGELOG.md
vership release
```

`--prepare` never creates or pushes a tag. `vership release` converges from the
reviewed, explicitly marked commit and performs the remaining tag and atomic
push steps. If the first push attempt fails before origin receives the tag,
rerun the same command; Vership recovers its unpublished local tag and retries
the same version.

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
