# meshdiet in CI

## Exit codes

| Code | When |
|---|---|
| 0 | Nothing reaches `--fail-on` (`high` by default) |
| 1 | Something does |
| 2 | A usage or config error: unknown flag, unknown key in `meshdiet.toml`, a size such as `"5 MiB"` |

Medium-confidence findings (LOAD-001, MESH-003) fail a build only with `--strict`.

## GitHub Action

```yaml
# .github/workflows/meshdiet.yml
name: meshdiet
on: [pull_request]

permissions:
  contents: read
  security-events: write   # only for the SARIF upload; drop it with `sarif: "false"`

jobs:
  models:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: uniqtechteam/meshdiet@v0
        with:
          path: public
          src: src
          fail-on: high
          version: v0.1.0   # pin the release you tested with
```

The Action downloads the meshdiet release for the runner and checks it against the release's
`SHA256SUMS`, writes the Markdown report to the job summary, uploads SARIF to code scanning, and
fails the job at `fail-on`. It needs no Rust on the runner and does not upload your models anywhere;
the summary and SARIF hold paths and numbers only. On a runner it has no build for (anything but
Linux x64/arm64, macOS Intel/Apple silicon and Windows x64), it stops with a clear error instead of
building from source.

| Input | Default | What it does |
|---|---|---|
| `path` | `paths` in meshdiet.toml, else `.` | Folders or model files, separated by spaces or new lines |
| `src` | the scanned folder | App code searched for decoder setup (LOAD-001) |
| `config` | meshdiet.toml when there is one | Config file |
| `baseline` | none | Report only what is new or grew since this baseline |
| `fail-on` | `fail_on` in meshdiet.toml, else `high` | `high`, `medium`, `low` or `never` |
| `format` | `markdown` | Format of the report file kept for later steps (`markdown`, `json`, `sarif`, `pretty`) |
| `summary` | `true` | Write the Markdown report to the job summary |
| `sarif` | `true` | Upload SARIF to code scanning (skipped with a note on pull requests from forks) |
| `args` | none | Extra `meshdiet scan` flags, for example `--strict --skip TEX-002` |
| `version` | the tag the Action is used at, else the latest release | meshdiet release to install |
| `install-gltfpack` | `false` | Install gltfpack 1.3 (pinned zip, SHA-256 checked) on PATH for `meshdiet fix`; Linux x64, macOS arm64 and Windows x64 |
| `binary` | none | A meshdiet binary already on the runner; skips the download |

Outputs: `findings`, `high`, `medium`, `low`, `files`, `total-bytes`, `uncompressed-bytes`,
`exit-code`, `report-file`, `sarif-file` and `gltfpack`. With `format: markdown`, `report-file` is
ready to post as a pull request comment.

### Running `fix` in a workflow

The downloaded `meshdiet` stays on PATH for the rest of the job, so with `install-gltfpack: "true"` a
later step can compress the models and upload them for someone to review:

```yaml
      - uses: uniqtechteam/meshdiet@v0
        with:
          path: public
          fail-on: never
          install-gltfpack: "true"
      - run: meshdiet fix public --out models-opt
      - uses: actions/upload-artifact@v4
        with:
          name: models-opt
          path: models-opt
```

The `meshdiet` command itself never downloads gltfpack; only the Action does, at a pinned version.

## A site that is already heavy

Write the baseline once on your main branch and commit it:

```bash
meshdiet scan public --write-baseline meshdiet-baseline.json
```

Then pass it in CI (`baseline: meshdiet-baseline.json`, or `--baseline` anywhere else). Only new
findings fail the job, plus any model or build that grew by more than `--tolerance` (1% by default),
reported as BASE-GROW. Regenerate the file when you have fixed something, so the bar stays where you
left it.

## Other CI systems

meshdiet is one binary with no runtime, so any CI that can run a command can use it: install it with
`cargo install --locked meshdiet` or unpack the release archive for your platform
(`meshdiet-<version>-<target>.tar.gz`, `.zip` on Windows) after checking it against `SHA256SUMS`.

The formats are made for this:

- `--format markdown` writes the summary line, the findings, the weight table and the pages, for a
  job summary or a merge request comment.
- `--format sarif` writes SARIF 2.1.0. Each result points at the model file, carries the measured
  numbers in its properties and has a stable fingerprint.
- `--format json` (schema 1) is for scripts: every asset with its size split, compression and images,
  every finding, the totals and the pages.

```console
$ meshdiet scan demo-site --format markdown --output report.md
$ head -3 report.md
## meshdiet

**4 files · 5.6 MB · 1 uncompressed (5.1 MB) · 2 high · 4 medium · 1 low · scanned in 0.00s**
```

`--output` writes the report and keeps the exit code, so the job still fails when it should.
