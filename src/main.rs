use cf_gateway::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
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
async fn execute() -> Result<()> {
    match Args::parse().command.unwrap_or(Command::Serve {
        host: "127.0.0.1".into(),
        port: 19686,
        token: std::env::var("CF_GATEWAY_TOKEN").unwrap_or_default(),
        capture_dir: None,
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
        } => {
            cf_gateway::web::serve(
                cf_gateway::service::Service::new(cf_gateway::engine::Engine { capture_dir })?,
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
        } => {
            let engine = cf_gateway::engine::Engine { capture_dir };
            let session = std::sync::Arc::new(cf_gateway::network::Session::new(
                cf_gateway::network::Dns::new(true)?,
                None,
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
                    html: &html,
                    parent_html: &parent,
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
