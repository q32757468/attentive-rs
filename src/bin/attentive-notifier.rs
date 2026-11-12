use attentive_notifier::{start_notifier, StartNotifierOptions};
use std::{env, error::Error, process};

const DEFAULT_PORT: u16 = 8765;

#[derive(Default)]
struct CliArgs {
    host: Option<String>,
    port: Option<u16>,
    help: bool,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args(env::args().skip(1))?;
    if args.help {
        println!("{}", notifier_help());
        return Ok(());
    }

    let host = args
        .host
        .or_else(|| env::var("ATTENTIVE_NOTIFIER_HOST").ok())
        .unwrap_or_else(|| "0.0.0.0".to_string());
    let port = args
        .port
        .or_else(|| {
            env::var("ATTENTIVE_NOTIFIER_PORT")
                .ok()
                .and_then(|value| parse_port(&value))
        })
        .unwrap_or(DEFAULT_PORT);

    let server = start_notifier(
        StartNotifierOptions::default()
            .with_host(host.clone())
            .with_port(port),
    )
    .await?;
    println!("Attentive notifier listening on {}", server.local_addr());

    tokio::signal::ctrl_c().await?;
    server.shutdown().await?;
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<CliArgs, String> {
    let mut args = args.into_iter();
    let mut result = CliArgs::default();

    while let Some(token) = args.next() {
        if token == "--" {
            continue;
        }
        if token == "--help" || token == "-h" {
            result.help = true;
            continue;
        }

        let (name, inline_value) = token
            .split_once('=')
            .map(|(name, value)| (name, Some(value.to_string())))
            .unwrap_or((token.as_str(), None));
        if name != "--host" && name != "--port" {
            return Err(format!("unknown option: {token}"));
        }

        let value = match inline_value {
            Some(value) => value,
            None => args
                .next()
                .ok_or_else(|| format!("{name} requires a value"))?,
        };
        if value.is_empty() || value.starts_with("--") {
            return Err(format!("{name} requires a value"));
        }

        if name == "--host" {
            result.host = Some(value);
        } else {
            result.port = parse_port(&value);
            if result.port.is_none() {
                return Err("--port must be an integer between 0 and 65535".to_string());
            }
        }
    }

    Ok(result)
}

fn parse_port(value: &str) -> Option<u16> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn notifier_help() -> String {
    format!(
        "Usage: attentive-notifier [options]\n\n\
         Starts the Windows notification HTTP service.\n\n\
         Options:\n\
           --host <address>  Listen address (default: 0.0.0.0)\n\
           --port <port>     Listen port (default: {DEFAULT_PORT})\n\
           -h, --help        Show this help"
    )
}
