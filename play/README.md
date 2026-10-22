# rawr-play

Run a Rawr snippet. A snippet that uses iced renders to a PNG. A plain
program runs and returns its output as text. One long-running process links
`rawr` and `rawr-iced` as libraries. It runs no rustc and no importer per request. A
snippet is checked as Rawr, its view is evaluated headlessly, and the result
is rendered with the `rawr snapshot` path: tiny-skia, scale 1.0, and the
embedded Fira Sans font. It needs no GPU, so it runs on any Linux VM or
container.

## Plain programs and iced snippets

The snippet is parsed once. rawr-iced's manifest (`iced/manifest.toml`)
says what is iced's, as it does for `rawr`. A snippet is iced's when it
names the `iced` crate. It is also iced's when it uses a name of the
manifest's `prelude` that it neither declares nor imports, such as `column!`
or `text`. Every other snippet is a plain program.

A plain program runs on the base language from that same parse, as `rawr
run` runs a file: its top level, then `fn main` if it has one. A snippet of
statements alone becomes a block in `fn main`, read as Rust reads a block.
The block's value is printed with `{:?}` unless it is `()`. So `1 + 2` prints
`3`, and `1 + 2;` prints nothing. A `main` that returns `Err`
prints `Error: ..` and exits with status 1. A panic exits with status 101,
as in Rust. A limit exits with status 1. Output is capped at 64 KiB. A program that writes more
is stopped and its output ends with `… output truncated`. Messages drive an iced app. A plain program runs without them, and its
output carries the note "messages are ignored for programs without iced".

## Snippet shapes for iced

- A bare expression: `column![text("hi"), button("go")]`
- A stateless view: `fn view() -> impl Widget<()> { ... }`
- A full app: a type with `fn view(&self)` and `fn update(&mut self, message)`.
  Messages (`--message` / `msg=`) are applied in order to `State::default()`
  before the view renders.

Expressions and `fn view()` snippets without `use` lines get the imports in
the manifest's `prelude`. A snippet with its own `#[preview]` is refused (its size would
bypass the 2048 cap); use `size=WxH` instead.

## CLI

```sh
cargo run -p rawr-play -- hello.rawr --size 400x300 --theme dark -o target/play/hello.png
cargo run -p rawr-play -- counter.rawr --message Message::Increment -o target/play/counter.webp
echo 'text("hi")' | cargo run -p rawr-play -- - --format webp -o - > target/play/hi.webp
```

The format follows `--format png|jpg|webp`, else the output extension, else
PNG. A plain program writes JSON where an image would go. The JSON is one
object that starts with `{`, and no image does:

```json
{"kind":"text","status":0,"output":[{"stream":"stdout","text":"total: 27\n"}],"truncated":false,"limit":false}
```

`output` holds the program's stdout and stderr in the order it wrote them.
Each entry is one run of text from one stream. `status` is 0. It is 1 when
`main` returned `Err` or a limit stopped it, and 101 when the program panicked. `limit`
says whether a limit stopped it. `truncated` says whether the output passed
64 KiB. `note`, when present, is a remark on the run, such as ignored messages. `--time-ms N` sets the script time budget and `--memory-mb N` caps the
process's address space (`RLIMIT_AS`, Linux only). Errors go to stderr with
exit status 1.

## Formats

UI renders are flat colour and text. Lossless WebP is the smallest and
Discord previews it inline, so the bot sends WebP by default (`PLAY_FORMAT`):

| Sample | PNG | WebP (lossless) | JPEG q90 |
| --- | --- | --- | --- |
| app, 640x400 | 13 KB | 7.6 KB | 18 KB |
| layout, 400x300 | 8.4 KB | 5.3 KB | 9.3 KB |
| styled card, 400x300 | 3.8 KB | 2.7 KB | 6.5 KB |

JPEG is larger and blurs text edges; it is there for clients that need it.

## Bot

