use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use clipboard_core::ipc::{ClipSummary, Request, Response};
use clipboard_ui::client::{
    clip_text, default_socket_path, request_recent, request_search, source_label, IpcClient,
    DEFAULT_RECENT_LIMIT,
};
use std::{
    io::{self, Write},
    path::PathBuf,
};

#[derive(Debug, Parser)]
#[command(
    name = "context-clipboard",
    version,
    about = "Phase-1 Context Clipboard UI shell and IPC client"
)]
struct Cli {
    /// Override the daemon Unix socket path.
    #[arg(long, global = true, value_name = "PATH")]
    socket: Option<PathBuf>,

    /// Print daemon responses as JSON.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Print daemon status.
    Status,
    /// List recent clipboard items.
    Recent {
        #[arg(short, long, default_value_t = DEFAULT_RECENT_LIMIT)]
        limit: u32,
    },
    /// Search clipboard text and source metadata.
    Search {
        query: String,
        #[arg(short, long, default_value_t = 20)]
        limit: u32,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        app: Option<String>,
        /// RFC3339 lower bound understood by the daemon.
        #[arg(long)]
        since: Option<String>,
    },
    /// Print decrypted clip text to stdout.
    Get { id: String },
    /// Delete one clipboard item.
    Delete { id: String },
    /// Clear clipboard history.
    Clear,
    /// Pause clipboard capture.
    Pause,
    /// Resume clipboard capture.
    Resume,
    /// Print the checked-in development UI stub location.
    ServeUi,
    /// Start a small terminal prompt that sends IPC commands.
    Interactive,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "context_clipboard=warn,clipboard_ui=warn".into()),
        )
        .without_time()
        .init();

    let cli = Cli::parse();
    let socket = match cli.socket {
        Some(path) => path,
        None => default_socket_path().context("resolving daemon socket path")?,
    };
    let client = IpcClient::new(socket);

    match cli.command.unwrap_or(Command::Interactive) {
        Command::Interactive => interactive(client, cli.json).await,
        command => run_command(&client, command, cli.json).await,
    }
}

async fn interactive(client: IpcClient, json: bool) -> Result<()> {
    println!("Context Clipboard IPC prompt");
    println!("Socket: {}", client.socket_path().display());
    println!("Type help for commands, quit to exit.");

    loop {
        print!("context-clipboard> ");
        io::stdout().flush().context("flush prompt")?;

        let mut line = String::new();
        if io::stdin().read_line(&mut line).context("read prompt")? == 0 {
            println!();
            return Ok(());
        }

        let Some(command) = parse_interactive_command(line.trim())? else {
            continue;
        };

        if let Command::Interactive = command {
            continue;
        }

        if let Err(error) = run_command(&client, command, json).await {
            eprintln!("error: {error}");
        }
    }
}

fn parse_interactive_command(line: &str) -> Result<Option<Command>> {
    if line.is_empty() {
        return Ok(None);
    }

    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return Ok(None);
    };

    let parsed = match command {
        "help" | "?" => {
            println!("status");
            println!("recent [limit]");
            println!("search <query>");
            println!("get <id>");
            println!("delete <id>");
            println!("clear");
            println!("pause");
            println!("resume");
            println!("serve-ui");
            println!("quit");
            return Ok(None);
        }
        "quit" | "exit" => std::process::exit(0),
        "status" => Command::Status,
        "recent" => {
            let limit = parts
                .next()
                .map(str::parse::<u32>)
                .transpose()
                .context("recent limit must be a number")?
                .unwrap_or(DEFAULT_RECENT_LIMIT);
            Command::Recent { limit }
        }
        "search" => {
            let query = parts.collect::<Vec<_>>().join(" ");
            if query.is_empty() {
                bail!("usage: search <query>");
            }
            Command::Search {
                query,
                limit: 20,
                category: None,
                app: None,
                since: None,
            }
        }
        "get" => Command::Get {
            id: require_one(parts.next(), "usage: get <id>")?.to_owned(),
        },
        "delete" | "del" | "rm" => Command::Delete {
            id: require_one(parts.next(), "usage: delete <id>")?.to_owned(),
        },
        "clear" => Command::Clear,
        "pause" => Command::Pause,
        "resume" => Command::Resume,
        "serve-ui" => Command::ServeUi,
        other => bail!("unknown command: {other}"),
    };

    Ok(Some(parsed))
}

