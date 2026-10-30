use cf_gateway::network::{NetworkConfig, NetworkMode, DOH_URL};
use cf_gateway::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(
        long,
        value_enum,
        help = "Network route [env: CF_GATEWAY_NETWORK_MODE] [default: direct]",
        global = true
    )]
    network_mode: Option<NetworkMode>,
    #[arg(long, global = true, help = "Upstream proxy [env: CF_GATEWAY_PROXY]")]
    proxy: Option<String>,
    #[arg(long, global=true, action=clap::ArgAction::Set, help="Enable DoH [env: CF_GATEWAY_DOH] [default: false]")]
    doh: Option<bool>,
    #[arg(
        long,
        global = true,
        help = "DoH HTTPS endpoint [env: CF_GATEWAY_DOH_URL]"
    )]
    doh_url: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    Css {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value = "19686")]
        port: u16,
        #[arg(long, env = "CF_GATEWAY_TOKEN", default_value = "")]
        token: String,
        #[arg(long)]
        capture_dir: Option<PathBuf>,
        #[arg(
            long,
            env = "CF_GATEWAY_LAYOUT_CACHE_DIR",
            help = "Directory for compiled initializer layouts; \"none\" disables [default: <temp>/cf-gateway-layout-cache]"
        )]
        layout_cache_dir: Option<String>,
    },
    Probe {
        #[arg(long)]
        url: String,
        #[arg(long, default_value = "90000")]
        timeout_ms: u64,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        capture_dir: Option<PathBuf>,
        #[arg(long, env = "CF_GATEWAY_LAYOUT_CACHE_DIR")]
        layout_cache_dir: Option<String>,
    },
    Compile {
        #[arg(long)]
        script: PathBuf,
        #[arg(long)]
        frame: bool,
        #[arg(long)]
        output: PathBuf,
    },
    Inspect {
        #[arg(long)]
        script: PathBuf,
        #[arg(long)]
        program: PathBuf,
        #[arg(long)]
        frame: bool,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        html: Option<PathBuf>,
        #[arg(long)]
        parent_html: Option<PathBuf>,
    },
}
/// Unset uses a private directory under the OS temporary path; "none" or an
/// empty value disables persistence. An unusable directory only disables it.
fn configure_layout_cache(option: Option<String>) -> serde_json::Value {
    let dir = match option.as_deref().map(str::trim) {
        None => Some(std::env::temp_dir().join("cf-gateway-layout-cache")),
        Some("") | Some("none") | Some("off") => None,
        Some(path) => Some(PathBuf::from(path)),
    };
    cf_gateway::data::layout_store::configure(dir)
}
async fn execute() -> Result<()> {
    let args = Args::parse();
    let network = args
        .network_config(|key| std::env::var(key).ok())
        .unwrap_or_else(|error| error.exit());
    match args.command.unwrap_or(Command::Serve {
        host: "127.0.0.1".into(),
        port: 19686,
        token: std::env::var("CF_GATEWAY_TOKEN").unwrap_or_default(),
        capture_dir: None,
        layout_cache_dir: std::env::var("CF_GATEWAY_LAYOUT_CACHE_DIR").ok(),
    }) {
        Command::Css { input, output } => {
            let html = std::fs::read_to_string(input)?;
            let f = cf_gateway::html::Facts::read(&html)?;
            let sheets = f
                .style_texts
                .iter()
                .map(|s| cf_gateway::css::stylesheet(s))
                .collect::<Result<Vec<_>>>()?;
            std::fs::write(output, serde_json::to_vec_pretty(&sheets)?)?;
            Ok(())
        }
        Command::Serve {
            host,
            port,
            token,
            capture_dir,
            layout_cache_dir,
        } => {
            println!(
                "{}",
                serde_json::json!({"layoutCache":configure_layout_cache(layout_cache_dir)})
            );
            cf_gateway::web::serve(
                cf_gateway::service::Service::with_network(
                    cf_gateway::engine::Engine { capture_dir },
                    network.clone(),
                )?,
                &host,
                port,
                token,
            )
            .await
        }
        Command::Probe {
            url,
            timeout_ms,
            output,
            capture_dir,
            layout_cache_dir,
        } => {
            configure_layout_cache(layout_cache_dir);
            let engine = cf_gateway::engine::Engine { capture_dir };
            let session = std::sync::Arc::new(cf_gateway::network::Session::new(
                cf_gateway::network::Dns::new(&network)?,
                network.proxy.clone(),
            )?);
            let started = std::time::Instant::now();
            let result = engine
                .run(
                    session,
                    &url,
                    started + std::time::Duration::from_millis(timeout_ms),
                    false,
                    None,
                )
                .await;
            let value = match result {
                Ok(r) => {
                    serde_json::json!({"ok":true,"status":r.response.status,"url":r.response.url,"body":r.response.text,"challenge":r.details,"strictPureAlgorithmAcceptance":r.solved,"timings":{"totalMs":started.elapsed().as_secs_f64()*1000.}})
                }
                Err(e) => {
                    let mut v = e.reply();
                    v["timings"] =
                        serde_json::json!({"totalMs":started.elapsed().as_secs_f64()*1000.});
                    v
                }
            };
            std::fs::write(output, serde_json::to_vec_pretty(&value)?)?;
            println!(
                "{}",
                serde_json::json!({"ok":value["ok"],"status":value["status"],"code":value["code"],"timings":value["timings"]})
            );
            Ok(())
        }
        Command::Compile {
            script,
            frame,
            output,
        } => {
            let p = cf_gateway::compiler::compile(&std::fs::read_to_string(script)?, frame)?;
            std::fs::write(output, serde_json::to_vec_pretty(&p.meta)?)?;
            Ok(())
        }
        Command::Inspect {
            script,
            program,
            frame,
            output,
            html,
            parent_html,
        } => {
            let script = std::fs::read_to_string(script)?;
            let plan = cf_gateway::data::Plan::analyze(&script, frame)?;
            let encoded = std::fs::read_to_string(program)?;
            let mut response = cf_gateway::data::Response::read(encoded.trim(), &plan, false)?;
            let catalog = if frame {
                Some(cf_gateway::catalog::Catalog::read(&mut response, &plan)?)
            } else {
                None
            };
            let mut computed = serde_json::Value::Null;
            if let (Some(html), Some(catalog)) = (html, catalog.as_ref()) {
                let html = std::fs::read_to_string(html)?;
                let parent = if let Some(p) = parent_html {
                    std::fs::read_to_string(p)?
                } else {
                    String::new()
                };
                let context = cf_gateway::models::Context {
                    response: &response,
                    plan: &plan,
                    catalog,
                    html: &html.into(),
                    parent_html: &parent.into(),
                    frame_url: "https://challenges.cloudflare.com/frame",
                    page_url: "https://twkan.com/",
                    mode: "managed",
                };
                let result = cf_gateway::engine::calculate_all(&context)?;
                computed = serde_json::json!({"fields":result.fields,"kinds":result.kinds,"count":result.fields.len()});
            }
            let value = serde_json::json!({"program":response.program,"literals":response.literals,"packed":response.packed,"catalog":catalog,"computed":computed});
            std::fs::write(output, serde_json::to_vec_pretty(&value)?)?;
            Ok(())
        }
    }
}
impl Args {
    fn network_config(
        &self,
        environment: impl Fn(&str) -> Option<String>,
    ) -> std::result::Result<NetworkConfig, clap::Error> {
        let bad = |message: String| clap::Error::raw(clap::error::ErrorKind::InvalidValue, message);
        let mode = if let Some(mode) = self.network_mode {
            mode
        } else {
            NetworkMode::from_str(
                &environment("CF_GATEWAY_NETWORK_MODE").unwrap_or_else(|| "direct".into()),
                false,
            )
            .map_err(bad)?
        };
        let doh = if let Some(enabled) = self.doh {
            enabled
        } else {
            match environment("CF_GATEWAY_DOH").as_deref().unwrap_or("false") {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return Err(bad("CF_GATEWAY_DOH needs true/false".into())),
            }
        };
        let proxy = self
            .proxy
            .clone()
            .or_else(|| environment("CF_GATEWAY_PROXY"))
            .filter(|v| !v.is_empty());
        let doh_url = self
            .doh_url
            .clone()
            .or_else(|| environment("CF_GATEWAY_DOH_URL"))
            .unwrap_or_else(|| DOH_URL.into());
        let config = NetworkConfig {
            mode,
            proxy,
            doh,
            doh_url,
        };
        config.validate().map_err(|error| bad(error.to_string()))?;
        Ok(config)
    }
}
fn main() {
    let result = std::thread::Builder::new()
        .name("cf-gateway".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .thread_stack_size(16 * 1024 * 1024)
                .enable_all()
                .build()
                .unwrap()
                .block_on(execute())
        })
        .unwrap()
        .join()
        .unwrap();
    if let Err(error) = result {
        eprintln!("{}", error.reply());
        std::process::exit(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_direct_system_dns_and_arguments_override_environment() {
        let args = Args::try_parse_from(["cf-gateway"]).unwrap();
        let config = args.network_config(|_| None).unwrap();
        assert_eq!(config.mode, NetworkMode::Direct);
        assert!(!config.doh);
        let args = Args::try_parse_from([
            "cf-gateway",
            "serve",
            "--network-mode",
            "proxy",
            "--proxy",
            "http://127.0.0.1:7890",
            "--doh=false",
            "--doh-url",
            DOH_URL,
        ])
        .unwrap();
        let config = args.network_config(|_| Some("invalid".into())).unwrap();
        assert_eq!(config.mode, NetworkMode::Proxy);
        assert!(!config.doh);
        assert_eq!(config.proxy.as_deref(), Some("http://127.0.0.1:7890"));
        for value in ["fixed", "hybrid", "doh"] {
            assert!(Args::try_parse_from(["cf-gateway", "--network-mode", value]).is_err());
        }
    }
    #[test]
    fn startup_requires_proxy_and_validates_dns_settings() {
        let args = Args::try_parse_from(["cf-gateway", "--network-mode", "proxy"]).unwrap();
        assert!(args.network_config(|_| None).is_err());
        let args = Args::try_parse_from(["cf-gateway"]).unwrap();
        assert!(args
            .network_config(|key| (key == "CF_GATEWAY_DOH").then(|| "invalid".into()))
            .is_err());
        let config = args
            .network_config(|key| match key {
                "CF_GATEWAY_DOH" => Some("true".into()),
                "CF_GATEWAY_DOH_URL" => Some("https://dns.example/dns-query".into()),
                _ => None,
            })
            .unwrap();
        assert!(config.doh);
        assert_eq!(config.doh_url, "https://dns.example/dns-query");
    }
}
