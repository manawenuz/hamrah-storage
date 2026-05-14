# PRD: Optimizing S3 Backend with Native Hamrah Verbs

## 1. Overview
The current S3 implementation in `hamrah-storage` is highly functional but relies on generic file operations. By leveraging newly discovered native Hamrah API endpoints for Rename, Copy, and Move, we can significantly improve performance and reduce network overhead.

## 2. Problem Statement
- **Rename/Move:** Currently, S3 clients often perform Rename as a `COPY` followed by a `DELETE`. If `COPY` is not natively supported, this might fall back to a full download and re-upload, which is slow and expensive.
- **Copy:** Without a native `CopyObject` implementation, server-side copies are not possible, leading to inefficient client-side data transfer.

## 3. Goals
- Implement native `CopyObject` support in the S3 proxy.
- Ensure `MOVE` and `RENAME` operations are "instant" by using the native Hamrah endpoints.
- Maintain full compatibility with `s3s` and existing S3 clients (mc, rclone, rustic).

## 4. Requirements

### 4.1 Native CopyObject
- Map S3 `CopyObject` requests to `POST /api/v5/rgw/copy-object/`.
- Correctly handle the `x-amz-copy-source` header to extract the source bucket and key.
- Target parent ID should be mapped based on the bucket/prefix.

### 4.2 Native Move/Rename Detection
- While S3 doesn't have a `MOVE` verb, some clients (like `mc mv`) use a combination of `CopyObject` + `DeleteObject`.
- We should investigate if we can "fuse" these operations or simply ensure the `CopyObject` part is native and fast.

### 4.3 Folder Support
- Implement `PUT` for directory markers (keys ending in `/`) using the native `create-folder` API if useful, or continue using virtual markers if parity with S3's flat namespace is preferred.
- **Decision:** Stick to virtual markers for S3 parity, but use native `create-folder` for WebDAV collections.

## 5. Technical Changes
- **`HamrahClient`:** (Done) Added `rename_object`, `copy_object`, `move_object`, and `create_folder`.
- **`HamrahS3Backend`:** Implemented the `copy_object` method in the `S3` trait.

## 6. Implementation Details
- **`s3_backend.rs`:** Added `async fn copy_object(&self, req: S3Request<CopyObjectInput>) -> S3Result<S3Response<CopyObjectOutput>>`.
  - Uses `s3s::dto::CopySource` which is already parsed from the `x-amz-copy-source` header.
  - Supports `CopySource::Bucket { bucket, key, .. }`; `AccessPoint` variants return `NotImplemented`.
  - Cross-bucket copy returns `NotImplemented` (each bucket maps to a distinct Hamrah account with separate auth).
  - Source object ID is resolved via the cached listing (`list_cached` + `find_by_key`).
  - Destination name is encoded via the shared `encode_key` helper (same `%2F` / `%25` scheme as `put_object`).
  - After a successful copy, the bucket cache is invalidated.
  - Returns `CopyObjectResult` populated with the source object's `e_tag` and `last_modified`.

## 7. Implementation Plan
- [x] Implement `copy_object` in `s3_backend.rs`.
- [x] Extract source bucket/key from `x-amz-copy-source`.
- [x] Map S3 keys to Hamrah IDs via the cached listing.
- [x] Test with `mc cp` — verified 1s native copy vs full download/upload.

## 8. Success Metrics
- `mc cp s3/bucket/file s3/bucket/copy` should complete in < 500ms (native API call) instead of seconds (download/upload).
- Reduced load on the proxy and Hamrah's upload/download endpoints.
