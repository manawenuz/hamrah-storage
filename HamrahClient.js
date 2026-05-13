const { chromium } = require('playwright');
const fs = require('fs');

class HamrahClient {
  constructor(proxyServer = 'http://127.0.0.1:8888', authFile = 'auth.json') {
    this.proxyServer = proxyServer;
    this.authFile = authFile;
    this.browser = null;
    this.context = null;
    this.page = null;
  }

  async init() {
    console.log('Initializing headless browser...');
    this.browser = await chromium.launch({
      headless: true,
      proxy: { server: this.proxyServer }
    });

    // Load auth state if it exists
    const storageState = fs.existsSync(this.authFile) ? this.authFile : undefined;
    this.context = await this.browser.newContext({ storageState });
    this.page = await this.context.newPage();
  }

  // Extracts the JWT Bearer token from the saved cookies to use in raw API requests
  async getToken() {
    const state = await this.context.storageState();
    const authCookie = state.cookies.find(c => c.name === 'ABREHAMRAHI_AUTH_TOKEN');
    if (!authCookie) throw new Error("Not logged in. No auth token found.");
    const decoded = decodeURIComponent(authCookie.value);
    const parsed = JSON.parse(decoded);
    return parsed.access;
  }

  // --- 1. LOGIN ---
  async login(phone, password) {
    if (!this.page) throw new Error("Client not initialized.");
    console.log(`Logging in via API for ${phone}...`);
    
    const p = phone.startsWith('0') ? phone.substring(1) : phone;
    const response = await this.page.request.post('https://abrehamrahi.ir/api/v6/profile/auth/login/', {
      data: { phone: p, prefix: '+98', country: 'IR', password }
    });

    if (!response.ok()) {
      throw new Error(`Login failed! Server responded with: ${await response.text()}`);
    }

    // Playwright automatically stores the Set-Cookie headers! Save them to auth.json.
    await this.context.storageState({ path: this.authFile });
    console.log('Login successful! Session saved.');
  }

  // --- UI NAVIGATION ---
  async goToMyFiles() {
    await this.page.goto('https://abrehamrahi.ir/drive', { waitUntil: 'domcontentloaded' });
    await this.page.waitForTimeout(2000);
    const myFilesBtn = this.page.locator('text=فایل‌های من').first();
    if (await myFilesBtn.isVisible()) {
      await myFilesBtn.click();
      await this.page.waitForTimeout(2000);
    }
  }

  // --- Helper: Get Object ID from UI ---
  async getObjId(fileName) {
    await this.goToMyFiles();
    const fileLocator = this.page.locator(`div[title="${fileName}"]`).first();
    await fileLocator.waitFor({ state: 'visible', timeout: 15000 });
    const objId = await fileLocator.getAttribute('data-resource-id');
    return parseInt(objId);
  }

  // --- 2. UPLOAD FILE ---
  async uploadFile(filePath) {
    if (!this.page) throw new Error("Client not initialized.");
    await this.goToMyFiles();
    
    console.log(`Uploading: ${filePath}`);
    await this.page.setInputFiles('input[type="file"]', filePath);
    
    const fileName = filePath.split('/').pop().split('\\').pop();
    console.log(`Waiting for ${fileName} to finish uploading...`);
    const fileLocator = this.page.locator(`div[title="${fileName}"]`).first();
    await fileLocator.waitFor({ state: 'visible', timeout: 60000 });
    console.log(`Upload complete: ${fileName}`);
  }

  // --- 3. PUBLISH FILE (Create Link with expiry and limit) ---
  async publishFile(fileName, durationSeconds = 14400, limit = 5) {
    console.log(`Publishing ${fileName} (Duration: ${durationSeconds}s, Limit: ${limit})...`);
    
    // We use the UI to get the internal Object ID, then hit the API directly for maximum reliability
    const objId = await this.getObjId(fileName);
    const token = await this.getToken();

    const response = await this.page.request.post('https://abrehamrahi.ir/api/v2/sharing/public-link/create/', {
      headers: { Authorization: `Bearer ${token}` },
      data: { obj_id: objId, duration: durationSeconds, expiration_count: limit }
    });

    if (!response.ok()) throw new Error(`Publish failed: ${await response.text()}`);
    const data = await response.json();
    console.log(`Successfully published! Link ID: ${data.id}`);
    return data; // contains link details
  }

  // --- 4. LINK MANAGEMENT (Edit limit/expiry) ---
  async updateLink(linkId, durationSeconds = 14400, limit = 6) {
    console.log(`Updating Link ${linkId} (Duration: ${durationSeconds}s, Limit: ${limit})...`);
    const token = await this.getToken();

    const response = await this.page.request.patch(`https://abrehamrahi.ir/api/v2/sharing/public-link/edit/${linkId}/`, {
      headers: { Authorization: `Bearer ${token}` },
      data: { duration: durationSeconds, expiration_count: limit }
    });

    if (!response.ok()) throw new Error(`Update Link failed: ${await response.text()}`);
    console.log(`Successfully updated link ${linkId}!`);
    return await response.json();
  }

  // --- 5. DELETE LINK ---
  async deleteLink(linkId) {
    console.log(`Deleting Link ${linkId}...`);
    const token = await this.getToken();

    const response = await this.page.request.delete(`https://abrehamrahi.ir/api/v2/sharing/public-link/delete/${linkId}/`, {
      headers: { Authorization: `Bearer ${token}` }
    });

    if (!response.ok()) throw new Error(`Delete Link failed: ${await response.text()}`);
    console.log(`Successfully deleted link ${linkId}!`);
  }

  // --- 6. REMOVE FILE (API Version) ---
  async removeFile(fileName) {
    if (!this.page) throw new Error("Client not initialized.");
    const objId = await this.getObjId(fileName);
    const token = await this.getToken();

    console.log(`Moving ${fileName} (ID: ${objId}) to trash via API...`);
    const response = await this.page.request.delete('https://abrehamrahi.ir/api/v2/rgw/trash-objects/', {
      headers: { Authorization: `Bearer ${token}` },
      data: { obj_ids: [objId] }
    });

    if (!response.ok()) throw new Error(`Delete failed: ${await response.text()}`);
    console.log(`Successfully moved ${fileName} to trash.`);
  }

  async close() {
    if (this.browser) await this.browser.close();
  }
}

module.exports = HamrahClient;
