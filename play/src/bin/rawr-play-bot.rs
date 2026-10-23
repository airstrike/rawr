//! `rawr-play-bot`: run `?play` code blocks posted to Discord. An iced
//! snippet replies with its image, a plain program with its output.
//!
//! Each render runs in its own `rawr-play` process (beside this binary, or
//! `PLAY_RENDERER`), so a render past `PLAY_TIMEOUT_MS` is killed and one
//! past `PLAY_MEMORY_MB` aborts without touching the bot.
//!
//! Env: `DISCORD_TOKEN` (or a file at `DISCORD_TOKEN_FILE`, else
//! `~/.config/rawr-play/token`), `PLAY_WORKERS` (default 2),
//! `PLAY_TIME_MS` (script time per render, default 2000),
//! `PLAY_TIMEOUT_MS` (whole render, default 10000), `PLAY_MEMORY_MB`
//! (address space per render on Linux, default 1024), `PLAY_FORMAT`
//! (png, webp or jpg; default png: Discord shows it unchanged), `PLAY_USER_PER_MIN` (default 4),
//! `PLAY_CHANNEL_PER_MIN` (default 12), `PLAY_CHANNELS` (comma-separated
//! channel IDs to answer in, overriding `/play-channel`), `PLAY_CONFIG_URL`
//! or `PLAY_CONFIG_FILE` (where `/play-channel` choices persist; default
//! `~/.config/rawr-play/channels.json`), `PLAY_REPORT_URL` (where boot
//! events and minute stats go; unset: stderr only), `PLAY_VERSION` (the
//! image's commit, baked in at build).
//!
//! `PLAY_HTTP_PORT`, when set, serves the web playground's `POST /render`
//! (the Worker is the only client); its renders share the render pool, but
//! one at a time and never ahead of a waiting Discord render.
//!
//! Mods choose where the bot answers with `/play-channel add|remove|list`;
//! a server with no channels chosen gets no answers.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rawr_play::channels::{self, Channels, Parents, Store};
use rawr_play::message::{self, Done, RateLimit, Reply};
use rawr_play::server::ServerKey;
use rawr_play::stats::{self, Batch, Outcome, Report, Status};
use serenity::all::{
    Channel, ChannelId, ChannelType, Command, CommandInteraction, CommandOptionType, Context,
    CreateAttachment, CreateCommand, CreateCommandOption, CreateMessage, EditInteractionResponse,
    EditMessage, EventHandler, GatewayIntents, Guild, GuildChannel, GuildId, Interaction,
    InteractionContext, Message, MessageId, MessageUpdateEvent, Permissions, ReactionType, Ready,
    ResolvedValue, UnavailableGuild,
};
use serenity::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

/// Replies kept for re-rendering on edit.
const REMEMBERED: usize = 1024;

/// The mods' command that chooses where the bot answers.
const COMMAND: &str = "play-channel";

struct Handler {
    defaults: rawr_play::Options,
    format: rawr_play::Format,
    shared: Arc<Shared>,
    /// Thread → parent, so a thread follows its parent channel.
    parents: Mutex<Parents>,
    users: Mutex<RateLimit>,
    channel_rate: Mutex<RateLimit>,
    /// Request message → our reply, oldest first.
    replies: Mutex<(HashMap<MessageId, MessageId>, VecDeque<MessageId>)>,
}

/// A render's result: the image or output, or its error text, how it
/// ended, how long.
struct Rendered {
    result: Result<Done, String>,
    outcome: Outcome,
    millis: u128,
}

/// The render pool: `PLAY_WORKERS` child processes at once. A playground
/// render waits its turn behind every Discord render that is waiting, and
/// only one runs or waits at a time, so Discord waits for at most one.
struct Renderer {
    path: PathBuf,
    workers: Semaphore,
    web: Semaphore,
    discord_waiting: AtomicUsize,
    memory_mb: u64,
    timeout: Duration,
    /// `PLAY_FONTS`: the renderer draws with these fonts only.
    fonts: Option<String>,
}

