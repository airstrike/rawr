# rawr-play

Run a Rawr snippet. A program runs and returns its output as text, and an
iced program also what it drew: a PNG of its settled frame, or a GIF when it
animated.
One long-running bot links `rawr` as a library and starts one `rawr-play`
process per snippet. It runs no rustc and no importer per request. It needs
no GPU and no fonts, so it runs on any Linux VM or container.

## Programs

Every snippet takes one path. The snippet is parsed once, and a program
runs on the base language from that parse, as `rawr run` runs a file: its top level, then `fn main` if it has one. A snippet of
statements alone becomes a block in `fn main`, read as Rust reads a block.
The block's value is printed with `{:?}` unless it is `()`. So `1 + 2`
prints `3`, and `1 + 2;` prints nothing. A `main` that returns `Err` prints
`Error: ..` and exits with status 1. A panic exits with status 101, as in
Rust. A limit exits with status 1. Output is capped at 64 KiB. A program
that writes more is stopped and its output ends with `… output truncated`.
A snippet that names a crate the base language doesn't have fails to
compile, as any program would. An `async fn main` with no runtime's
attribute is rustc's E0752.

## The universe runner

The image names a universe runner (`PLAY_UNIVERSE`, `/universe`), and then
every snippet runs on it instead of the base language
(`play/src/universe.rs`). `rawr-import` builds it once, in the image's
`universe` stage, from `play/universe` with `RAWR_UNIVERSE_RUNNER=1`: the
generated bindings of iced (at the pinned rev), iced_test and tokio, and a
runner that links them.

The bot starts the runner once, as a server, with `RAWR_PLAY_ROOT` and
the socket it serves on (`RAWR_PLAY_SOCKET`). The runner decodes its
bindings, installs them in an engine and binds their runtime, and then
answers each snippet in a fork of itself (`src/play.rs` in rawr). The fork
compiles the snippet beside the universe's `Cargo.toml` against that
engine, with no Cargo and no nightly, and runs `main` once. So a snippet
pays only for its own compile and run, and has its own copy of the
engine, its globals and its native values. Its time budget runs from the
fork, and `--memory-mb` caps its address space. Each `rawr-play` the bot
runs names the server's socket (`PLAY_UNIVERSE_SOCKET`). A `rawr-play` run
that names none starts a server of its own for that one snippet.

iced has no window in the universe. Its `[patch]` puts ?play's shell
(`play/universe/shell`) in place of iced_winit, the crate iced's `run`
hands a program to. The shell runs the program headless, as iced_test's
emulator does, drawn with tiny-skia at the program's own `scale_factor`,
until its boot and the tasks the boot started have finished. Then it keeps
drawing a frame every 50 ms, as iced_winit would redraw: each redraw goes to
the program's subscriptions (`window::frames()`), and a widget's redraw
request is honoured. It stops once nothing is left to change the program:
no update, no pending task, no redraw request, and no subscription that
changed the frame in the last second. It also stops at 100 frames, at 6 MiB
of GIF, or before the deadline `PLAY_DEADLINE` names (the snippet's time
budget). A program whose frame never changed is written as a PNG, and one
that changed as a GIF of each change, with no loop extension, so it plays
once and holds its last frame. Either goes to the file `PLAY_FRAME` names,
and `run` returns, as iced's `run` returns when its window closes. A run
that wrote a frame and exited with status 0 answers with its text and the
frame.

A universe runner names a script type by its index in the program's
definition order (`__RawrScript0`, ..), never by its name, so a snippet
whose native calls are the universe program's runs whatever its types are
called. A snippet that makes a native call the universe program doesn't
make fails to compile, as a program whose native call is missing does.

Locally, after `RAWR_UNIVERSE_RUNNER=1 rawr-import run play/universe`:

```sh
PLAY_UNIVERSE=$PWD/play/universe \
PLAY_UNIVERSE_RUNNER=$PWD/play/universe/target/rawr/runner/target/debug/play \
  cargo run -p rawr-play -- snippet.rs -o -
```

## CLI

```sh
cargo run -p rawr-play -- hello.rs -o target/play/hello.json
echo '1 + 2' | cargo run -p rawr-play -- - -o -
```

