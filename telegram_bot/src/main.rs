use anyhow::{anyhow, Context, Result};
use client_rust::{HamrahClient, Object};
use std::collections::HashSet;
use std::sync::Arc;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, MediaKind, MessageId, MessageKind,
};
use teloxide::utils::command::BotCommands;
use tokio::sync::Mutex;

type Client = Arc<Mutex<HamrahClient>>;
type Admins = Arc<HashSet<i64>>;

const PAGE_SIZE: usize = 5;
const DEFAULT_LINK_DURATION: u32 = 86_400; // 24h
const DEFAULT_LINK_LIMIT: u32 = 100;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
enum Cmd {
    #[command(description = "Show this help.")]
    Help,
    #[command(description = "Show welcome message.")]
    Start,
    #[command(description = "List your files.")]
    List,
    #[command(description = "Browse / manage files with buttons.")]
    Manage,
    #[command(description = "Publish a file by id: /publish [id]")]
    Publish(String),
    #[command(description = "Delete a file by id: /delete [id]")]
    Delete(String),
    #[command(description = "Show your Telegram user id.")]
    Whoami,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let token = std::env::var("TELOXIDE_TOKEN")
        .or_else(|_| std::env::var("TELEGRAM_TOKEN"))
        .context("TELOXIDE_TOKEN env not set")?;
    let phone = std::env::var("HAMRAH_PHONE").context("HAMRAH_PHONE env not set")?;
    let password = std::env::var("HAMRAH_PASSWORD").context("HAMRAH_PASSWORD env not set")?;
    let admin_ids_raw = std::env::var("ADMIN_IDS").context("ADMIN_IDS env not set (comma separated Telegram user ids)")?;
    let proxy = std::env::var("HAMRAH_PROXY").ok().filter(|s| !s.is_empty());

    let admins: HashSet<i64> = admin_ids_raw
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .collect();
    if admins.is_empty() {
        return Err(anyhow!("ADMIN_IDS parsed to empty set"));
    }
    let admins: Admins = Arc::new(admins);
    log::info!("Loaded {} admins", admins.len());

    let mut client = HamrahClient::new(proxy.as_deref());
    client
        .login(&phone, &password)
        .await
        .map_err(|e| anyhow!("Hamrah login failed: {e}"))?;
    log::info!("Hamrah login OK");
    let client: Client = Arc::new(Mutex::new(client));

    let bot = Bot::new(token);

    let msg_handler = Update::filter_message()
        .filter(|msg: Message, admins: Admins| {
            msg.from().map(|u| admins.contains(&(u.id.0 as i64))).unwrap_or(false)
        })
        .branch(
            dptree::entry()
                .filter_command::<Cmd>()
                .endpoint(|bot: Bot, msg: Message, cmd: Cmd, client: Client| async move {
                    if let Err(e) = handle_command(bot.clone(), msg.clone(), cmd, client).await {
                        log::warn!("command failed: {e}");
                        let _ = bot.send_message(msg.chat.id, format!("❌ {e}")).await;
                    }
                    Ok::<(), teloxide::RequestError>(())
                }),
        )
        .branch(dptree::endpoint(
            |bot: Bot, msg: Message, client: Client| async move {
                if let Err(e) = handle_message(bot.clone(), msg.clone(), client).await {
                    log::warn!("message handler failed: {e}");
                    let _ = bot.send_message(msg.chat.id, format!("❌ {e}")).await;
                }
                Ok::<(), teloxide::RequestError>(())
            },
        ));

    let cb_handler = Update::filter_callback_query()
        .filter(|q: CallbackQuery, admins: Admins| admins.contains(&(q.from.id.0 as i64)))
        .endpoint(|bot: Bot, q: CallbackQuery, client: Client| async move {
            if let Err(e) = handle_callback(bot, q, client).await {
                log::warn!("callback failed: {e}");
            }
            Ok::<(), teloxide::RequestError>(())
        });