The bot replies to a message whose first line starts with `?play`, or whose
code fence info string contains `?play`. The snippet is the first code
block: ` ```rust `, ` ```rawr ` or a bare fence. It answers with the image
attached, or with the error text in a code block (at most 2000 characters).
A plain program gets its output in a ` ```text ` block. Output longer than
Discord allows is cut, with a note. A non-zero exit status follows the
block.
It ignores bots. When the author edits a message it already answered, it
re-renders and edits its reply. Over the rate limit it reacts with ⏳ and
does not render.

````text
?play 320x200 light msg=Message::Increment
```rust
column![text("hi"), button("go")]
```
````

Options: `size=WxH` (or a bare `WxH`, at most 2048 per side),
`theme=dark|light` (or a bare `dark`/`light`), and repeated `msg=EXPR`.

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
| `PLAY_WORKERS` | 2 | Renders running at once |
| `PLAY_TIME_MS` | 2000 | Script time budget per render (engine deadline) |
| `PLAY_TIMEOUT_MS` | 10000 | Whole render; the render process is killed after it |
| `PLAY_MEMORY_MB` | 1024 | Address-space cap per render process (Linux) |
| `PLAY_FORMAT` | png | Reply image: png, webp (lossless) or jpg; PNG is shown unchanged by Discord |
| `PLAY_USER_PER_MIN` | 4 | Renders per user per minute |
| `PLAY_CHANNEL_PER_MIN` | 12 | Renders per channel per minute |
| `PLAY_RENDERER` | beside the bot | Path to the `rawr-play` binary |
| `PLAY_CHANNELS` | — | Comma-separated channel IDs to answer in, overriding `/play-channel` |
| `PLAY_CONFIG_URL` | — | `http://` URL whose GET/PUT load and save the `/play-channel` choices |
| `PLAY_CONFIG_FILE` | `~/.config/rawr-play/channels.json` | File for the choices when `PLAY_CONFIG_URL` is unset |
| `PLAY_REPORT_URL` | — | Where boot events (`POST /events`) and minute stats (`PUT /stats`) go; unset: stderr only |
| `PLAY_VERSION` | `dev` | The image's commit, baked in by `push-image.sh` |
| `PLAY_FONTS` | — | Fonts directory passed to each render as `--fonts` |
| `PLAY_HTTP_PORT` | — | Serve the playground's `POST /render` on this port |
| `PLAY_LOG_KEY` | — | 64 hex digits; servers are reported by token under it, and not at all without it |

## Fonts

The renderer draws with iced's embedded Fira Sans and, for anything it
lacks, bundled Noto fonts: CJK (one collection; Han without a locale reads
as Simplified Chinese, kana as Japanese, Hangul as Korean), color emoji,
symbols, sans and mono. `play/fetch-fonts.sh DIR` fetches the pinned files
and checks their hashes; the image puts them in `/usr/share/rawr-play/fonts`
and sets `PLAY_FONTS`, which the bot passes to each render as `--fonts DIR`.
With `--fonts` the renderer sees those files and nothing of the system's, on
any machine, so a Mac's system fonts cannot hide a missing glyph (`cargo
test -p rawr-play --test fonts` after `play/fetch-fonts.sh
target/play/fonts`). The files are memory-mapped and read only when a glyph
falls back to them: one release render on macOS peaked at 23.4 MB for ASCII
and 26.0 MB for CJK with emoji (23.7 MB with system fonts).

## Limits and sandbox

Snippets are untrusted. Each render is its own `rawr-play` process, started
with an empty environment, reading the code on stdin and writing the image
on stdout. The bot kills it at `PLAY_TIMEOUT_MS`. On Linux the process's
address space is capped at `PLAY_MEMORY_MB`, so a runaway allocation aborts
that render only and the bot replies "out of memory".

A plain program runs under the same process limits. Its Engine has the same
limits as a render. Its output is capped at 64 KiB.

Inside, a fresh Engine has: 1,000,000 operations per evaluation, call depth
of at most 32, 64 KiB strings, 10,000-element arrays and maps, and a
wall-clock deadline checked from the progress callback. Canvas sides are at
most 2048. Each limit gets a plain reply such as `operation limit exceeded:
more than 1000000 operations (an endless loop?)`.

The iced facade has no filesystem, network, process or environment
functions, and `render_png` touches no files. Untyped functions
(`fn f(x) { .. }`) still reach the base language, so `?play` also disables
`print`, `debug`, `eval` and `import`, installs a module resolver that loads
nothing, and replaces `sleep` with an error.

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

The renderer is tiny-skia on the CPU with the Fira Sans font compiled in.
It needs no GPU, no system fonts and no CA bundle (rustls ships its roots),
and the bot only makes outbound connections (Discord's gateway), so one
always-on container runs it. `play/deploy/cloudflare` is a Worker whose
`PlayBot` Durable Object owns that container: it never sleeps, and a
five-minute cron (and `GET /bot/health`) starts it again if it stopped. The
container has no disk: the bot keeps `/play-channel` choices in the Durable
Object's storage through `http://play-config.internal/channels`, a host the
Worker intercepts for the container only (no public endpoint, no secret).
The bot also reports its boot phases and aggregate stats there each minute
(renders, code errors, limits, median and p95 time, rate-limited counts,
servers and channels; no message content, user IDs or server names), and
`https://rawr.airstrike.fyi/bot` shows them, with the last 50 boot events,
as a page in RawrGallery's Ink sheet.