impl Renderer {
    /// Render in a child process; the child is killed when the timeout
    /// drops it. `None`: a playground render found the playground busy.
    async fn run(
        &self,
        request: &message::Request,
        format: rawr_play::Format,
        web: bool,
    ) -> Option<Rendered> {
        let _web = if web {
            let gate = tokio::time::timeout(Duration::from_secs(15), self.web.acquire()).await;
            let gate = gate.ok()?.ok()?;
            while self.discord_waiting.load(Ordering::Acquire) > 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Some(gate)
        } else {
            self.discord_waiting.fetch_add(1, Ordering::AcqRel);
            None
        };
        let permit = self.workers.acquire().await;
        if !web {
            self.discord_waiting.fetch_sub(1, Ordering::AcqRel);
        }
        let Ok(_permit) = permit else {
            return Some(Rendered {
                result: Err("render pool is closed".to_owned()),
                outcome: Outcome::Limit,
                millis: 0,
            });
        };
        let started = Instant::now();
        let (result, outcome) = self.spawn(request, format).await;
        Some(Rendered {
            result,
            outcome,
            millis: started.elapsed().as_millis(),
        })
    }

    async fn spawn(
        &self,
        request: &message::Request,
        format: rawr_play::Format,
    ) -> (Result<Done, String>, Outcome) {
        let child = tokio::process::Command::new(&self.path)
            .args(request.args(format, self.memory_mb))
            .args(self.fonts.iter().flat_map(|dir| ["--fonts", dir.as_str()]))
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                eprintln!(
                    "rawr-play-bot: cannot start {}: {error}",
                    self.path.display()
                );
                return (Err("cannot start a render".to_owned()), Outcome::Limit);
            }
        };
        let code = request.code.clone().into_bytes();
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let run = async move {
            let _ = stdin.write_all(&code).await;
            drop(stdin);
            child.wait_with_output().await
        };
        match tokio::time::timeout(self.timeout, run).await {
            Ok(Ok(output)) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let success = output.status.success();
                let outcome = stats::classify(success, !output.stdout.is_empty(), &stderr);
                let result = message::outcome(success, output.stdout, &stderr);
                let outcome = match &result {
                    Ok(Done::Text(output)) => stats::classify_output(output),
                    _ => outcome,
                };
                (result, outcome)
            }
            Ok(Err(error)) => {
                eprintln!("rawr-play-bot: render failed: {error}");
                (Err("render crashed".to_owned()), Outcome::Limit)
            }
            Err(_) => (
                Err(format!(
                    "render timed out after {} ms",
                    self.timeout.as_millis()
                )),
                Outcome::Limit,
            ),
        }
    }
}

/// What the reporter task shares with the handler.
struct Shared {
    renderer: Renderer,
    /// Channels to answer in. Held across a save, so saves keep their order.
    channels: tokio::sync::Mutex<Channels>,
    /// Whether `channels` came from the store; until then changes are
    /// refused, so a store that is briefly unreachable is never overwritten.
    loaded: AtomicBool,
    store: Store,
    /// `PLAY_CHANNELS`.
    only: std::collections::HashSet<u64>,
    report: Report,
    /// `PLAY_LOG_KEY`: servers are reported by token, and not at all without it.
    key: Option<ServerKey>,
    stats: Mutex<Batch>,
    version: String,
    boot: u64,
    /// Unix seconds of the first ready; 0 before it.
    ready: AtomicU64,
    servers: AtomicU64,
    reporting: AtomicBool,
}

impl Shared {
    async fn status(&self) -> Status {
        let channels = self.channels.lock().await.count();
        Status {
            version: self.version.clone(),
            boot: self.boot,
            ready: Some(self.ready.load(Ordering::Relaxed)).filter(|&ready| ready != 0),
            servers: self.servers.load(Ordering::Relaxed),
            channels: channels as u64,
            channels_loaded: self.loaded.load(Ordering::Relaxed),
        }
    }

