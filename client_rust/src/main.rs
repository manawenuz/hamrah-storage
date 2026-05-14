use clap::{Parser, Subcommand};
use client_rust::HamrahClient;
use client_rust::config::AppConfig;
use client_rust::s3_backend::HamrahS3Backend;
use client_rust::webdav_server::{WebDavState, handle_webdav_request, prewarm_caches};
use s3s::auth::SimpleAuth;
use s3s::service::S3ServiceBuilder;
use s3s::validation::NameValidation;

struct AnyName;
impl NameValidation for AnyName {
    fn validate_bucket_name(&self, _: &str) -> bool { true }
}
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
        #[arg(short, long)]
        port: Option<u16>,
        #[arg(short, long)]
        account: Option<String>,
    },
    /// Start a WebDAV server
    WebDav {
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Start S3 server (optionally also WebDAV with --webdav)
    Serve {
        #[arg(short, long)]
        s3_port: Option<u16>,
        /// Also start WebDAV server (experimental, read-only)
        #[arg(long)]
        webdav: bool,
        #[arg(long)]
        webdav_port: Option<u16>,
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

    async fn login_all_accounts(
        config: &AppConfig,
        proxy: Option<&str>,
    ) -> Result<std::collections::HashMap<String, HamrahClient>, Box<dyn std::error::Error>> {
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
        Ok(clients)
    }

    match cli.command {
        Commands::S3 { port, account: _ } => {
            let mc = config.mc.as_ref();
            let bind_port = port
                .or_else(|| mc.map(|m| m.port))
                .unwrap_or(8080);

            let clients = login_all_accounts(&config, proxy).await?;

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
        Commands::WebDav { port } => {
            let bind_port = port.unwrap_or(8081);
            let clients = login_all_accounts(&config, proxy).await?;

            let webdav_state = Arc::new(WebDavState::new(clients));
            prewarm_caches(webdav_state.clone()).await;
            let app = axum::Router::new()
                .fallback(handle_webdav_request)
                .with_state(webdav_state);

            let addr = SocketAddr::from(([127, 0, 0, 1], bind_port));
            let listener = tokio::net::TcpListener::bind(addr).await?;
            println!("Starting WebDAV server on http://{}", addr);
            axum::serve(listener, app).await?;
        }
        Commands::Serve { s3_port, webdav, webdav_port } => {
            let mc = config.mc.as_ref();
            let s3_bind_port = s3_port
                .or_else(|| mc.map(|m| m.port))
                .unwrap_or(8080);

            let clients = login_all_accounts(&config, proxy).await?;
            let s3_clients = clients.clone();
            let s3_auth = mc.map(|m| (m.access_key.clone(), m.secret_key.clone()));

            // S3 server task
            let s3_task = tokio::spawn(async move {
                let backend = HamrahS3Backend::new(s3_clients);
                let mut builder = S3ServiceBuilder::new(backend);
                builder.set_validation(AnyName);
                if let Some((access_key, secret_key)) = s3_auth {
                    builder.set_auth(SimpleAuth::from_single(&access_key, secret_key.as_str()));
                }
                let service = builder.build();

                let addr = SocketAddr::from(([127, 0, 0, 1], s3_bind_port));
                let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
                println!("Starting S3-compatible server on http://{}", addr);

                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let service = service.clone();
                    tokio::spawn(async move {
                        let conn = hyper::server::conn::http1::Builder::new()
                            .serve_connection(hyper_util::rt::TokioIo::new(stream), service);
                        if let Err(err) = conn.await {
                            eprintln!("Error serving connection: {:?}", err);
                        }
                    });
                }
            });

            if webdav {
                // WebDAV server (experimental, read-only) — pre-warm caches before accepting
                let webdav_bind_port = webdav_port.unwrap_or(8081);
                let webdav_state = Arc::new(WebDavState::new(clients));
                prewarm_caches(webdav_state.clone()).await;
                let app = axum::Router::new()
                    .fallback(handle_webdav_request)
                    .with_state(webdav_state);
                let addr = SocketAddr::from(([127, 0, 0, 1], webdav_bind_port));
                let listener = tokio::net::TcpListener::bind(addr).await?;
                println!("Starting WebDAV server (read-only) on http://{}", addr);
                let webdav_task = tokio::spawn(async move {
                    axum::serve(listener, app).await.unwrap();
                });
                tokio::select! {
                    _ = s3_task => {},
                    _ = webdav_task => {},
                }
            } else {
                s3_task.await?;
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