The same Worker serves all of `rawr.airstrike.fyi`. The landing page at `/`
comes from `site/public`. The invite redirect at `/bot/invite` and the link
preview cards in `/og/` come from `public/`. `build.mjs` merges the two into
`dist/`, and the build fails if a path is in both. Wrangler runs it before
every deploy. The Worker runs before the assets so it can answer the old
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
(Tab indents, Ctrl/Cmd+Enter runs), dark/light, an optional size, messages
(one `Message` expression per line, like `msg=`), iced's examples from
`play/tests/fixtures` (embedded when the Worker is built) and the output: the
PNG, a plain program's output as a terminal with its exit status, or the same
error text the bot replies with. **Share** stores the code
and options in the Durable Object under a content-addressed id
(`/play/<id>`, at most 64 KiB; the same snippet is the same id).

A shared link unfurls with its own first frame: `/play/<id>` carries Open
Graph and Twitter card tags whose image is `/play/<id>.png`. That PNG is
rendered once (concurrent first requests share the render; a share warms
it), kept in the Durable Object's SQLite (at most 1 MiB) and in the edge
cache, and served `immutable`, since an id is a content hash. A snippet that
does not render gets an error card (`public/og/play-error.png`, drawn from
`og/error.rs`), kept too. A plain program's share keeps its output. Its
link shows the site's card (`site/public/og/card.png`), and its first line
of output is the description; a busy or unreachable renderer is retried after
ten minutes. Uncached previews are limited per IP (10 a minute, 60 an hour),
without Turnstile. Shares not visited (page or image) for 30 days are
purged by the cron once a day, with their previews. The editor keeps the
latest snippet in the browser (`localStorage`): bare `/play` restores it; a
share shows the share until it is edited; Examples → Reset brings back the
counter.

Renders go Worker → Durable Object → the bot's HTTP port (`PLAY_HTTP_PORT`,
8080, reachable only through the Durable Object) → the same child-process
renderer as `?play`, with the same limits. The bot's `POST /render` answers
with `image/png`, or with `application/json` for a plain program's output.
Playground renders share the pool
but run one at a time and never ahead of a waiting Discord render. Each IP
(kept only as an HMAC) gets 6 runs a minute and 60 an hour, and 10 shares a
minute; code is capped at 16 KiB; Cloudflare Turnstile (site key in
`vars.TURNSTILE_SITEKEY`, secret `TURNSTILE_SECRET`) checks browsers. A
request carrying `x-play-admin: <hex HMAC-SHA256 of "rawr-play admin">`
skips Turnstile but not the limits. The HMAC key is the log key's 64 hex
digits as text, not the 32 bytes they spell:
`printf 'rawr-play admin' | openssl dgst -sha256 -hmac "$(cat ~/.config/rawr-play/log-key)" -r`. The dashboard counts
playground runs apart from Discord's.

The landing page has Run buttons for its own files. They call `POST
/play/run` with a file's exact text. That route needs no Turnstile. It
accepts only the text of a `<code data-run>` element in
`site/public/index.html`, checked by SHA-256. The Worker embeds that page
when it is built, so the list is always the deployed page's. A result is
cached for each image version. Uncached runs are limited per IP (20 a
minute, 120 an hour).

A deploy does not replace a Durable Object's running container, which keeps
the image it started from. `push-image.sh` writes the image's commit into
both the image and `vars.PLAY_VERSION`; the Durable Object records the
version it started, and the cron or `GET /bot/health` restarts a container
started from any other.

The image is `play/Dockerfile`: a `rust:1-slim-bookworm` stage that
cross-compiles from the build host's architecture to linux/amd64 (or arm64),
and a `gcr.io/distroless/cc-debian12:nonroot` runtime with the two stripped
binaries. `push-image.sh` builds it with Apple's `container` (no Docker
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

`instance_type` is `lite` (1/16 vCPU, 256 MiB): a render's peak RSS was
27 MB for a 640x400 app, 41 MB for 100k widgets stopped at the time limit
and 63 MB at the 2048x2048 maximum (macOS `/usr/bin/time -l`, release),
so `PLAY_WORKERS=1` fits with room to spare. Switch to
`basic` (1/4 vCPU, 1 GiB) for faster renders. With Docker installed,
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