A program writes one JSON object:

```json
{"kind":"text","status":0,"output":[{"stream":"stdout","text":"total: 27\n"}],"truncated":false,"limit":false}
```

An iced program's object also has `"frame":{"type":"image/png","data":".."}`:
what it drew, in base64, a PNG or (`image/gif`) a GIF.

`output` holds the program's stdout and stderr in the order it wrote them.
Each entry is one run of text from one stream. `status` is 0. It is 1 when
`main` returned `Err` or a limit stopped it, and 101 when the program
panicked. `limit` says whether a limit stopped it. `truncated` says whether
the output passed 64 KiB. `note`, when present, is a remark on the run, such
as an iterator that nothing consumed.

`--time-ms N` sets the script time budget, and `--memory-mb N`
caps the process's address space (`RLIMIT_AS`, Linux only). Errors go to
stderr with exit status 1.

## Bot

The bot replies to a message whose first line starts with `?play`, or whose
code fence info string contains `?play`. The snippet is the first code
block: ` ```rust `, ` ```rawr ` or a bare fence. A program gets its
output in a ` ```text ` block. Output longer than Discord allows is cut,
with a note. A non-zero exit status follows the block. An iced program
gets its frame attached as `play.png`. An error gets its text in a code
block (at most 2000 characters). It ignores bots. When the author edits a
message it already answered, it runs it again and edits its reply. Over the
rate limit it reacts with ⏳ and does not run.

````text
?play
```rust
let xs = vec![3, 1, 2];
xs.iter().sum::<i64>()
```
````

`?play` takes no options. A word after it on its line gets an error reply.

### Where it answers

A server's mods choose the channels with `/play-channel add [channel]`,
`/play-channel remove [channel]` and `/play-channel list` (the channel
defaults to the current one; replies are visible only to the caller). The
command is shown to members with Manage Channels; admins can change that in
Server Settings → Integrations, and the bot still checks Manage Channels (or
Administrator) in the target channel before changing anything. A server
with no channels chosen gets no answers, and neither do DMs. A thread
follows its parent channel: it is answered when the parent is, and
`/play-channel add|remove` run in (or naming) a thread changes the parent.
`PLAY_CHANNELS`, when set, overrides every server's choice (for local runs),
and is matched against a thread's parent likewise.

Choices persist in `PLAY_CONFIG_URL` (on Cloudflare, the `PlayBot` Durable
Object's storage), else the file `PLAY_CONFIG_FILE`, else
`~/.config/rawr-play/channels.json`.

| Env var | Default | Meaning |
| --- | --- | --- |
| `DISCORD_TOKEN` | — | Bot token; else read from `DISCORD_TOKEN_FILE`, else `~/.config/rawr-play/token` |
| `PLAY_WORKERS` | 2 | Runs at once |
| `PLAY_TIME_MS` | 2000 | Script time budget per run (engine deadline) |
| `PLAY_TIMEOUT_MS` | 10000 | Whole run; the `rawr-play` process is killed after it |
| `PLAY_MEMORY_MB` | 1024 | Address-space cap per run process (Linux) |
| `PLAY_USER_PER_MIN` | 4 | Runs per user per minute |
| `PLAY_CHANNEL_PER_MIN` | 12 | Runs per channel per minute |
| `PLAY_RENDERER` | beside the bot | Path to the `rawr-play` binary |
| `PLAY_CHANNELS` | — | Comma-separated channel IDs to answer in, overriding `/play-channel` |
| `PLAY_CONFIG_URL` | — | `http://` URL whose GET/PUT load and save the `/play-channel` choices |
| `PLAY_CONFIG_FILE` | `~/.config/rawr-play/channels.json` | File for the choices when `PLAY_CONFIG_URL` is unset |
| `PLAY_REPORT_URL` | — | Where boot events (`POST /events`) and minute stats (`PUT /stats`) go; unset: stderr only |
| `PLAY_VERSION` | `dev` | The image's commit, baked in by `push-image.sh` |
| `PLAY_HTTP_PORT` | — | Serve the playground's `POST /render` on this port |
| `PLAY_LOG_KEY` | — | 64 hex digits; servers are reported by token under it, and not at all without it |

