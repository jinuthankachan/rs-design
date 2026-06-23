# Spike: Native addon load under the bundled Node — RPATH/glibc on Ubuntu 24.04 (CP1-Task4)

**Question:** do the two native `.node` addons — **`better-sqlite3`** and **`node-pty`** — load and
*function* under the **bundled** Node 24 (not the dev/nvm Node) on Ubuntu 24.04, with no RPATH
surprises, no build-machine path leakage, and a glibc/GLIBCXX floor the target distro satisfies?
This is the "main reason V1 isn't single-file" risk; it must be a fact before CP6.

**Verdict: both load and run cleanly under a relocated, stripped Node 24, in a fully PATH-stripped
env.** `better-sqlite3` executes real SQL (statically-linked SQLite 3.53.1); `node-pty`, **once
compiled for linux-x64**, spawns a real PTY and reads its output. Neither `.node` carries an
RPATH/RUNPATH, neither needs `patchelf`/relocation, and both link only base system libraries
present on Ubuntu 24.04. The one real task this surfaces: **node-pty has no shipped Linux prebuild
and must be compiled at packaging time** — which succeeds in ~1 s with `build-essential` and pins
the bundle's glibc floor to the build host.

**Environment:** Ubuntu 24.04 (glibc **2.39**, GLIBCXX **3.4.33**), Node **v24.16.0** (ABI/
`NODE_MODULE_VERSION` **137**), `better-sqlite3@12.10.0`, `node-pty@1.1.0`. "Bundled Node" =
`process.execPath` copied to a resource-like `bin/node` and `strip`-ped (101 M), run via `env -i`.

## Load-test — the definitive proof ✅

Ran under the **bundled stripped Node**, `env -i HOME=… PATH=""` (no inherited env), requiring each
addon from the submodule's pnpm tree:

| Addon | Result |
|---|---|
| `better-sqlite3` | ✅ open `:memory:`, `CREATE`/`INSERT`/`SELECT` → `{"x":42}`; `sqlite_version()` → **3.53.1** |
| `node-pty` | ✅ `spawn('/bin/echo', …)` in a real PTY → read `"pty-works"`, exit 0 |

ABI reported at runtime: `NODE_MODULE_VERSION 137`. Both addons were built for ABI 137 and the
bundled Node is 24.16 (also 137) — **ABI is stable across a Node major, so any Node 24.x loads
them**; the bundle just needs to pin *a* Node 24.x.

## ELF / ABI / glibc matrix (measured)

| Artifact | ABI | Max `GLIBC_` | Max `GLIBCXX_` | RPATH/RUNPATH | Notable `NEEDED` | Min distro |
|---|---|---|---|---|---|---|
| bundled `node` 24.16 | 137 | 2.28 | — | none | libstdc++, libc | Ubuntu 18.10+ |
| `better_sqlite3.node` | 137 | **2.29** | **3.4.20** | **none** | base only — **SQLite statically linked** (no `libsqlite3`) | Ubuntu 18.10+ |
| `pty.node` (built on 24.04) | 137 | **2.34** | **3.4.22** | **none** | base only (`libutil` folded into `libc`≥2.34) | Ubuntu 22.04+ |
| **Ubuntu 24.04 (target)** | — | **2.39** | **3.4.33** | — | — | ✓ loads all, with margin |

