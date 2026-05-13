const HamrahClient = require('./HamrahClient');
const fs = require('fs');

(async () => {
  const testFileName = 'test_upload.txt';
  fs.writeFileSync(testFileName, 'Scrubbed test content');

  const client = new HamrahClient(process.env.HAMRAH_PROXY || 'http://127.0.0.1:8888', 'auth.json');
  
  try {
    await client.init();

    // Use environment variables for login
    if (process.env.HAMRAH_PHONE && process.env.HAMRAH_PASSWORD) {
        await client.login(process.env.HAMRAH_PHONE, process.env.HAMRAH_PASSWORD);
    }

    console.log('\n--- UPLOADING ---');
    await client.uploadFile(testFileName);

    console.log('\n--- PUBLISHING ---');
    const linkData = await client.publishFile(testFileName, 14400, 5);
    console.log(`Link: ${linkData.link}`);

    console.log('\n--- CLEANUP ---');
    await client.deleteLink(linkData.id);
    await client.removeFile(testFileName);

    console.log('\nSuccess!');
  } catch (error) {
    console.error('\nError:', error);
  } finally {
    await client.close();
  }
})();