    /// Send the batch so far; keep it for next time if it does not arrive.
    async fn send_stats(&self) {
        let status = self.status().await;
        let per_server = self.channels.lock().await.per_server();
        let batch = self.stats.lock().unwrap().take();
        let rows = batch.server_rows(self.key.as_ref(), &per_server);
        if !self.report.stats(batch.to_json(&status, rows)).await {
            self.stats.lock().unwrap().restore(batch);
        }
    }

    fn record(&self, outcome: Outcome, millis: u128, guild: Option<GuildId>) {
        self.stats
            .lock()
            .unwrap()
            .record(outcome, millis, guild.map(GuildId::get));
    }

    /// An event about a server: its token in the detail, or nothing that
    /// names it without the key.
    async fn server_event(&self, phase: &str, guild: GuildId, detail: &str) {
        let detail = match &self.key {
            Some(key) => {
                let sep = if detail.is_empty() { "" } else { ", " };
                format!("server {}{sep}{detail}", key.encrypt(guild.get()))
            }
            None => detail.to_owned(),
        };
        self.report.event(phase, &detail).await;
    }

    /// Load the saved channels: every 2 s for the first tries, then every
    /// 30 s, until it works. Until then the bot answers nowhere and refuses
    /// changes rather than overwrite what it could not read.
    async fn load_channels(self: Arc<Self>) {
        let mut tries = 0u32;
        loop {
            let loaded = self
                .store
                .load()
                .await
                .and_then(|text| Channels::from_json(&text, self.only.clone()));
            match loaded {
                Ok(channels) => {
                    let count = channels.count();
                    *self.channels.lock().await = channels;
                    self.loaded.store(true, Ordering::Relaxed);
                    self.report
                        .event("channels loaded", &format!("{count} channel(s)"))
                        .await;
                    return;
                }
                Err(error) => {
                    if tries == 0 || tries == 5 {
                        self.report.event("channels unreachable", &error).await;
                    }
                    tries += 1;
                    let wait = if tries < 5 { 2 } else { 30 };
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }
            }
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, context: Context, msg: Message) {
        if msg.author.bot {
            return;
        }
        // Parse first: only a `?play` message may need its thread's parent.
        let Some(request) = message::parse(&msg.content, &self.defaults) else {
            return;
        };
        if !self.listens(&context, msg.guild_id, msg.channel_id).await {
            return;
        }
        if !self.allow(msg.author.id.get(), msg.channel_id) {
            self.shared
                .stats
                .lock()
                .unwrap()
                .rate_limited(msg.guild_id.map(GuildId::get));
            let _ = msg
                .react(&context.http, ReactionType::Unicode("⏳".into()))
                .await;
            return;
        }
        let reply = self.reply(request, msg.guild_id).await;
        let builder = CreateMessage::new().reference_message(&msg);
        let builder = match reply {
            Reply::Image(image, note) => {
                let builder = builder.add_file(self.attachment(image));
                match note {
                    Some(note) => builder.content(format!("-# {note}")),
                    None => builder,
                }
            }
            Reply::Text(text) => builder.content(text),
        };
        match msg.channel_id.send_message(&context.http, builder).await {
            Ok(sent) => self.remember(msg.id, sent.id),
            Err(error) => eprintln!("rawr-play-bot: cannot reply: {error}"),
        }
    }

    /// Re-render a request we already answered when its author edits it.
    async fn message_update(
        &self,
        context: Context,
        _old: Option<Message>,
        _new: Option<Message>,
        event: MessageUpdateEvent,
    ) {
        let Some(content) = event.content else {
            return;
        };
        if !self
            .listens(&context, event.guild_id, event.channel_id)
            .await
        {
            return;
        }
        let Some(reply_id) = self.replies.lock().unwrap().0.get(&event.id).copied() else {
            return;
        };
        let Some(request) = message::parse(&content, &self.defaults) else {
            return;
        };
        let user = event.author.map_or(0, |author| author.id.get());
        if !self.allow(user, event.channel_id) {
            self.shared
                .stats
                .lock()
                .unwrap()
                .rate_limited(event.guild_id.map(GuildId::get));
            return;
        }
        let builder = match self.reply(request, event.guild_id).await {
            Reply::Image(image, note) => EditMessage::new()
                .content(note.map_or_else(String::new, |note| format!("-# {note}")))
                .remove_all_attachments()
                .new_attachment(self.attachment(image)),
            Reply::Text(text) => EditMessage::new().content(text).remove_all_attachments(),
        };
        if let Err(error) = event
            .channel_id
            .edit_message(&context.http, reply_id, builder)
            .await
        {
            eprintln!("rawr-play-bot: cannot edit reply: {error}");
        }
    }

    /// Register `/play-channel`: global, guild-only, and shown by default to
    /// members with Manage Channels (admins can retarget it per server).
    async fn ready(&self, context: Context, ready: Ready) {
        let channel = || {
            CreateCommandOption::new(
                CommandOptionType::Channel,
                "channel",
                "Default: this channel",
            )
            .channel_types(vec![
                ChannelType::Text,
                ChannelType::News,
                ChannelType::Voice,
                ChannelType::PublicThread,
                ChannelType::PrivateThread,
                ChannelType::NewsThread,
            ])
        };
        let sub = |name, description| {
            CreateCommandOption::new(CommandOptionType::SubCommand, name, description)
        };
        let command = CreateCommand::new(COMMAND)
            .description("Choose the channels where ?play answers")
            .default_member_permissions(Permissions::MANAGE_CHANNELS)
            .dm_permission(false)
            .contexts(vec![InteractionContext::Guild])
            .add_option(sub("add", "Answer ?play in a channel").add_sub_option(channel()))
            .add_option(
                sub("remove", "Stop answering ?play in a channel").add_sub_option(channel()),
            )
            .add_option(sub("list", "List the channels where ?play answers"));
        let shared = &self.shared;
        shared
            .servers
            .store(ready.guilds.len() as u64, Ordering::Relaxed);
        let _ =
            shared
                .ready
                .compare_exchange(0, stats::now(), Ordering::Relaxed, Ordering::Relaxed);
        match Command::create_global_command(&context.http, command).await {
            Ok(_) => {
                let detail = format!("{} server(s), /{COMMAND} registered", ready.guilds.len());
                shared.report.event("ready", &detail).await;
            }
            Err(error) => {
                let detail = format!("cannot register /{COMMAND}: {error}");
                shared.report.event("error", &detail).await;
            }
        }
        // One reporter for the process, whatever the reconnects.
        if !shared.reporting.swap(true, Ordering::Relaxed) {
            let shared = Arc::clone(shared);
            tokio::spawn(async move {
                loop {
                    shared.send_stats().await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    let servers = context.cache.guild_count();
                    shared.servers.store(servers as u64, Ordering::Relaxed);
                }
            });
        }
    }

    /// Joining a server is an event (by token); the cache fills either way.
    async fn guild_create(&self, _context: Context, guild: Guild, is_new: Option<bool>) {
        if is_new == Some(true) {
            self.shared
                .server_event("joined server", guild.id, "")
                .await;
        }
    }

    async fn guild_delete(&self, _context: Context, gone: UnavailableGuild, _full: Option<Guild>) {
        if !gone.unavailable {
            self.shared.server_event("left server", gone.id, "").await;
        }
    }

    async fn interaction_create(&self, context: Context, interaction: Interaction) {
        let Interaction::Command(command) = interaction else {
            return;
        };
        if command.data.name != COMMAND {
            return;
        }
        if let Err(error) = command.defer_ephemeral(&context.http).await {
            eprintln!("rawr-play-bot: cannot answer /{COMMAND}: {error}");
            return;
        }
        let text = self.play_channel(&context, &command).await;
        let edit = EditInteractionResponse::new().content(text);
        if let Err(error) = command.edit_response(&context.http, edit).await {
            eprintln!("rawr-play-bot: cannot answer /{COMMAND}: {error}");
        }
    }
}

impl Handler {
    async fn listens(&self, context: &Context, guild: Option<GuildId>, channel: ChannelId) -> bool {
        let parent = self.parent(context, guild, channel).await;
        self.shared.channels.lock().await.listens_in(
            guild.map(GuildId::get),
            channel.get(),
            parent.map(ChannelId::get),
        )
    }