fn require_one<'a>(value: Option<&'a str>, usage: &str) -> Result<&'a str> {
    value.ok_or_else(|| anyhow!(usage.to_owned()))
}

async fn run_command(client: &IpcClient, command: Command, json: bool) -> Result<()> {
    match command {
        Command::Status => {
            let response = request_or_error(client, &Request::GetStatus).await?;
            if json {
                print_json(&response)?;
                return Ok(());
            }
            print_status(response)
        }
        Command::Recent { limit } => {
            let response = request_or_error(client, &request_recent(limit)).await?;
            if json {
                print_json(&response)?;
                return Ok(());
            }
            print_items(response)
        }
        Command::Search {
            query,
            limit,
            category,
            app,
            since,
        } => {
            let request = request_search(query, limit, category, app, since);
            let response = request_or_error(client, &request).await?;
            if json {
                print_json(&response)?;
                return Ok(());
            }
            print_items(response)
        }
        Command::Get { id } => {
            let response = request_or_error(client, &Request::Get { id }).await?;
            if json {
                print_json(&response)?;
                return Ok(());
            }
            print_clip_text(response)
        }
        Command::Delete { id } => {
            let response = request_or_error(client, &Request::Delete { id }).await?;
            print_ack(response, json, "deleted")
        }
        Command::Clear => {
            let response = request_or_error(client, &Request::Clear).await?;
            print_ack(response, json, "cleared")
        }
        Command::Pause => {
            let response = request_or_error(client, &Request::SetPaused { paused: true }).await?;
            print_ack(response, json, "paused")
        }
        Command::Resume => {
            let response = request_or_error(client, &Request::SetPaused { paused: false }).await?;
            print_ack(response, json, "resumed")
        }
        Command::ServeUi => {
            let path = frontend_index_path();
            println!("Development UI stub: {}", path.display());
            println!("No HTTP server was started. Production wiring belongs in Tauri.");
            Ok(())
        }
        Command::Interactive => Ok(()),
    }
}

async fn request_or_error(client: &IpcClient, request: &Request) -> Result<Response> {
    let response = client.send(request).await?;
    if let Response::Err { code, message } = &response {
        bail!("daemon returned {code}: {message}");
    }
    Ok(response)
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_status(response: Response) -> Result<()> {
    match response {
        Response::Status {
            paused,
            count,
            version,
            platform_caps,
        } => {
            println!("paused: {paused}");
            println!("clips: {count}");
            println!("protocol version: {version}");
            if !platform_caps.is_empty() {
                println!("platform caps: {}", platform_caps.join(", "));
            }
            Ok(())
        }
        other => unexpected_response(other, "Status"),
    }
}

fn print_items(response: Response) -> Result<()> {
    match response {
        Response::SearchResults { items } => {
            if items.is_empty() {
                println!("No clips found.");
                return Ok(());
            }

            for (index, item) in items.iter().enumerate() {
                print_item(index + 1, item);
            }
            Ok(())
        }
        other => unexpected_response(other, "SearchResults"),
    }
}

fn print_item(index: usize, item: &ClipSummary) {
    let source = source_label(item).unwrap_or("unknown source");
    let category = if item.category.is_empty() {
        "uncategorized"
    } else {
        item.category.as_str()
    };
    let preview = if item.preview.is_empty() {
        "<no preview>"
    } else {
        item.preview.as_str()
    };

    println!(
        "{index}. {} [{}] {source} {}",
        item.id, category, item.last_seen_at
    );
    println!("   {preview}");
}

fn print_clip_text(response: Response) -> Result<()> {
    match response {
        Response::ClipDetail { detail } => {
            let body = clip_text(&detail).unwrap_or("");
            print!("{body}");
            io::stdout().flush().context("flush clip text")?;
            Ok(())
        }
        other => unexpected_response(other, "ClipDetail"),
    }
}

fn print_ack(response: Response, json: bool, fallback: &str) -> Result<()> {
    if json {
        print_json(&response)?;
        return Ok(());
    }

    match response {
        Response::Ok => {
            println!("{fallback}");
            Ok(())
        }
        Response::Status { .. } => print_status(response),
        other => unexpected_response(other, "Ok"),
    }
}

fn unexpected_response(response: Response, expected: &str) -> Result<()> {
    bail!(
        "expected {expected} response, got {}",
        serde_json::to_string(&response)?
    )
}

fn frontend_index_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("frontend")
        .join("index.html")
}
