# Review of REPORT_ATHENA.MD

Reviewed on 5 October 2026 against `main` at
`667b6ed98689c230fab2981098e036feb873a478` and the open
[PR #3](https://github.com/FrancescoDiBella/athena-broker/pull/3) at
`a3be8f2d588ae43daafe256f17c866a933cff0f9` (`feat/roadmap-completion`).

The attached report documents a real HTTP/PostGIS experiment on 1 October.
Its external `probe.py`, Compose files and raw response artifacts were not attached.
This review compares source and existing acceptance coverage, and adds a
repository regression; it is not a rerun of the original external script.

## Findings

| Report case | Main | PR #3 before this change | Action |
| --- | --- | --- | --- |
| Temporal filtering before pagination | Already uses matching samples before LIMIT/OFFSET | Preserved through `selection_sql` and `matched` | Add the 2,001-entity regression |
| Historical within/intersects/near ignored | Present | Fixed: parsed geo filter is compiled against instances inside the temporal interval before pagination | Reuse #2/#3; add all three report relations |
| Existing ID with no matching samples returns id/type only | Present | Fixed: collection IDs now use the same historical selection as type queries, including EXISTS | Add negative controls for both observedAt and createdAt |
| Collection query without temporal interval accepted | Present | Still present: builder defaults to after/Unix epoch | Require an explicit temporal query in the collection handler |
| Current-entity geo controls | Report passed | Spatial implementation expanded in #3 | No duplicate issue from this report |

The mandatory temporal query is specified in ETSI GS CIM 009 V1.9.1,
[clause 5.7.4.4, printed page 208](https://www.etsi.org/deliver/etsi_gs/CIM/001_099/009/01.09.01_60/gs_CIM009v010901p.pdf#page=208).
`timeproperty` remains optional. Resource-ID retrieval is a distinct operation;
this collection fix preserves its existing optional-query behavior.

## Change and regression coverage

Collection requests without `timerel`, or without its required `timeAt`, return
400 `BadRequestData`; `between` also requires `endTimeAt`. This includes requests
with only geo parameters or `lastN`. API documentation describes the changed contract.

Unit tests check missing/incomplete temporal queries, valid intervals, the default
`observedAt` property and the separate resource-ID builder. Existing invalid-regex,
oversized-offset and unsupported-federation regressions now supply a valid interval
so that they continue testing their intended condition.

The PostGIS/router regression imports 2,001 historical entities through the storage
API, with deterministic `observedAt`/`createdAt` timestamps. The target is inserted
first and has the last lexical ID. It verifies all three unfiltered pages, then
checks filtered first-page results, positive/negative IDs, counts and all three
missing-interval geographic cases. It removes its own historical fixture on success.
The fixture tests historical entities directly; it does not repeat current-entity
creation, federation, load/performance measurements or all 27 original assertions.

## GitHub state at review

- Issue #1 remains the broad roadmap tracker.
- Issue #2 tracks the increment implemented by #3.
- PR #3 is open, unmerged and mergeable. Its two Rust checks and GitGuardian check
  passed on `a3be8f2`. No review comments or submitted reviews were present.
- Those existing green checks predate this validation fix and its new tests.

No merge, deployment or schema migration is included in this change.

## Local validation

Rust 1.98.1 was installed in the task workspace. `cargo test --locked --workspace
-j 2` passed: 36 tests passed and 8 database/service/benchmark tests were ignored
as declared by the repository. The new PostGIS regression compiled but has not
been executed: this environment has no PostgreSQL/PostGIS or Docker service.

Formatting passed with Rust 1.98.1 `rustfmt --edition 2021 --check` over all 18
workspace target roots obtained from `cargo metadata --no-deps`. `git diff --check`
also passed. The Cargo formatting launcher cannot run here because `/proc/self/exe`
is unavailable; invoking rustfmt directly checks the same workspace source roots.

The compiler used an explicit toolchain sysroot and the system linker
(`RUSTFLAGS="--sysroot <toolchain-prefix> -C linker-features=-lld"`) because the
bundled linker launcher also depends on `/proc/self/exe`. No project configuration
or lockfile was changed for these environment adaptations.

Clippy completed successfully using `RUSTC_WORKSPACE_WRAPPER=<toolchain-prefix>/bin/
clippy-driver CLIPPY_ARGS='' cargo check --locked --workspace --all-targets -j 2`.
The standard `cargo clippy` launcher also requires `/proc/self/exe`. Warnings remain
in existing code, and the new collection builder follows the existing builder's
large `Response` error-return pattern, producing the same `result_large_err` warning.
SQLx retains its pre-existing future-compatibility warning.