    /// A thread's parent channel; `None` for any other channel. From the
    /// memo, else the guild cache, else one HTTP request (a failed one is
    /// not remembered).
    async fn parent(
        &self,
        context: &Context,
        guild: Option<GuildId>,
        channel: ChannelId,
    ) -> Option<ChannelId> {
        let guild = guild?;
        if let Some(known) = self.parents.lock().unwrap().get(channel.get()) {
            return known.map(ChannelId::new);
        }
        let cached = context.cache.guild(guild).and_then(|guild| {
            guild
                .channels
                .get(&channel)
                .or_else(|| guild.threads.iter().find(|thread| thread.id == channel))
                .map(thread_parent)
        });
        let parent = match cached {
            Some(parent) => parent,
            None => match context.http.get_channel(channel).await {
                Ok(Channel::Guild(found)) => thread_parent(&found),
                Ok(_) => None,
                Err(error) => {
                    eprintln!("rawr-play-bot: cannot look up channel {channel}: {error}");
                    return None;
                }
            },
        };
        self.parents
            .lock()
            .unwrap()
            .insert(channel.get(), parent.map(ChannelId::get));
        parent
    }

    fn allow(&self, user: u64, channel: ChannelId) -> bool {
        let now = Instant::now();
        self.users.lock().unwrap().allow(user, now)
            && self.channel_rate.lock().unwrap().allow(channel.get(), now)
    }

