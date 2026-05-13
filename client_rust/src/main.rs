use client_rust::HamrahClient;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let phone = env::var("HAMRAH_PHONE").expect("HAMRAH_PHONE env var not set");
    let password = env::var("HAMRAH_PASSWORD").expect("HAMRAH_PASSWORD env var not set");
    let proxy = env::var("HAMRAH_PROXY").unwrap_or_else(|_| "http://127.0.0.1:8888".to_string());

    let mut client = HamrahClient::new(&proxy);

    println!("Logging in...");
    client.login(&phone, &password).await?;

    // Example: List objects
    println!("Listing objects...");
    let objects = client.list_objects().await?;
    println!("Found {} objects.", objects.len());

    // End-to-end test flow (using a temporary test file)
    let test_file_name = "rust_test_file.txt";
    std::fs::write(test_file_name, "Hello from the scrubbed Rust client!")?;

    println!("Uploading test file...");
    client.upload_file(test_file_name).await?;

    let objects = client.list_objects().await?;
    let test_obj = objects.iter().find(|o| o.name == test_file_name)
        .ok_or("Uploaded file not found")?;
    
    println!("Creating public link...");
    let link_data = client.create_public_link(test_obj.id, 3600, 5).await?;
    println!("Public Link: {}", link_data.link);

    // --- TEST: CONTACTS & SHARING ---
    println!("\n--- TEST: CONTACTS & SHARING ---");
    let contact_name = "Test Contact";
    let contact_phone = "0912XXXXXXX";

    println!("Adding contact {}...", contact_name);
    // Note: This might fail if already exists, so we wrap it
    let _ = client.add_contact(contact_name, contact_phone).await;

    println!("Listing contacts...");
    let contacts = client.list_contacts().await?;
    if let Some(test) = contacts.iter().find(|c| c.name == contact_name) {
        println!("Found Test Contact with User ID: {}", test.user_id);
        
        println!("Sharing file with Test Contact (Read-only)...");
        client.share_file(test_obj.id, vec![
            client_rust::SharePermission {
                access: 1,
                user: test.user_id
            }
        ]).await?;
        println!("Shared successfully!");
    }

    println!("\nCleaning up...");
    client.delete_link(link_data.id).await?;
    client.delete_file(test_obj.id).await?;

    println!("Test completed successfully!");
    Ok(())
}
