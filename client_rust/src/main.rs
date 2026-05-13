use clap::{Parser, Subcommand};
use client_rust::HamrahClient;
use client_rust::config::AppConfig;
use client_rust::s3_backend::HamrahS3Backend;
use s3s::auth::SimpleAuth;
use s3s::service::S3ServiceBuilder;
use s3s::validation::NameValidation;

struct AnyName;
impl NameValidation for AnyName {
    fn validate_bucket_name(&self, _: &str) -> bool { true }
}
use std::net::SocketAddr;

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
        #[arg(short, long)]
        port: Option<u16>,
        #[arg(short, long)]
        account: Option<String>,
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
    let proxy = config.proxy.as_deref().filter(|s| !s.is_empty());

    match cli.command {
        Commands::S3 { port, account: _ } => {
            let mc = config.mc.as_ref();
            let bind_port = port
                .or_else(|| mc.map(|m| m.port))
                .unwrap_or(8080);

            let mut clients = std::collections::HashMap::new();

            for (name, acc) in &config.accounts {
                println!("Logging in to account: {}...", name);
                let mut client = HamrahClient::new(proxy);
                if let Err(e) = client.login(&acc.phone, &acc.password).await {
                    eprintln!("Failed to login to account {}: {}", name, e);
                    continue;
                }
                clients.insert(name.clone(), client);
                println!("Login successful for account: {}", name);
            }

            if clients.is_empty() {
                return Err("No accounts successfully logged in".into());
            }

            let backend = HamrahS3Backend::new(clients);
            let mut builder = S3ServiceBuilder::new(backend);
            builder.set_validation(AnyName);
            if let Some(mc) = mc {
                builder.set_auth(SimpleAuth::from_single(&mc.access_key, mc.secret_key.as_str()));
            }
            let service = builder.build();

            let addr = SocketAddr::from(([127, 0, 0, 1], bind_port));
            let listener = tokio::net::TcpListener::bind(addr).await?;
            println!("Starting S3-compatible server on http://{}", addr);
            println!("Buckets available: {:?}", config.accounts.keys().collect::<Vec<_>>());
            if let Some(mc) = mc {
                println!("mc alias:  {}", mc.alias_cmd("hamrah"));
            }
            
            loop {
                let (stream, _) = listener.accept().await?;
                let service = service.clone();
                tokio::spawn(async move {
                    let conn = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service);
                    if let Err(err) = conn.await {
                        eprintln!("Error serving connection: {:?}", err);
                    }
                });
            }
        }
        Commands::List { account } => {
            let acc = config.accounts.get(&account).ok_or("Account not found")?;
            let mut client = HamrahClient::new(proxy);
            client.login(&acc.phone, &acc.password).await?;
            
            let objects = client.list_objects().await?;
            for obj in objects {
                println!("- {} (ID: {})", obj.name, obj.id);
            }
        }
        Commands::Test { account } => {
            let acc = config.accounts.get(&account).ok_or("Account not found")?;
            let mut client = HamrahClient::new(proxy);
            client.login(&acc.phone, &acc.password).await?;

            let test_file = "test_upload.txt";
            std::fs::write(test_file, "Client test content")?;
            println!("Uploading test file...");
            client.upload_file(test_file).await?;
            
            println!("Done!");
        }
    }

    Ok(())
}