    /// Run `/play-channel add|remove|list`; the reply text (ephemeral).
    async fn play_channel(&self, context: &Context, command: &CommandInteraction) -> String {
        let Some(guild) = command.guild_id else {
            return format!("Use /{COMMAND} in a server.");
        };
        let options = command.data.options();
        let Some((name, args)) = options.first().and_then(|option| match &option.value {
            ResolvedValue::SubCommand(args) => Some((option.name, args)),
            _ => None,
        }) else {
            return format!("Unknown /{COMMAND} command.");
        };
        // The channel option carries the invoker's permissions there; the
        // current channel's are the member's. A thread stands for its parent.
        let option = args.iter().find_map(|arg| match arg.value {
            ResolvedValue::Channel(channel) => Some(channel),
            _ => None,
        });
        let (channel, permissions, parent) = match option {
            Some(channel) => (
                channel.id,
                channel.permissions,
                is_thread(channel.kind)
                    .then_some(channel.parent_id)
                    .flatten(),
            ),
            None => (
                command.channel_id,
                command
                    .member
                    .as_ref()
                    .and_then(|member| member.permissions),
                self.parent(context, Some(guild), command.channel_id).await,
            ),
        };
        let target = parent.unwrap_or(channel);
        // Discord gives a thread its parent's permissions; compute them in
        // the parent when the cache has it.
        let permissions = parent
            .zip(command.member.as_deref())
            .and_then(|(parent, member)| {
                let guild = context.cache.guild(guild)?;
                Some(guild.user_permissions_in(guild.channels.get(&parent)?, member))
            })
            .or(permissions);
        let via = if parent.is_some() {
            format!(" (<#{channel}> is a thread, so it follows its parent channel)")
        } else {
            String::new()
        };
        let mut channels = self.shared.channels.lock().await;
        let note = if channels.overridden() {
            "\n(PLAY_CHANNELS is set on this bot, so it answers only in the channels listed there.)"
        } else {
            ""
        };
        if name == "list" {
            let list = channels.list(guild.get());
            if list.is_empty() {
                return format!("?play answers nowhere in this server. Add a channel with /{COMMAND} add.{note}");
            }
            let list: Vec<String> = list.iter().map(|id| format!("<#{id}>")).collect();
            return format!("?play answers in {}.{note}", list.join(", "));
        }
        let channel = target;
        if !self.shared.loaded.load(Ordering::Relaxed) {
            return "The saved channels have not loaded yet, so nothing can change; try again in a minute.".to_owned();
        }
        if !channels::may_configure(permissions.map_or(0, |p| p.bits())) {
            return format!(
                "You need Manage Channels in <#{channel}> to change where ?play answers."
            );
        }
        let (changed, done) = match name {
            "add" => (channels.add(guild.get(), channel.get()), "now answers"),
            "remove" => (
                channels.remove(guild.get(), channel.get()),
                "no longer answers",
            ),
            _ => return format!("Unknown /{COMMAND} command."),
        };
        if !changed {
            let already = if name == "add" {
                "already answers"
            } else {
                "does not answer"
            };
            return format!("?play {already} in <#{channel}>{via}.{note}");
        }
        let count = channels.list(guild.get()).len();
        let saved = self.shared.store.save(&channels.to_json()).await;
        drop(channels);
        let phase = if name == "add" {
            "channel added"
        } else {
            "channel removed"
        };
        let detail = format!("{count} channel(s) now");
        self.shared.server_event(phase, guild, &detail).await;
        if let Err(error) = saved {
            eprintln!("rawr-play-bot: cannot save channels: {error}");
            return format!("?play {done} in <#{channel}>{via}, but saving failed, so this lasts only until the bot restarts.{note}");
        }
        format!("?play {done} in <#{channel}>{via}.{note}")
    }