    Dispatcher::builder(bot, dptree::entry().branch(msg_handler).branch(cb_handler))
        .dependencies(dptree::deps![client, admins])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

async fn handle_command(bot: Bot, msg: Message, cmd: Cmd, client: Client) -> Result<()> {
    match cmd {
        Cmd::Help | Cmd::Start => {
            bot.send_message(msg.chat.id, Cmd::descriptions().to_string())
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
        }
        Cmd::Whoami => {
            let id = msg.from().map(|u| u.id.0).unwrap_or(0);
            bot.send_message(msg.chat.id, format!("Your Telegram id: {id}"))
                .await?;
        }
        Cmd::List => {
            let objs = {
                let mut c = client.lock().await;
                c.list_objects().await.map_err(|e| anyhow!(e.to_string()))?
            };
            let text = format_list(&objs, 0);
            bot.send_message(msg.chat.id, text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await?;
        }
        Cmd::Manage => {
            let objs = {
                let mut c = client.lock().await;
                c.list_objects().await.map_err(|e| anyhow!(e.to_string()))?
            };
            send_manage_page(&bot, msg.chat.id, &objs, 0, None).await?;
        }
        Cmd::Publish(id_str) => {
            let id: u64 = id_str.trim().parse().context("invalid id")?;
            let link = {
                let c = client.lock().await;
                c.create_public_link(id, DEFAULT_LINK_DURATION, DEFAULT_LINK_LIMIT)
                    .await
                    .map_err(|e| anyhow!(e.to_string()))?
            };
            bot.send_message(msg.chat.id, format!("Published: {}", link.link))
                .await?;
        }
        Cmd::Delete(id_str) => {
            let id: u64 = id_str.trim().parse().context("invalid id")?;
            {
                let c = client.lock().await;
                c.delete_file(id).await.map_err(|e| anyhow!(e.to_string()))?;
            }
            bot.send_message(msg.chat.id, format!("Deleted {id}")).await?;
        }
    }
    Ok(())
}

async fn handle_message(bot: Bot, msg: Message, client: Client) -> Result<()> {
    // Decide: media upload, URL fetch, or unknown.
    if let MessageKind::Common(common) = &msg.kind {
        match &common.media_kind {
            MediaKind::Document(d) => {
                let name = d.document.file_name.clone().unwrap_or_else(|| "file.bin".to_string());
                upload_telegram_file(&bot, &msg, client, &d.document.file.id, &name).await?;
                return Ok(());
            }
            MediaKind::Photo(p) => {
                let largest = p.photo.last().ok_or_else(|| anyhow!("no photo sizes"))?;
                let name = format!("photo_{}.jpg", msg.id.0);
                upload_telegram_file(&bot, &msg, client, &largest.file.id, &name).await?;
                return Ok(());
            }
            MediaKind::Video(v) => {
                let name = v
                    .video
                    .file_name
                    .clone()
                    .unwrap_or_else(|| format!("video_{}.mp4", msg.id.0));
                upload_telegram_file(&bot, &msg, client, &v.video.file.id, &name).await?;
                return Ok(());
            }
            MediaKind::Audio(a) => {
                let name = a
                    .audio
                    .file_name
                    .clone()
                    .unwrap_or_else(|| format!("audio_{}.mp3", msg.id.0));
                upload_telegram_file(&bot, &msg, client, &a.audio.file.id, &name).await?;
                return Ok(());
            }
            MediaKind::Voice(v) => {
                let name = format!("voice_{}.ogg", msg.id.0);
                upload_telegram_file(&bot, &msg, client, &v.voice.file.id, &name).await?;
                return Ok(());
            }
            MediaKind::Animation(a) => {
                let name = a
                    .animation
                    .file_name
                    .clone()
                    .unwrap_or_else(|| format!("anim_{}.mp4", msg.id.0));
                upload_telegram_file(&bot, &msg, client, &a.animation.file.id, &name).await?;
                return Ok(());
            }
            MediaKind::Text(t) => {
                if let Some(url) = extract_url(&t.text) {
                    upload_from_url(&bot, &msg, client, &url).await?;
                    return Ok(());
                }
            }
            _ => {}
        }
    }

    bot.send_message(
        msg.chat.id,
        "Send a file/photo/video, or a https URL, or /help.",
    )
    .await?;
    Ok(())
}

async fn upload_telegram_file(
    bot: &Bot,
    msg: &Message,
    client: Client,
    file_id: &str,
    name: &str,
) -> Result<()> {
    let status = bot
        .send_message(msg.chat.id, format!("⬆️ Uploading {name} …"))
        .await?;

    let file = bot.get_file(file_id).await?;
    let mut buf: Vec<u8> = Vec::with_capacity(file.size as usize);
    bot.download_file(&file.path, &mut buf).await?;

    let res: std::result::Result<(), String> = {
        let mut c = client.lock().await;
        c.upload_bytes(name, buf).await.map_err(|e| e.to_string())
    };
    match res {
        Ok(()) => {
            bot.edit_message_text(msg.chat.id, status.id, format!("✅ Uploaded {name}"))
                .await?;
        }
        Err(e) => {
            bot.edit_message_text(msg.chat.id, status.id, format!("❌ Upload failed: {e}"))
                .await?;
        }
    }
    Ok(())
}

async fn upload_from_url(bot: &Bot, msg: &Message, client: Client, url: &str) -> Result<()> {
    let status = bot
        .send_message(msg.chat.id, format!("🌐 Fetching {url} …"))
        .await?;

    let resp = match reqwest::get(url).await {
        Ok(r) => r,
        Err(e) => {
            bot.edit_message_text(msg.chat.id, status.id, format!("❌ Fetch failed: {e}"))
                .await?;
            return Ok(());
        }
    };
    if !resp.status().is_success() {
        bot.edit_message_text(
            msg.chat.id,
            status.id,
            format!("❌ Fetch returned {}", resp.status()),
        )
        .await?;
        return Ok(());
    }

    let name = filename_from_url(url, resp.headers());
    let bytes = match resp.bytes().await {
        Ok(b) => b.to_vec(),
        Err(e) => {
            bot.edit_message_text(msg.chat.id, status.id, format!("❌ Read failed: {e}"))
                .await?;
            return Ok(());
        }
    };

    bot.edit_message_text(
        msg.chat.id,
        status.id,
        format!("⬆️ Uploading {name} ({} bytes) …", bytes.len()),
    )
    .await?;

    let res: std::result::Result<(), String> = {
        let mut c = client.lock().await;
        c.upload_bytes(&name, bytes).await.map_err(|e| e.to_string())
    };
    match res {
        Ok(()) => {
            bot.edit_message_text(msg.chat.id, status.id, format!("✅ Uploaded {name}"))
                .await?;
        }
        Err(e) => {
            bot.edit_message_text(msg.chat.id, status.id, format!("❌ Upload failed: {e}"))
                .await?;
        }
    }
    Ok(())
}

async fn handle_callback(bot: Bot, q: CallbackQuery, client: Client) -> Result<()> {
    let data = q.data.clone().unwrap_or_default();
    let chat_id = q
        .message
        .as_ref()
        .map(|m| m.chat().id)
        .unwrap_or(ChatId(q.from.id.0 as i64));
    let msg_id = q.message.as_ref().map(|m| m.id());

    bot.answer_callback_query(q.id.clone()).await.ok();

    let mut parts = data.splitn(2, ':');
    let action = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("");

    match action {
        "page" => {
            let page: usize = arg.parse().unwrap_or(0);
            let objs = {
                let mut c = client.lock().await;
                c.list_objects().await.map_err(|e| anyhow!(e.to_string()))?
            };
            send_manage_page(&bot, chat_id, &objs, page, msg_id).await?;
        }
        "pub" => {
            let id: u64 = arg.parse().context("bad id")?;
            let res: std::result::Result<client_rust::PublicLinkResponse, String> = {
                let c = client.lock().await;
                c.create_public_link(id, DEFAULT_LINK_DURATION, DEFAULT_LINK_LIMIT)
                    .await
                    .map_err(|e| e.to_string())
            };
            match res {
                Ok(link) => {
                    bot.send_message(chat_id, format!("🔗 {}", link.link)).await?;
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("❌ Publish failed: {e}"))
                        .await?;
                }
            }
        }
        "del" => {
            let id: u64 = arg.parse().context("bad id")?;
            let res: std::result::Result<(), String> = {
                let c = client.lock().await;
                c.delete_file(id).await.map_err(|e| e.to_string())
            };
            match res {
                Ok(()) => {
                    bot.send_message(chat_id, format!("🗑 Deleted {id}")).await?;
                    // Refresh page 0
                    let objs = {
                        let mut c = client.lock().await;
                        c.list_objects().await.map_err(|e| anyhow!(e.to_string()))?
                    };
                    send_manage_page(&bot, chat_id, &objs, 0, msg_id).await?;
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("❌ Delete failed: {e}"))
                        .await?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn format_list(objs: &[Object], page: usize) -> String {
    if objs.is_empty() {
        return "No files.".to_string();
    }
    let pages = (objs.len() + PAGE_SIZE - 1) / PAGE_SIZE;
    let page = page.min(pages.saturating_sub(1));
    let start = page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(objs.len());
    let mut s = format!("<b>Files</b> (page {}/{}, {} total):\n", page + 1, pages.max(1), objs.len());
    for o in &objs[start..end] {
        let size = o.size.map(human_size).unwrap_or_else(|| "-".into());
        let name = o.name.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        s.push_str(&format!("• <code>{}</code> — {} ({})\n", o.id, name, size));
    }
    s.push_str("\nUse <code>/publish [id]</code> or <code>/delete [id]</code>, or /manage for buttons.");
    s
}

async fn send_manage_page(
    bot: &Bot,
    chat: ChatId,
    objs: &[Object],
    page: usize,
    edit: Option<MessageId>,
) -> Result<()> {
    if objs.is_empty() {
        if let Some(mid) = edit {
            bot.edit_message_text(chat, mid, "No files.").await?;
        } else {
            bot.send_message(chat, "No files.").await?;
        }
        return Ok(());
    }
    let pages = (objs.len() + PAGE_SIZE - 1) / PAGE_SIZE;
    let page = page.min(pages - 1);
    let start = page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(objs.len());

    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for o in &objs[start..end] {
        let label = truncate(&o.name, 30);
        rows.push(vec![InlineKeyboardButton::callback(
            format!("📄 {label}"),
            format!("noop:{}", o.id),
        )]);
        rows.push(vec![
            InlineKeyboardButton::callback("🔗 Publish", format!("pub:{}", o.id)),
            InlineKeyboardButton::callback("🗑 Delete", format!("del:{}", o.id)),
        ]);
    }
    let mut nav = Vec::new();
    if page > 0 {
        nav.push(InlineKeyboardButton::callback("◀ Prev", format!("page:{}", page - 1)));
    }
    if page + 1 < pages {
        nav.push(InlineKeyboardButton::callback("Next ▶", format!("page:{}", page + 1)));
    }
    if !nav.is_empty() {
        rows.push(nav);
    }
    let kb = InlineKeyboardMarkup::new(rows);
    let text = format!("Files (page {}/{}, {} total):", page + 1, pages, objs.len());
    if let Some(mid) = edit {
        bot.edit_message_text(chat, mid, &text).reply_markup(kb).await?;
    } else {
        bot.send_message(chat, text).reply_markup(kb).await?;
    }
    Ok(())
}

fn extract_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.starts_with("http://") || w.starts_with("https://"))
        .and_then(|w| url::Url::parse(w).ok().map(|u| u.to_string()))
}

fn filename_from_url(url_str: &str, headers: &reqwest::header::HeaderMap) -> String {
    if let Some(cd) = headers.get(reqwest::header::CONTENT_DISPOSITION) {
        if let Ok(s) = cd.to_str() {
            if let Some(idx) = s.find("filename=") {
                let rest = &s[idx + 9..];
                let name = rest.trim_matches('"').split(';').next().unwrap_or("").trim();
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }
    if let Ok(u) = url::Url::parse(url_str) {
        if let Some(seg) = u.path_segments().and_then(|s| s.last()) {
            if !seg.is_empty() {
                return urlencoding::decode(seg)
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| seg.to_string());
            }
        }
    }
    "download.bin".to_string()
}

fn human_size(b: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = b as f64;
    let mut idx = 0;
    while size >= 1024.0 && idx < UNITS.len() - 1 {
        size /= 1024.0;
        idx += 1;
    }
    format!("{size:.1} {}", UNITS[idx])
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