**Findings:**
- **No RPATH/RUNPATH on either addon.** No build-machine paths baked in, no `$ORIGIN` games, **no
  `patchelf` needed for the addons.** They resolve `libstdc++`/`libgcc_s`/`libc` through the normal
  system loader, which has them on any modern Ubuntu. (The `patchelf` prereq in PACKAGING.md is for
  Tauri/AppImage's WebKit bundling, *not* these `.node` files.)
- **`better-sqlite3` statically links SQLite** — `sqlite_version()` works with no `libsqlite3`
  system dependency. Its prebuild was compiled on an older toolchain (floor **GLIBC 2.29**), so it
  is portable well below our target.
- **The bundle's glibc floor = the host that builds node-pty.** Our locally-built `pty.node` floors
  at **GLIBC 2.34** (forkpty/openpty moved into libc at 2.34), i.e. **Ubuntu 22.04+**. It is the
  highest floor in the bundle — node and better-sqlite3 are lower. For the V1 **Ubuntu 24.04**
  target this is comfortable; to widen support to 20.04 later, build node-pty on an older-glibc
  container (manylinux / 20.04). Tracked for CP6, not a V1 issue.

## node-pty — the build gap, resolved

`node-pty@1.1.0` ships prebuilds only for **darwin** and **win32** — **no Linux binary**, and pnpm
**skips its build script by default** (`Ignored build scripts: node-pty@1.1.0`). So out of the box
there is no loadable node-pty on Linux (consistent with CP1-Task3 and the `/api/terminal`
out-of-smoke decision). Compiling it is straightforward and fast:

```bash
cd <node-pty pkg dir>   # must see its node-addon-api dep (pnpm: build inside .pnpm/node-pty@*/…)
npx node-gyp rebuild    # CXX src/unix/pty.o → SOLINK pty.node  (~1 s)
# → build/Release/pty.node  (75 KB, ABI 137)
```

Requires `build-essential` (g++, make), `python3`, and node-gyp's `node-addon-api` dependency
resolvable — all present on this box. **A clean isolated copy fails** (`Cannot find module
'node-addon-api'`): the build must run where node-pty's own deps resolve.

## CP6 implications

- **Pin a Node 24.x** linux-x64 runtime (ABI 137). Any 24.x loads both addons; no exact-minor match
  needed.
- **Compile node-pty for linux-x64 in the build** (`pnpm approve-builds` then rebuild, or explicit
  `node-gyp rebuild`). Ship the resulting `build/Release/pty.node`. Without this, the terminal is
  dead; the daemon still boots (node-pty is lazy-loaded — CP1-Task3).
- **`better-sqlite3` needs nothing special** — its prebuilt `.node` (built against Node 24) is kept
  as-is; drop only `deps/`+`src/`+`build/Release/obj` (CP1-Task3 prune).
- **No `patchelf` / RPATH patching for the addons.** Don't add relocation steps for `.node` files.
- **glibc floor = node-pty's build host.** Building on the 24.04 CI runner is fine for the 24.04
  target. If V1 later claims broader distro support, move node-pty's compile to an older-glibc
  image; better-sqlite3 and node are already portable.
- **Verify-after-bundle gate** (CP6/CP7): a one-liner that `require()`s both addons under the
  *bundled* node and exercises one op each — exactly the load-test below — catching ABI/glibc/missing
  -binary regressions on a submodule or Node bump.

## How to reproduce

```bash
cd vendor/open-design
# 1. compile node-pty for linux-x64
( cd node_modules/.pnpm/node-pty@1.1.0/node_modules/node-pty && npx --yes node-gyp rebuild )

# 2. inspect (expect: no RPATH/RUNPATH; GLIBC<=2.34; GLIBCXX<=3.4.22; ABI tag via runtime)
readelf -d node_modules/.pnpm/node-pty@1.1.0/node_modules/node-pty/build/Release/pty.node | grep -E 'RPATH|RUNPATH|NEEDED'
objdump -T node_modules/.pnpm/*/node_modules/better-sqlite3/build/Release/better_sqlite3.node | grep -oP 'GLIBC_[0-9.]+' | sort -V | tail -1

# 3. load-test under a relocated, stripped node, env -i
cp "$(readlink -f "$(which node)")" /tmp/bundled-node && strip /tmp/bundled-node
env -i HOME=$HOME PATH="" /tmp/bundled-node -e '
  const r=require("module").createRequire(process.cwd()+"/x");
  const D=r(process.argv[1]); const db=new D(":memory:"); db.exec("create table t(x)"); db.prepare("insert into t values(1)").run();
  console.log("sqlite", db.prepare("select sqlite_version() v").get().v);
  const pty=r(process.argv[2]); const p=pty.spawn("/bin/echo",["ok"],{cols:80,rows:24});
  let b=""; p.onData(d=>b+=d); p.onExit(({exitCode})=>{console.log("pty",JSON.stringify(b.trim()),exitCode);process.exit(0)});
' \
  "$PWD/node_modules/.pnpm/better-sqlite3@12.10.0/node_modules/better-sqlite3" \
  "$PWD/node_modules/.pnpm/node-pty@1.1.0/node_modules/node-pty"
# expect: sqlite 3.53.1 / pty "ok" 0
```

## Implications for the roadmap
- **CP1-Task4:** answered — both addons load/run under the bundled Node on Ubuntu 24.04; no RPATH
  work; node-pty must be compiled (succeeds), and it sets the bundle's glibc floor (2.34).
- **CP6:** pin Node 24.x; compile + ship `pty.node`; keep `better_sqlite3.node` as-is; add the
  verify-after-bundle load-test; if broader-distro support is ever claimed, build node-pty on older
  glibc.
- **PACKAGING.md:** "two `.node` addons load under bundled Node" upgraded from _to confirm_ to
  _verified_, with the glibc/RPATH facts and the node-pty compile requirement.