    fn attachment(&self, image: Vec<u8>) -> CreateAttachment {
        CreateAttachment::bytes(image, format!("play.{}", self.format.extension()))
    }

    fn remember(&self, request: MessageId, reply: MessageId) {
        let (map, order) = &mut *self.replies.lock().unwrap();
        if map.insert(request, reply).is_none() {
            order.push_back(request);
        }
        while order.len() > REMEMBERED {
            if let Some(oldest) = order.pop_front() {
                map.remove(&oldest);
            }
        }
    }

    async fn reply(
        &self,
        request: Result<message::Request, String>,
        guild: Option<GuildId>,
    ) -> Reply {
        match request {
            Err(error) => Reply::Text(message::error_text(&error)),
            Ok(request) => self.render(&request, guild).await,
        }
    }

    async fn render(&self, request: &message::Request, guild: Option<GuildId>) -> Reply {
        let Some(rendered) = self.shared.renderer.run(request, self.format, false).await else {
            return Reply::Text(message::error_text("render pool is busy"));
        };
        self.shared.record(rendered.outcome, rendered.millis, guild);
        match rendered.result {
            Ok(Done::Image(image)) => Reply::Image(image, message::image_note(&request.code)),
            Ok(Done::Text(output)) => Reply::Text(message::output_text(&output)),
            Err(text) => Reply::Text(message::error_text(&text)),
        }
    }
}

/// Serve `POST /render` (the playground, through the Worker) and `GET
/// /health` on `port`: a PNG, or a plain program's output as JSON
/// (`rawr_play::Output::to_json`), on success; the error text as 422
/// otherwise.
async fn serve(shared: Arc<Shared>, defaults: rawr_play::Options, port: u16) {
    use http_body_util::{BodyExt, Full, Limited};
    use hyper::body::{Bytes, Incoming};
    use hyper::{Method, Response, StatusCode};

    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            shared
                .report
                .event("error", &format!("cannot listen on port {port}: {error}"))
                .await;
            return;
        }
    };
    let defaults = Arc::new(defaults);
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let shared = Arc::clone(&shared);
        let defaults = Arc::clone(&defaults);
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |request: hyper::Request<Incoming>| {
                let shared = Arc::clone(&shared);
                let defaults = Arc::clone(&defaults);
                async move {
                    let reply = |status: StatusCode, kind: &str, body: Vec<u8>| {
                        Response::builder()
                            .status(status)
                            .header("content-type", kind)
                            .body(Full::new(Bytes::from(body)))
                    };
                    let text = |status, text: String| {
                        reply(status, "text/plain; charset=utf-8", text.into_bytes())
                    };
                    match (request.method(), request.uri().path()) {
                        (&Method::GET, "/health") => text(StatusCode::OK, "ok".into()),
                        (&Method::POST, "/render") => {
                            let limit = rawr_play::web::MAX_CODE * 2 + 16 * 1024;
                            let body =
                                match Limited::new(request.into_body(), limit).collect().await {
                                    Ok(body) => body.to_bytes(),
                                    Err(_) => {
                                        return text(
                                            StatusCode::PAYLOAD_TOO_LARGE,
                                            "the request is too large".into(),
                                        )
                                    }
                                };
                            let render = match rawr_play::web::parse_render(&body, &defaults) {
                                Ok(render) => render,
                                Err(error) => return text(StatusCode::BAD_REQUEST, error),
                            };
                            let format = rawr_play::Format::Png;
                            let Some(rendered) = shared.renderer.run(&render, format, true).await
                            else {
                                return text(
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    "the playground is busy; try again in a moment".into(),
                                );
                            };
                            shared
                                .stats
                                .lock()
                                .unwrap()
                                .record_web(rendered.outcome, rendered.millis);
                            match rendered.result {
                                Ok(Done::Image(image)) => reply(StatusCode::OK, "image/png", image),
                                Ok(Done::Text(output)) => reply(
                                    StatusCode::OK,
                                    "application/json",
                                    output.to_json().into_bytes(),
                                ),
                                Err(error) => text(StatusCode::UNPROCESSABLE_ENTITY, error),
                            }
                        }
                        _ => text(StatusCode::NOT_FOUND, "not found".into()),
                    }
                }
            });
            let io = hyper_util::rt::TokioIo::new(stream);
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await;
        });
    }
}

