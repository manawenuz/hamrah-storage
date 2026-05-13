use clap::{Parser, Subcommand};
use client_rust::HamrahClient;
use client_rust::config::AppConfig;
use client_rust::s3_backend::HamrahS3Backend;
use s3_server::S3ServiceBuilder;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, default_value = "config.yaml")]
    config: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an S3-compatible server
    S3 {
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
        #[arg(short, long)]
        account: String,
    },
    /// List objects in an account
    List {
        #[arg(short, long)]
        account: String,
    },
    /// Test upload/delete flow
    Test {
        #[arg(short, long)]
        account: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let cli = Cli::parse();
    
    let config = AppConfig::from_file(&cli.config)?;
    let proxy = config.proxy.clone().unwrap_or_else(|| "http://127.0.0.1:8888".to_string());

    match cli.command {
        Commands::S3 { port, account } => {
            let acc = config.accounts.get(&account).ok_or("Account not found")?;
            let mut client = HamrahClient::new(&proxy);
            client.login(&acc.phone, &acc.password).await?;
            
            let backend = HamrahS3Backend::new(client);
            let mut service = S3ServiceBuilder::new(Arc::new(backend));
            service.set_base_domain(None);
            let service = service.build();

            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            println!("Starting S3-compatible server on http://{}", addr);
            
            // Note: In a real implementation, we would use hyper to serve the service.
            // This is a placeholder for the actual server loop.
            println!("S3 server is ready (integration in progress)...");
            // hyper::server::conn::http1::Builder::new().serve_connection(...)
            tokio::signal::ctrl_c().await?;
        }
        Commands::List { account } => {
            let acc = config.accounts.get(&account).ok_or("Account not found")?;
            let mut client = HamrahClient::new(&proxy);
            client.login(&acc.phone, &acc.password).await?;
            
            let objects = client.list_objects().await?;
            for obj in objects {
                println!("- {} (ID: {})", obj.name, obj.id);
            }
        }
        Commands::Test { account } => {
            let acc = config.accounts.get(&account).ok_or("Account not found")?;
            let mut client = HamrahClient::new(&proxy);
            client.login(&acc.phone, &acc.password).await?;

            let test_file = "test_upload.txt";
            std::fs::write(test_file, "S3 compatibility test")?;
            println!("Uploading test file...");
            client.upload_file(test_file).await?;
            
            println!("Done!");
        }
    }

    Ok(())
}