## Limits and sandbox

Snippets are untrusted. Each run is its own `rawr-play` process, started
with an empty environment, reading the code on stdin and writing its result
on stdout. The bot kills it at `PLAY_TIMEOUT_MS`. On Linux the process's
address space is capped at `PLAY_MEMORY_MB`, so a runaway allocation aborts
that run only and the bot replies "out of memory".

Inside, a fresh Engine has: 1,000,000 operations per run, call depth of at
most 32, 64 KiB strings, 65,536-element arrays and maps, and a wall-clock
deadline checked from the progress callback. Each limit gets a plain reply
such as `operation limit exceeded: more than 1000000 operations (an endless
loop?)`.

A snippet reads no file. `include_str!`, `include_bytes!` and `std::fs` fail
with `?play has no files`. The Engine disables `eval` and `import`, installs
a module resolver that loads nothing, and replaces `sleep` with an error.

## Run locally

With the token in `~/.config/rawr-play/token` (or `DISCORD_TOKEN`), and
confined to one test channel (right-click the channel → Copy Channel ID,
with Developer Mode on):

```sh
PLAY_CHANNELS=<channel id> cargo run --release -p rawr-play --features bot --bin rawr-play-bot
```

The bot starts `rawr-play` from beside its own binary, so build both first
(`cargo build --release -p rawr-play --features bot --bins`; `cargo run`
builds only the bot). The memory cap applies on Linux only.

## Deploy on Cloudflare Containers