/// Read every file in `dir` once and drop the bytes: the kernel keeps them
/// in its page cache for the renders that memory-map them.
fn warm_fonts(dir: &std::path::Path) {
    use std::io::Read;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut buffer = vec![0; 1 << 20];
    for entry in entries.flatten() {
        if let Ok(mut file) = std::fs::File::open(entry.path()) {
            while matches!(file.read(&mut buffer), Ok(n) if n > 0) {}
        }
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// `DISCORD_TOKEN`, else the file at `DISCORD_TOKEN_FILE`, else
/// `~/.config/rawr-play/token`.
fn token() -> Option<String> {
    if let Ok(token) = std::env::var("DISCORD_TOKEN") {
        return Some(token.trim().to_owned()).filter(|token| !token.is_empty());
    }
    let path = std::env::var_os("DISCORD_TOKEN_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/rawr-play/token"))
        })?;
    let token = std::fs::read_to_string(path).ok()?;
    Some(token.trim().to_owned()).filter(|token| !token.is_empty())
}

fn is_thread(kind: ChannelType) -> bool {
    matches!(
        kind,
        ChannelType::PublicThread | ChannelType::PrivateThread | ChannelType::NewsThread
    )
}

/// A thread's parent; `None` for any other channel (a channel's own
/// `parent_id` is its category).
fn thread_parent(channel: &GuildChannel) -> Option<ChannelId> {
    is_thread(channel.kind)
        .then_some(channel.parent_id)
        .flatten()
}

