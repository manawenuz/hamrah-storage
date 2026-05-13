# HamrahStorage Rust Client 🚀

A high-performance, native Rust CLI and S3-compatible proxy for **HamrahStorage** (`abrehamrahi.ir`). This tool allows you to bypass the web interface and integrate your cloud storage directly into professional backup workflows like **rustic** and **duplicati**.

## ✨ Features

- **Blazing Fast**: Native Rust implementation with async I/O.
- **S3-Compatible**: Expose your storage as an S3 endpoint for seamless integration.
- **Multi-Account**: Manage multiple HamrahStorage accounts in one place.
- **Full Lifecycle**: Upload, Download, List, Delete, and Share files.
- **Private Sharing**: Add contacts and manage granular file permissions.
- **Cross-Platform**: Binaries available for Linux, Windows, and macOS.

---

## 📦 Installation

### Download Releases
Fetch the latest pre-compiled binary for your platform from the [GitHub Releases](https://github.com/manawenuz/hamrah-storage/releases).

### Compiling from Source
If you have the Rust toolchain installed:
```bash
git clone https://github.com/manawenuz/hamrah-storage.git
cd hamrah-storage/client_rust
cargo build --release
```
The binary will be located at `./target/release/client_rust`.

---

## ⚙️ Configuration

Create a `config.yaml` file to manage your accounts and settings:

```yaml
accounts:
  my_personal:
    phone: "${HAMRAH_PHONE}" # Automatically expanded from environment variables
    password: "${HAMRAH_PASSWORD}"
  backup_account:
    phone: "935XXXXXXX"
    password: "AnotherPassword"

# proxy: "${HAMRAH_PROXY}" # Optional proxy
s3_port: 8080
```

---

## 🚀 Usage

### 1. Listing Files
See what's in your drive:
```bash
./client_rust --config config.yaml list --account my_personal
```

### 2. Testing Upload
Upload a test file to verify connectivity:
```bash
./client_rust --config config.yaml test --account my_personal
```

### 3. Sharing & Contacts
The client supports the full reverse-engineered sharing API:
- Create public links with expiration and download limits.
- Add contacts and set private "Read" or "Read/Write" permissions.
*(Refer to the CLI help `./client_rust --help` for detailed command flags)*

---

## 🛠️ Integration with Backup Tools (S3 Mode)

The most powerful feature of this client is providing an **S3-compatible endpoint**. This allows you to use HamrahStorage as a backend for tools that support S3.

### Step 1: Start the S3 Proxy
```bash
./client_rust --config config.yaml s3 --account my_personal --port 8080
```

### Step 2: Configure your Backup Tool

In S3 mode, each account defined in your `config.yaml` is exposed as a **separate bucket**. Changing the bucket name in your backup tool automatically switches the target HamrahStorage account.

#### For **rustic**:
```bash
# Initialize account 'my_personal'
rustic -r s3:http://localhost:8080/my_personal init

# Initialize account 'backup_account'
rustic -r s3:http://localhost:8080/backup_account init

# Perform a backup to 'my_personal'
rustic -r s3:http://localhost:8080/my_personal backup /path/to/data
```

#### For **duplicati**:
1. Select **S3 Compatible** as the storage type.
2. **Server**: `Custom server URL` -> `http://localhost:8080`
3. **Bucket Name**: Set this to your account name from `config.yaml` (e.g., `my_personal` or `backup_account`).
4. **AWS Access Key / Secret Key**: Use any placeholder values (the proxy handles real auth via its own config).

---

## 🛡️ Privacy & Security

- **Direct API**: No headless browser or Playwright overhead.
- **Scrubbed Logs**: The client does not log sensitive personal information.
- **Open Source**: Verify the code yourself to ensure your credentials are safe.

## 📄 License
MIT License. See [LICENSE](LICENSE) for details.
