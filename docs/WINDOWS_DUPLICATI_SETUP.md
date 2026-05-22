# Windows + Duplicati Setup Guide

This guide walks you through backing up files from a Windows machine to **HamrahStorage** using **Duplicati**, with the HamrahStorage Rust client acting as a local S3-compatible proxy.

```
[ Duplicati ]  -->  [ client_rust (S3 proxy, localhost:8080) ]  -->  [ abrehamrahi.ir ]
```

---

## 1. Prerequisites

- Windows 10 or 11 (x86_64)
- A HamrahStorage account (phone number + password)
- Administrator access (only for installing Duplicati)

---

## 2. Download the HamrahStorage client

1. Open <https://github.com/manawenuz/hamrah-storage/releases>.
2. Download the latest **Windows x86_64** binary (e.g. `client_rust-x86_64-pc-windows-msvc.zip`).
3. Extract it to a folder you control, for example `C:\HamrahStorage\`.
   You should end up with `C:\HamrahStorage\client_rust.exe`.

---

## 3. Create the config file

In `C:\HamrahStorage\`, create a new text file called `config.yaml` with the following content (replace the phone number and password with your own):

```yaml
accounts:
  personal:
    phone: "09123456789"
    password: "YourHamrahPassword"

mc:
  host: "127.0.0.1"
  port: 8080
  bucket: "personal"
  access_key: "any"
  secret_key: "any"
```

Notes:
- The `accounts` key name (`personal` above) becomes your **S3 bucket name**. You can rename it or add more accounts.
- `access_key` and `secret_key` can be any non-empty string — the proxy does not validate them.

> ⚠️ Use a plain text editor like Notepad or VS Code. Word will corrupt the file.

---

## 4. Start the S3 proxy

Open **PowerShell** (or Command Prompt) and run:

```powershell
cd C:\HamrahStorage
.\client_rust.exe --config config.yaml s3 --port 8080
```

You should see output similar to:

```
S3 proxy listening on 0.0.0.0:8080
mc alias set hamrah http://127.0.0.1:8080 any any
```

**Leave this window open while backups run.** Closing it stops the proxy.

### Optional: run it automatically at boot

Create a shortcut to `client_rust.exe` with the arguments `--config config.yaml s3 --port 8080` and place it in:

```
C:\Users\<YourUser>\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup
```

Or use Task Scheduler with trigger "At log on" and action "Start a program" pointing at `client_rust.exe`.

---

## 5. Install Duplicati

1. Download Duplicati from <https://duplicati.com/download>.
2. Pick the **stable Windows** installer (`.msi`).
3. Install with defaults. Duplicati runs as a tray icon and opens its UI in your browser at <http://localhost:8200>.

---

## 6. Create a backup job in Duplicati

1. Open the Duplicati web UI: <http://localhost:8200>.
2. Click **Add backup → Configure a new backup → Next**.

### Step 1 — General

- **Name**: `HamrahStorage Backup` (anything you want)
- **Encryption**: AES-256 (recommended)
- **Passphrase**: choose a strong passphrase and **save it somewhere safe** — without it, your backups cannot be restored.

### Step 2 — Destination

- **Storage Type**: `S3 Compatible`
- **Use SSL**: ❌ unchecked (the proxy runs over plain HTTP on localhost)
- **Server**: `Custom server url`
- **Custom server url**: `127.0.0.1:8080`
- **Bucket name**: `personal` (must match the account name in `config.yaml`)
- **Bucket create region**: leave default (`us-east-1` or "Any")
- **Storage class**: leave default
- **Folder path**: `duplicati/` (or any prefix you like — keeps things tidy if you add more backups later)
- **AWS Access ID**: `any`
- **AWS Access Key**: `any`

Click **Test connection**. If the proxy is running, you should see a success message. If Duplicati asks about creating the bucket, click **Yes**.

### Step 3 — Source Data

Tick the folders you want to back up (e.g. `C:\Users\<YourUser>\Documents`).

### Step 4 — Schedule

Pick a schedule, e.g. daily at 02:00, or set to "Run at most once a day".

### Step 5 — Options

- **Upload volume size**: `50 MB` (default is fine; smaller volumes are friendlier to flaky connections)
- **Backup retention**: pick a policy, e.g. "Smart backup retention".

Click **Save**.

---

## 7. Run your first backup

1. From the Duplicati home page, click **Run now** on the job.
2. The first run will be slow (it uploads everything). Subsequent runs only upload changes.
3. The PowerShell window running `client_rust.exe` will show upload activity.

---

## 8. Restore a file

1. In Duplicati, click **Restore** → pick your backup job.
2. Browse the snapshot tree, select the files/folders, choose a restore location, and confirm.
3. You'll need the encryption passphrase from Step 6.

---

## Troubleshooting

**"Test connection" fails in Duplicati**
- Make sure the PowerShell window with `client_rust.exe` is still running.
- Confirm `Custom server url` is exactly `127.0.0.1:8080` (no `http://`, no trailing slash).
- Make sure **Use SSL** is unchecked.

**`client_rust.exe` exits immediately**
- Check `config.yaml` is valid YAML (indentation matters — use spaces, not tabs).
- Verify the phone number and password by logging into the HamrahStorage app/website.

**Uploads stall or fail with 401/503**
- The client auto-refreshes its session. If problems persist, delete the `.session_*` file next to `client_rust.exe` and restart it.

**Windows Defender / SmartScreen warning**
- The binary is unsigned. Click **More info → Run anyway**, or build from source (see main README).

**Slow uploads**
- HamrahStorage rate-limits per account. For large datasets, leave the first backup running overnight.

---

## Tips

- Keep `config.yaml` and your Duplicati passphrase backed up **outside** the same backup — otherwise you cannot restore.
- You can add a second account in `config.yaml` (e.g. `backup:`) and point a second Duplicati job at bucket `backup` to spread data across accounts.
- To stop the proxy cleanly: focus the PowerShell window and press **Ctrl+C**.

---

## Useful links

- HamrahStorage client repo: <https://github.com/manawenuz/hamrah-storage>
- Duplicati documentation: <https://docs.duplicati.com/>
- Main README (CLI usage, rclone/restic): [../README.md](../README.md)
