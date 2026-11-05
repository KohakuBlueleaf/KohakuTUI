# Releasing

Three workflows in `.github/workflows/`, modelled on KohakuVault's:

| Workflow | Runs on | Does |
| -------- | ------- | ---- |
| `ci.yml` | push to `main`, pull requests, manual | lint (black, ruff), Rust (fmt, clippy on both feature sets, tests), Python tests on Linux, Windows and macOS × 3.10 and 3.14 |
| `nightly.yml` | 03:00 UTC daily, manual | if `main` has green CI and is not published yet, dispatches `release.yml` with `channel=nightly` |
| `release.yml` | a `v*` tag, a published GitHub release, manual | builds every wheel and the sdist; publishes nightlies and tagged releases |

`release.yml` is the only workflow that uploads, so PyPI needs one trusted publisher.

## Versions

`Cargo.toml` holds the version (`X.Y.Z`); maturin gives it to the wheel.
`scripts/release_version.py` resolves one version per build and stamps the checkout:

| Build | Cargo | Python | Tag |
| ----- | ----- | ------ | --- |
| stable | `0.1.0` | `0.1.0` | `v0.1.0` |
| nightly | `0.1.0-dev.20261105030000` | `0.1.0.dev20261105030000` | `nightly-20261105` (prerelease) |

The tracked version is never changed by a nightly; the stamp exists only in the build checkout.
A tag that does not match `Cargo.toml` fails the release before anything is built.

## What a release builds

- **Wheels** (abi3, one per platform for CPython 3.10+): Linux x86_64 and aarch64 (manylinux
  2.28), Windows x86_64, macOS arm64 and x86_64 (cross-built on the arm runner).
- Each native wheel is installed into a clean venv and checked: the version matches, the module
  imports, and the agent demo plays a whole turn on the emulated terminal, which exercises the
  compiled engine rather than just the import.
- **sdist**, checked with `twine check`.

## Cutting a stable release

1. Set the version in `Cargo.toml` (`version = "0.1.0"`), commit, push to `main`, wait for CI.
2. Tag and push: `git tag v0.1.0 && git push origin v0.1.0`.
3. `release.yml` builds, attaches the files to the GitHub release for the tag, and publishes to
   PyPI. Publishing a GitHub release for an existing `v*` tag does the same.

A manual run with `channel=build` builds and smoke-tests every artifact without publishing,
which is a good dry run before the first tag.

## PyPI trusted publishing

No API token is stored anywhere: PyPI trusts GitHub's OIDC identity of this one workflow.

**On PyPI.** The project does not exist yet, so add a *pending* publisher (it turns into a
normal one on the first upload): <https://pypi.org/manage/account/publishing/>, section
"Add a new pending publisher", tab GitHub:

| Field | Value |
| ----- | ----- |
| PyPI Project Name | `kohakutui` |
| Owner | `KohakuBlueleaf` |
| Repository name | `KohakuTUI` |
| Workflow name | `release.yml` |
| Environment name | `pypi` |

Once the project exists, the same values go under the project's *Settings → Publishing*.

**On GitHub.** *Settings → Environments → New environment* named `pypi`:

- *Deployment branches and tags*: "Selected branches and tags", with the branch `main` (nightly
  runs are dispatched on `main`) and the tag pattern `v*`.
- Required reviewers are optional. They pause every upload, nightlies included, until someone
  approves.

`publish-to-pypi` is the only job with `id-token: write`, and it runs in that environment.

## Nightlies

`nightly.yml` checks the head of `main`: CI must have passed on that commit, and a commit already
published as a nightly is skipped (`force_rebuild` overrides that). It then dispatches
`release.yml`, which publishes `X.Y.Z.devTIMESTAMP` to PyPI (pip installs it only with `--pre`)
and as a dated GitHub prerelease with SHA-256 checksums. Each nightly's notes name its source
commit.
