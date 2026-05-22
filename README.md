# HamrahStorage Rust Client

A native Rust CLI and S3-compatible proxy for **HamrahStorage** (`abrehamrahi.ir`). Exposes your cloud storage as a local S3 endpoint so standard backup tools like **rustic**, **restic**, and **rclone** work against it directly.

## Features

- **S3-Compatible Proxy** — full S3 API: list (v1+v2), get (with byte-range), put, delete, multipart upload, head, bucket ops
- **rustic / restic compatible** — backup, restore, and snapshot lifecycle fully tested end-to-end
- **rclone compatible** — all rclone operations work (`copy`, `sync`, `cat`, etc.)
- **mc compatible** — all MinIO client operations work (`ls`, `stat`, `cp`, `rm`, `mirror`)
- **Multi-Account** — each Hamrah account maps to a separate S3 bucket
- **Session Caching** — tokens persisted to disk and reused across restarts to avoid re-login
- **Auto Token Refresh** — detects 401/503 responses during upload and re-authenticates automatically
- **Path Encoding** — arbitrary S3 keys (including `/`) mapped to flat Hamrah filenames via `%2F`/`%25` encoding
- **Proxy Support** — optional HTTP proxy for all outbound Hamrah API traffic

---

## Installation

### Pre-built binaries
Download the latest binary for your platform from [GitHub Releases](https://github.com/manawenuz/hamrah-storage/releases).

Platforms: Linux x86_64, Windows x86_64, macOS (Apple Silicon + Intel).

### Build from source
```bash
git clone https://github.com/manawenuz/hamrah-storage.git
cd hamrah-storage/client_rust
cargo build --release
# Binary: ./target/release/client_rust
```

---

## Configuration

Create a `config.yaml` (environment variables are expanded automatically):

```yaml
accounts:
  personal:
    phone: "${HAMRAH_PHONE}"       # e.g. 09123456789
    password: "${HAMRAH_PASSWORD}"
  backup:
    phone: "09350000000"
    password: "AnotherPassword"

proxy: "${HAMRAH_PROXY}"           # optional, e.g. http://127.0.0.1:8888

mc:                                # optional — printed as alias command on startup
  host: "127.0.0.1"
  port: 8080
  bucket: "personal"
  access_key: "any"
  secret_key: "any"
```

Phone numbers are normalised automatically — leading `0`, `+98`, or `98` prefix all work.

---

## Usage

### Start the S3 proxy

```bash
export HAMRAH_PHONE=09123456789
export HAMRAH_PASSWORD=YourPassword
./client_rust --config config.yaml s3 --port 8080
```

Each account in `config.yaml` becomes a separate S3 bucket. The server prints the `mc alias` command on startup.

### List files

```bash
./client_rust --config config.yaml list --account personal
```

---

## Integration with Backup Tools

### rclone

Add a remote in `~/.config/rclone/rclone.conf`:

```ini
[hamrah]
type = s3
provider = Other
access_key_id = any
secret_access_key = any
endpoint = http://127.0.0.1:8080
region = us-east-1
```

```bash
rclone ls hamrah:personal
rclone copy /local/path hamrah:personal/backup/
```

### rustic

rustic uses rclone as its S3 transport layer:

```bash
# Initialize a repository
RUSTIC_REPOSITORY="rclone:hamrah:personal/my-repo" \
RUSTIC_PASSWORD="repopassword" \
  rustic init

# Back up a directory
RUSTIC_REPOSITORY="rclone:hamrah:personal/my-repo" \
RUSTIC_PASSWORD="repopassword" \
  rustic backup /path/to/data

# Restore latest snapshot
RUSTIC_REPOSITORY="rclone:hamrah:personal/my-repo" \
RUSTIC_PASSWORD="repopassword" \
  rustic restore latest /path/to/restore

# Keep only the 5 most recent snapshots and clean up unreferenced data
RUSTIC_REPOSITORY="rclone:hamrah:personal/my-repo" \
RUSTIC_PASSWORD="repopassword" \
  rustic forget --keep-last 5 --prune
```

### Duplicati (Windows)

See [docs/WINDOWS_DUPLICATI_SETUP.md](docs/WINDOWS_DUPLICATI_SETUP.md) for a step-by-step guide to backing up a Windows machine to HamrahStorage using Duplicati.

### mc (MinIO Client)

```bash
mc alias set hamrah http://127.0.0.1:8080 any any
mc ls hamrah/personal
mc cp localfile.txt hamrah/personal/path/to/file.txt
mc rm hamrah/personal/path/to/file.txt
mc mirror /local/dir hamrah/personal/backup/
```

---

## S3 API Coverage

| Operation | Status |
|---|---|
| ListObjectsV2 (with prefix/delimiter filtering) | ✅ |
| ListObjects v1 | ✅ |
| HeadObject | ✅ |
| GetObject (including byte-range requests) | ✅ |
| PutObject | ✅ |
| DeleteObject | ✅ |
| DeleteObjects (bulk) | ✅ |
| CreateMultipartUpload / UploadPart / CompleteMultipartUpload | ✅ |
| AbortMultipartUpload | ✅ |
| HeadBucket | ✅ |
| GetBucketLocation | ✅ |
| CreateBucket (no-op for mapped accounts) | ✅ |

---

## Privacy & Security

- No headless browser — direct HTTP API calls only.
- Credentials are never logged.
- Open source — verify the code yourself.

## License
MIT. See [LICENSE](LICENSE).