#[tokio::main]
async fn main() {
    let Some(token) = token() else {
        eprintln!(
            "rawr-play-bot: set DISCORD_TOKEN, or DISCORD_TOKEN_FILE, or write \
             ~/.config/rawr-play/token"
        );
        std::process::exit(2);
    };
    let renderer = std::env::var_os("PLAY_RENDERER").map_or_else(
        || {
            std::env::current_exe()
                .expect("current executable")
                .with_file_name("rawr-play")
        },
        PathBuf::from,
    );
    let format = match std::env::var("PLAY_FORMAT") {
        Ok(value) => value.parse().unwrap_or_else(|error| {
            eprintln!("rawr-play-bot: {error}");
            std::process::exit(2);
        }),
        Err(_) => rawr_play::Format::Png,
    };
    let only = message::channels(&std::env::var("PLAY_CHANNELS").unwrap_or_default())
        .unwrap_or_else(|error| {
            eprintln!("rawr-play-bot: PLAY_CHANNELS: {error}");
            std::process::exit(2);
        });
    if !only.is_empty() {
        eprintln!(
            "rawr-play-bot: PLAY_CHANNELS: answering only in {} channel(s)",
            only.len()
        );
    }
    let workers = usize::try_from(env_u64("PLAY_WORKERS", 2))
        .unwrap_or(2)
        .max(1);
    let shared = Arc::new(Shared {
        renderer: Renderer {
            path: renderer,
            workers: Semaphore::new(workers),
            web: Semaphore::new(1),
            discord_waiting: AtomicUsize::new(0),
            memory_mb: env_u64("PLAY_MEMORY_MB", 1024),
            timeout: Duration::from_millis(env_u64("PLAY_TIMEOUT_MS", 10_000)),
            fonts: std::env::var("PLAY_FONTS")
                .ok()
                .filter(|dir| !dir.is_empty()),
        },
        channels: tokio::sync::Mutex::new(
            Channels::from_json("", only.clone()).unwrap_or_default(),
        ),
        loaded: AtomicBool::new(false),
        store: Store::from_env(),
        only,
        report: Report::from_env(),
        key: ServerKey::from_env(),
        stats: Mutex::new(Batch::default()),
        version: std::env::var("PLAY_VERSION").unwrap_or_else(|_| "dev".to_owned()),
        boot: stats::now(),
        ready: AtomicU64::new(0),
        servers: AtomicU64::new(0),
        reporting: AtomicBool::new(false),
    });
    let mut defaults = rawr_play::Options::default();
    defaults.limits.time = Duration::from_millis(env_u64("PLAY_TIME_MS", 2_000));
    // Read the bundled fonts once, so the page cache holds them before the
    // first render maps them (a cold first CJK render ran past its budget).
    if let Some(dir) = shared.renderer.fonts.clone() {
        tokio::task::spawn_blocking(move || warm_fonts(std::path::Path::new(&dir)));
    }
    // Listen first: the container counts as started once this port answers.
    if let Some(port) = std::env::var("PLAY_HTTP_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
    {
        tokio::spawn(serve(Arc::clone(&shared), defaults.clone(), port));
    }
    shared.report.event("booting", &shared.version).await;
    shared.send_stats().await;
    tokio::spawn(Arc::clone(&shared).load_channels());
    let per_minute = |name, default| {
        RateLimit::new(
            u32::try_from(env_u64(name, default)).unwrap_or(u32::MAX),
            Duration::from_secs(60),
        )
    };
    let handler = Handler {
        defaults,
        format,
        shared: Arc::clone(&shared),
        parents: Mutex::new(Parents::new(REMEMBERED)),
        users: Mutex::new(per_minute("PLAY_USER_PER_MIN", 4)),
        channel_rate: Mutex::new(per_minute("PLAY_CHANNEL_PER_MIN", 12)),
        replies: Mutex::new((HashMap::new(), VecDeque::new())),
    };
    // GUILDS fills the guild cache (channels, threads) that thread → parent
    // and the parent's permissions are read from.
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;
    let client = serenity::Client::builder(&token, intents)
        .event_handler(handler)
        .await;
    let error = match client {
        Ok(mut client) => match client.start().await {
            Ok(()) => "the gateway closed".to_owned(),
            Err(error) => error.to_string(),
        },
        Err(error) => error.to_string(),
    };
    shared
        .report
        .event("error", &format!("exiting: {error}"))
        .await;
    shared.send_stats().await;
    std::process::exit(1);
}