A run needs no GPU and no fonts. The bot's TLS is OpenSSL through native-tls.
The distroless runtime image already ships libssl3 and a CA bundle. The bot
only makes outbound connections (Discord's gateway), so one always-on
container runs it. `play/deploy/cloudflare` is a Worker whose
`PlayBot` Durable Object owns that container: it never sleeps, and a
five-minute cron (and `GET /bot/health`) starts it again if it stopped. The
container has no disk: the bot keeps `/play-channel` choices in the Durable
Object's storage through `http://play-config.internal/channels`, a host the
Worker intercepts for the container only (no public endpoint, no secret).
The bot also reports its boot phases and aggregate stats there each minute
(runs, code errors, limits, median and p95 time, rate-limited counts,
servers and channels; no message content, user IDs or server names), and
`https://rawr.airstrike.fyi/bot` shows them, with the last 50 boot events,
as a page in RawrGallery's Ink sheet.

The same Worker serves all of `rawr.airstrike.fyi`. The landing page, the
docs and the playground's page come from the site's Astro build
(`site/dist`; `npm ci` in `site/` once). The invite redirect at
`/bot/invite` and the link preview cards in `/og/` come from `public/`.
`build.mjs` builds the site and merges the two into `dist/`, and the build
fails if a path is in both. Wrangler runs it before every deploy. For
`/play` and `/play/<id>` the Worker takes the site's static `play.html` and
writes in the request's link preview, a share's snippet and theme, the
examples and Turnstile's key (`src/playpage.ts`). The Worker runs before the assets so it can answer the old
host. Every request to `rawr-bot.airstrike.fyi` gets a 301 to the same path
on `rawr.airstrike.fyi`. `/` there goes to `/bot` and `/invite` goes to
`/bot/invite`.

Servers appear on the dashboard only as tokens: the guild ID as one
AES-256 block under `PLAY_LOG_KEY` (32 hex digits; the same server always
gives the same token). The Worker never has the key. Locally, with the key
in `~/.config/rawr-play/log-key` (or `PLAY_LOG_KEY_FILE`):

```sh
openssl rand -hex 32 > ~/.config/rawr-play/log-key && chmod 600 ~/.config/rawr-play/log-key  # once
npx wrangler secret put PLAY_LOG_KEY < ~/.config/rawr-play/log-key
cargo run -p rawr-play -- decrypt-server <token>...    # prints each token's guild ID
cargo run -p rawr-play -- encrypt-server <guild id>... # the token of a known server
```

### Playground

`https://rawr.airstrike.fyi/play` is a web playground: an editor
(Tab indents, Ctrl/Cmd+Enter runs), plain Rust examples from
`play/tests/fixtures/rust` (embedded when the Worker is built) and the
output: a program's output as a terminal with its exit status, or the
same error text the bot replies with. A run is in the browser by default (rawr-web, `web/`), and on the
server on request. **Share** stores the code in the Durable Object under a
content-addressed id (`/play/<id>`, at most 64 KiB; the same snippet is the
same id).

A shared link unfurls with its first line of output as the description and
the site's card (`site/public/og/card.png`) as the image. A busy or
unreachable server is retried after ten minutes. Shares not
visited for 30 days are purged by the cron once a day, with their previews.
The editor keeps the latest snippet in the browser (`localStorage`): bare
`/play` restores it; a share shows the share until it is edited; Examples →
Reset brings back the first example.

Server runs go Worker → Durable Object → the bot's HTTP port
(`PLAY_HTTP_PORT`, 8080, reachable only through the Durable Object) → the
same child process as `?play`, with the same limits. The bot's `POST
/render` reads `{"code": ..}` and answers with `application/json`: a
program's output. Playground runs
share the pool but run one at a time and never ahead of a waiting Discord
run. Each IP
(kept only as an HMAC) gets 6 runs a minute and 60 an hour, and 10 shares a
minute; code is capped at 16 KiB; Cloudflare Turnstile (site key in
`vars.TURNSTILE_SITEKEY`, secret `TURNSTILE_SECRET`) checks browsers. A
request carrying `x-play-admin: <hex HMAC-SHA256 of "rawr-play admin">`
skips Turnstile but not /play's limits. The HMAC key is the log key's 64 hex
digits as text, not the 32 bytes they spell:
`printf 'rawr-play admin' | openssl dgst -sha256 -hmac "$(cat ~/.config/rawr-play/log-key)" -r`. The dashboard counts
playground runs apart from Discord's.

The landing page has Run buttons for its own files. They call `POST
/play/run` with a file's exact text. That route needs no Turnstile. It
accepts only the text of a `<code data-run>` element in the site's built
pages, checked by SHA-256. `build.mjs` collects them when it builds, so the
list is always the deployed pages'. A result is
cached for each image version. Uncached runs are limited per IP (20 a
minute, 120 an hour). A request whose `x-play-admin` is the admin HMAC
or the Worker's secret `PLAY_SMOKE_TOKEN` skips that limit. `deploy.sh`
sends `PLAY_SMOKE_TOKEN` from its environment, or else the admin HMAC of
the local log key, so its smoke test never waits on the limit.

A deploy does not replace a Durable Object's running container, which keeps
the image it started from. `push-image.sh` writes the image's commit into
both the image and `vars.PLAY_VERSION`; the Durable Object records the
version it started, and the cron or `GET /bot/health` restarts a container
started from any other.

The image is `play/Dockerfile`. Its Rust stages run on the build host's
architecture. They cross-compile to linux/amd64 (or arm64) when the host
differs, and build natively when it matches. Each cargo build keeps its
registry, git checkouts and target directory in BuildKit cache mounts, so a
build with a warm cache recompiles only rawr's own crates. rawr-import and
its driver, which only build the universe and are not shipped, build
without LTO. The runtime is `gcr.io/distroless/cc-debian12:nonroot`
with the two stripped binaries. `push-image.sh` builds it with Apple's `container` (no Docker
needed), pushes it to `registry.cloudflare.com` and writes its address into
`wrangler.jsonc`; since `image` is then a registry address, `wrangler
deploy` never calls Docker.

```sh
cd play/deploy/cloudflare
npm install
npx wrangler login
./push-image.sh            # builds the committed HEAD (CLOUDFLARE_ACCOUNT_ID overrides the account)
npx wrangler secret put DISCORD_TOKEN
npx wrangler deploy        # runs build.mjs first
curl https://rawr.airstrike.fyi/bot/health   # restarts the bot onto the new image now
```

After the first setup, one command releases the committed HEAD:

```sh
npm run release            # image, tests, deploy, wait for the bot, smoke test, commit the bump, push main
npm run release:worker     # Worker and site only, no new image
```

It refuses to run while the Worker or site sources have uncommitted changes.
`deploy.sh` does the part after the image: the tests, the deploy, the wait
for the bot and the smoke test. It retries the deploy when Cloudflare
answers 100146 or a 500.

### Release from GitHub Actions

`.github/workflows/release.yml` (the **Release** workflow) releases from
GitHub instead of a Mac and is the main path. It runs by hand, from the
Actions tab (Run workflow). It chains four stages. Each is a reusable
workflow that also runs on its own, and each starts only after the one
before it succeeded or was skipped. A failure stops every stage after it.

| Stage | Workflow | What it does | When it runs |
| --- | --- | --- | --- |
| 1. binaries | `binaries.yml` | Builds the prebuilt archives for the GitHub release of the version. | Only when the `binaries` input is ticked. Patch releases have no tags yet. |
| 2. playground | `playground.yml` | Builds rawr-web into the run's `web` artifact. | Every release. |
| 3. bot | `bot.yml` | Builds the ?play image and pushes it as `rawr-play:<commit>`. | When the image's inputs changed since the image Cloudflare runs (`bot: auto`). `always` builds anyway and `skip` never builds. |
| 4. site | `site.yml` | Deploys the Worker and site with the new rawr-web and the new image, or the running image when stage 3 built none. Then it waits for the bot and runs the site's snippets. | Every release. |

Stages 2 and 3 only publish: an artifact and a registry image. The site
stage makes the one deploy, so the site never goes live ahead of its bot,
and a failed stage leaves the live site as it was. Run alone, `playground.yml`
and `bot.yml` deploy through `site.yml` themselves (input `deploy`). A
playground deploy keeps the running image. A bot deploy keeps the live
rawr-web.

The bot stage skips its build when nothing the image is built from has
changed. `image-changed.sh` compares this commit with the commit of the
running image (its tag). It counts every file the Docker build copies
except the ones no build step reads: `site/`, `plan/`, `play/deploy/`,
`.github/`, `guides/`, `CHANGELOG.md` and rawr-web's sources. It ignores
a release commit's version bumps. A new top-level directory counts as an
input until it is listed there.

The image builds on `ubuntu-latest` (x86-64) with Docker Buildx, natively,
with 24 GB of swap and four rustc jobs. Its cargo caches are BuildKit cache
mounts (`play/Dockerfile`). `buildkit-cache-dance` moves them between the
builder and `actions/cache`, under a key made of the external dependencies
of `Cargo.lock` (`scripts/deps-key.sh`, which ignores the workspace's own
versions), the Dockerfile and the universe's manifest. iced, wgpu, tokio and
serenity therefore compile once per dependency or toolchain change. Every
other build compiles only rawr's crates and the universe's bindings. The
rawr-web build caches the same way.

Expected times, measured from the last single-job release (about 38
minutes):

- Playground: about 3 minutes. The rawr-web build took 2.5 minutes with its
  cache.
- Bot, cold cache: about 15 to 20 minutes. Before, the universe stage took
  20 minutes: 9.5 for rawr-import and 4 for its driver (fat LTO, one codegen
  unit), and 6.5 for the runner. The bot binary took 5 minutes in parallel,
  and exporting the layer cache took another 5. Without LTO the two tools
  should take a third to a half of that time, and the cache export is now
  small.
- Bot, warm cache: about 8 to 12 minutes, mostly rawr's own crates and the
  runner's bindings. A release that changes no image input skips the build:
  under a minute.
- Site: 2 to 4 minutes on the running image. A new image adds up to 5
  minutes for the rollout and the bot's first runs.

A whole release that changes no bot input takes about 7 minutes. One that
does takes about 15 to 25 minutes. These figures are estimates until the
first runs measure them.

The image is tagged with its commit. `pin-image.sh` writes the image and
`PLAY_VERSION` into the job's own copy of `wrangler.jsonc`. The job never
commits, so the committed `wrangler.jsonc` keeps the last image
`release.sh` pushed. Every deploy without a new image runs `pin-image.sh`
with no image, which reads the image Cloudflare runs. The jobs never see the
Discord token, which stays among the Worker's secrets.

A deploy replaces every asset, so a deploy that builds no rawr-web ships the
live one again. `web/build.sh` writes `SHA256SUMS` beside rawr-web, which
the site serves as `/web/SHA256SUMS`. `live-web.sh` reads it from the site,
fetches each file it lists and checks each hash.

### Deploy the site and the Worker between releases

`.github/workflows/site.yml` also runs on every push to main that changes
`plan/`, `site/` or the Worker (`play/deploy/cloudflare`). `/todo`, the
changelog, the docs, the Worker's code and its settings therefore go live
within minutes, on the running image with the live rawr-web. One example is
`PLAY_TIME_MS` in `wrangler.jsonc`. A settings change restarts the bot on
the image it already runs. This deploy builds nothing in Rust.

Every workflow takes the concurrency group `release-play` with
`queue: max` when it runs on its own. A deploy never runs beside a release,
and a waiting run never cancels another. A stage called from another
workflow takes a group of its own, because the caller already holds
`release-play`. A deploy reads the running image only once its turn comes,
so a release that ran before it is never rolled back.

The first release with these workflows has to build rawr-web, because the
live site has no `/web/SHA256SUMS` before it. Until that release, a
site-only deploy stops at `live-web.sh` with a message that says so. Run
**Release** with `bot: always` the first time. It rebuilds the image with
the new Dockerfile and saves the cold caches. In the run, watch the `bot /
image` job's "Build the image" step for the cold time, and the `site /
site` job for the rollout and the snippets. Then run **Release** again with
`bot: always` to measure a warm build. After that, `auto` is the default.

The repository needs two secrets and one variable, under Settings → Secrets
and variables → Actions:

- The secret `CLOUDFLARE_API_TOKEN` holds a Cloudflare API token.
- The secret `PLAY_SMOKE_TOKEN` holds the same random value as the
  Worker's secret of that name. Make one and set both from stdin, so it is
  never shown:
  `t="$(openssl rand -hex 32)"; printf '%s' "$t" | npx wrangler secret put PLAY_SMOKE_TOKEN; printf '%s' "$t" | gh secret set PLAY_SMOKE_TOKEN; unset t`.
- The variable `CLOUDFLARE_ACCOUNT_ID` is `fce80b8101fbb2f85548560babc6c7bc`.
  It is not a secret.

Make the token in the Cloudflare dashboard from the "Edit Cloudflare Workers"
template. Scope it to this account and to the `airstrike.fyi` zone. Then add
the account permission Containers: Edit. That gives these permissions:

- Account: Workers Scripts: Edit (the Worker, its Durable Object and cron).
- Account: Containers: Edit (the registry push and the container rollout).
- Account: Account Settings: Read.
- Zone: Workers Routes: Edit (the two custom domains).
- Zone: Zone: Read.

The template's other permissions (Workers KV, R2, Tail and Memberships) are
harmless but unused.

`instance_type` is `lite` (1/16 vCPU, 256 MiB), and `PLAY_WORKERS=1` fits
in it. Switch to `basic` (1/4 vCPU, 1 GiB) for faster runs. With Docker installed,
`docker build --platform linux/amd64 -f play/Dockerfile -t rawr-play .`
from the repository root builds the same image.

### Discord application

1. In the Developer Portal, create an application, add a Bot, and copy its
   token into `~/.config/rawr-play/token` locally, or `npx wrangler secret
   put DISCORD_TOKEN` (never into the repository).
2. Under Bot → Privileged Gateway Intents, turn on **Message Content
   Intent**. The bot asks for `GUILD_MESSAGES`, `DIRECT_MESSAGES` and
   `MESSAGE_CONTENT`.
3. Invite it with scopes `bot` and `applications.commands` and permissions View Channels, Send Messages,
   Send Messages in Threads, Attach Files, Read Message History and Add
   Reactions (`274878008384`):
   `https://discord.com/oauth2/authorize?client_id=<APPLICATION_ID>&scope=bot+applications.commands&permissions=274878008384`
4. The bot registers `/play-channel` globally when it connects; a mod then
   runs `/play-channel add` in each channel it should answer in.
