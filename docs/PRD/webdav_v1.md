# PRD: WebDAV Class 1 Server for HamrahStorage

## 1. Overview
The goal is to add a WebDAV Class 1 (RFC 4918) server to the `hamrah-storage` proxy. This allows users to mount their Hamrah cloud storage as a native network drive on macOS, Windows, and Linux without third-party sync clients.

## 2. Goals
- Native filesystem mounting support.
- Support for file Rename/Move without re-uploading (via discovered `/api/v2/rgw/move-object/`).
- Seamless integration with mobile file managers.
- Shared logic with the existing S3 backend.

## 3. Scope (Class 1)
| Verb | Implementation Strategy | Status |
|---|---|---|
| `OPTIONS` | Return standard WebDAV headers (Class 1). | ✅ Done |
| `GET` / `HEAD` | Stream via `download_url` (re-use S3 logic). | ✅ Done |
| `PUT` | Multipart upload (re-use S3 logic). | ✅ Done |
| `DELETE` | Move to trash (re-use S3 logic). | ✅ Done |
| `PROPFIND` | XML response. Map Hamrah object fields to `<D:prop>`. | ✅ Done |
| `MKCOL` | No-op (Hamrah uses virtual folders). | ✅ Done |
| `MOVE` | **Native:** Call `rename-object` (flat namespace). | ✅ Done |
| `COPY` | **Native:** Call `copy-object`. | ✅ Done |

## 4. Technical Specifications

### 4.1 Path Semantics
- Use the same `%2F` / `%25` encoding as the S3 backend to ensure compatibility.
- The root of the WebDAV server lists all configured accounts as collections (folders).
- `http://localhost:8081/personal/path/to/file.txt` maps to account `personal` and key `path/to/file.txt`.

### 4.2 Authentication
- **HTTP Basic Auth** is accepted but not enforced in local mode (parity with S3 "any/any").
- Username: Account name from `config.yaml`.
- Password: Not verified in local mode.
- (Future) Support for phone/password login on-the-fly.

### 4.3 XML Serialization (`PROPFIND`)
- XML is generated as a UTF-8 string and returned with `Content-Type: application/xml; charset=utf-8`.
- `quick-xml` is available as a dependency for future refactoring if needed.
- Required properties returned:
    - `getcontentlength`
    - `getlastmodified` (RFC 1123)
    - `getetag`
    - `resourcetype` (`<D:collection/>` for folders)
    - `displayname`

## 5. Architecture

### 5.1 New Module: `webdav_server.rs`
- **`WebDavState`** — wraps `Arc<HashMap<String, Arc<Mutex<HamrahClient>>>>`, shared across all handlers.
- **`DavResource`** — internal struct representing a PROPFIND response entry.
- **Helpers:**
  - `encode_key` / `decode_key` — re-used from `s3_backend.rs` (now `pub`).
  - `encode_href_path` — URL-encodes path segments for XML `href` values.
  - `xml_escape` — basic XML entity escaping.
  - `format_rfc1123` — formats Hamrah timestamps for WebDAV.
  - `parse_path` — splits `/account/path/to/file` into account + subpath.
  - `extract_path_from_destination` — handles absolute and relative `Destination` headers.

### 5.2 Handlers
- **`handle_options`** — returns `DAV: 1` and the full allowed method list.
- **`handle_propfind`** —
  - Root (`/`) returns all configured accounts as collections.
  - Files return a single `<D:response>`.
  - Folders return themselves plus direct children, using the same prefix/delimiter filtering logic as the S3 backend (`/` as delimiter).
  - Respects `Depth: 0` (resource only) and `Depth: 1` (resource + children); defaults to `1`.
- **`handle_get_head`** — resolves the object, downloads the full payload via `download_object`, and returns it (or headers only for `HEAD`).
- **`handle_put`** — collects the request body via `http_body_util::BodyExt::collect`, then calls `upload_bytes`.
- **`handle_delete`** — resolves the object ID and calls `delete_file` (trash).
- **`handle_mkcol`** — returns `201 Created`; Hamrah uses virtual folders.
- **`handle_move`** — parses the `Destination` header, resolves source object ID, and calls `rename_object`.
- **`handle_copy`** — parses the `Destination` header, resolves source object ID, and calls `copy_object`.
- Cross-account MOVE/COPY returns `403 Forbidden`.

## 6. Implementation Milestones

### Phase 1: Read-Only
- [x] Setup `axum` on a separate port.
- [x] Implement `OPTIONS`.
- [x] Implement `PROPFIND` with XML serialization.
- [x] Implement `GET` / `HEAD` — HEAD fixed to use listing metadata, no download.
- [x] Verified: PROPFIND returns correct XML with size, etag, last-modified.

### Phase 2: Read-Write
- [x] Implement `PUT` (Upload).
- [x] Implement `DELETE`.
- [x] Implement `MKCOL` (No-op).
- [x] Verified: PUT returns 201, DELETE returns 204, listing cache invalidated.

### Phase 3: Native Operations
- [x] Implement `MOVE` using native Hamrah API.
- [x] Implement `COPY` using native Hamrah API.
- [x] Verified: COPY completes in ~400ms, MOVE/RENAME in ~1.7s — no re-upload.

### Additional Improvements
- [x] Added 30s listing cache to WebDAV (mirrors S3 backend cache).
- [x] Cache invalidated on PUT, DELETE, MOVE, COPY.

## 7. CLI Integration
Three new/updated commands are available in `main.rs`:
- `cargo run -- S3 --port 8080` — S3 server only.
- `cargo run -- WebDav --port 8081` — WebDAV server only.
- `cargo run -- Serve --s3-port 8080 --webdav-port 8081` — both servers simultaneously.

## 8. Non-Goals
- Class 2 Locks (`LOCK`, `UNLOCK`).
- Custom Properties (`PROPPATCH`).
- Web interface (UI).
