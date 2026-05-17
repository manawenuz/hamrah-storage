# HamrahStorage Telegram Bot

Simple Telegram bot that uploads files into a HamrahStorage account, manages them, and produces public links on demand. Single Hamrah account, 1–5 Telegram admins.

## Quick start

```bash
cp .env.example .env
# fill in TELOXIDE_TOKEN, HAMRAH_PHONE, HAMRAH_PASSWORD, ADMIN_IDS
docker compose up --build -d
docker compose logs -f
```

`ADMIN_IDS` is a comma-separated list of numeric Telegram user ids. Use `@userinfobot` on Telegram to look yours up. Anything from non-admins is silently ignored.

## Commands

- `/help` — list commands
- `/list` — list files (id, name, size)
- `/manage` — inline UI with Publish/Delete buttons per file, paginated
- `/publish <id>` — create a public link (24 h, 100 views)
- `/delete <id>` — move file to trash
- `/whoami` — show your Telegram user id

Sending any file / photo / video / voice / audio uploads it. Sending an `https://` URL fetches it and uploads the result.

## Notes

- Telegram Bot API caps file *downloads* at ~20 MB. Larger files sent to the bot will fail; URL ingest is not affected.
- Hamrah session tokens are persisted under `./data/.session_<phone>` so the bot reuses the session across restarts.
- Built directly on the `client_rust` library in this repo — no S3 layer in between.
- Set `TELEGRAM_PROXY` (e.g. `socks5h://user:pass@host:1080` or `http://host:8080`) if `api.telegram.org` is blocked from your network. `HAMRAH_PROXY` is independent.
