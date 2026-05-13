# HamrahStorage Reverse Engineered API Documentation

This document outlines the internal HTTP API used by the **HamrahStorage** (`abrehamrahi.ir`) web dashboard. By implementing these endpoints directly, you can build native applications (in Rust, Python, Go, etc.) without needing a headless browser.

---

## 1. Authentication
Authentication is done via phone number and password. It returns a JWT token which must be sent as a Bearer token in subsequent requests.

### Login
- **Endpoint:** `POST https://abrehamrahi.ir/api/v6/profile/auth/login/`
- **Headers:** 
  - `Content-Type: application/json`
  - `User-Agent: Mozilla/5.0 ...` (Standard Chrome User-Agent is mandatory)
  - `Referer: https://abrehamrahi.ir/auth/login`
  - `Origin: https://abrehamrahi.ir`
- **Payload:**
  ```json
  {
    "phone": "912XXXXXXX", 
    "prefix": "+98",
    "country": "IR",
    "password": "YourPassword"
  }
  ```
- **Notes:** 
  - The `phone` should not have the leading zero.
  - **IMPORTANT:** The server checks headers strictly. Failing to provide a valid Browser User-Agent and Referer may result in a `500 Server Error`.
  - **Response:** The server returns the JWT in the JSON body (`access`) and also sets an `access_token` cookie.

### Subsequent Requests
All API requests (except login) require the JWT Bearer Token:
- **Header:** `Authorization: Bearer <your_access_token>`

---

## 2. File & Directory Management

### List Objects
Retrieves the files and directories inside your drive.
- **Endpoint:** `GET https://abrehamrahi.ir/api/v2/flat/list-objects/?is_trash=false&limit=1000`
- **Response:** A JSON object with a `results` array. Each entry includes:
  - `id` — internal object ID (required for delete/share)
  - `name` — stored filename
  - `size` — file size in bytes
  - `last_modified` — Unix timestamp
  - `type` — MIME type (e.g. `binary/octet-stream`)
  - `download_url` — direct CDN URL for downloading the file (e.g. `https://abrehamrahi.ir/o/...`)

---

## 3. Uploading a File (S3 Multipart Flow)
Uploading files bypassing the UI is a 3-step process.

### Step 3.1: Start Upload
Tells the server to prepare an S3 upload.
- **Endpoint:** `POST https://abrehamrahi.ir/api/v2/flat/start-upload/`
- **Payload:**
  ```json
  {
    "obj_size": 1042  // Total size of the file in bytes
  }
  ```
- **Response:** Returns an `upload_id`, an S3 `key`, and a `signed_urls` array containing the target URL(s) for the binary chunks.

### Step 3.2: Upload Binary Chunk
You must PUT the raw binary data to the **first** URL provided in the `signed_urls` array.
- **Method:** `PUT`
- **URL:** The exact URL from `signed_urls[0]`. It already contains the necessary signatures and query params.
- **Headers:** `Content-Type: application/octet-stream`
- **Body:** Raw binary file bytes.
- **Crucial Step:** When the server responds with HTTP 200, it includes an `ETag` header. **You must save this ETag!**

### Step 3.3: Complete Upload
Finalizes the upload and registers the file in your drive.
- **Endpoint:** `POST https://abrehamrahi.ir/api/v2/flat/complete-upload/`
- **Payload:**
  ```json
  {
    "key": "<key_from_step_1>",
    "name": "your_filename.txt",
    "upload_id": "<upload_id_from_step_1>",
    "parts": [
      {
        "ETag": "\"<etag_from_step_2>\"", // Must include quotes
        "PartNumber": 1,
        "size": 1042
      }
    ],
    "force_overwrite": true
  }
  ```
- **Notes:**
  - Set `force_overwrite: true` when re-uploading a file that may already exist (e.g. content-addressed storage). The server replaces the existing entry.
  - The `name` field in `complete-upload` is the logical filename stored in the listing. For the S3 proxy, this is the percent-encoded S3 key (e.g. `path%2Fto%2Ffile.bin`).

---

## 4. Downloading a File

Use the `download_url` field returned by the List Objects API. This is a direct CDN URL that accepts the Bearer token.

- **Method:** `GET`
- **URL:** Value of `download_url` from the listing (e.g. `https://abrehamrahi.ir/o/<token>/`)
- **Headers:** `Authorization: Bearer <your_access_token>`
- **Response:** Raw file bytes.

---

## 5. Public Link Management (Publishing)

Once a file is uploaded, you can generate a public download link.

### Create Public Link
- **Endpoint:** `POST https://abrehamrahi.ir/api/v2/sharing/public-link/create/`
- **Payload:**
  ```json
  {
    "obj_id": 11269116,          // The file's internal ID
    "duration": 14400,           // Expiry time in seconds (e.g., 4 hours = 14400)
    "expiration_count": 5        // Maximum allowed downloads
  }
  ```
- **Response:** Returns the created link details. The shareable URL is in the `link` field.

### Edit Link Limits
- **Endpoint:** `PATCH https://abrehamrahi.ir/api/v2/sharing/public-link/edit/{link_id}/`
- **Payload:** `{"duration": 14400, "expiration_count": 6}`

### Delete Link
- **Endpoint:** `DELETE https://abrehamrahi.ir/api/v2/sharing/public-link/delete/{link_id}/`

---

## 6. Deleting a File (Move to Trash)
To move a file to the trashcan:
- **Method:** `DELETE`
- **Endpoint:** `https://abrehamrahi.ir/api/v2/rgw/trash-objects/`
- **Payload:** `{"obj_ids": [11269116]}`

---

## 7. Contacts & Private Sharing
You can share files privately with specific users by adding them as contacts.

### Add Contact
- **Endpoint:** `POST https://abrehamrahi.ir/api/v6/profile/contact/create-contact/`
- **Payload:** `{"phone":"912XXXXXXX","name":"Test User","prefix":"+98"}`

### List Contacts
- **Endpoint:** `GET https://abrehamrahi.ir/api/v6/profile/contact/list-contact/?limit=1000`
- **Response:** Returns a list of contacts. Each contact has a `user` field (User ID) which is required for sharing.

### Set File Permissions (Share)
- **Endpoint:** `POST https://abrehamrahi.ir/api/v2/sharing/set-permission/{obj_id}/`
- **Payload:**
  ```json
  [
    {
      "access": 1,      // 1: Read-only, 3: Read/Write
      "user": 123456    // The User ID of the contact
    }
  ]
  ```
