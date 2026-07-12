use anyhow::Result;
use aws_lambda_events::s3::S3Event;
use clap::{ArgAction, Parser};
use jluszcz_rust_utils::set_up_logger;
use lambdupdate::{APP_NAME, create_s3_event_record, update};
use log::debug;

#[derive(Debug, Parser)]
#[command(name = "LambdUpdate", version, author, infer_long_args = true)]
struct Args {
    /// Verbose mode (-v for debug, -vv for trace logging).
    #[arg(short, action = ArgAction::Count)]
    verbosity: u8,

    /// AWS region.
    #[arg(short, long)]
    region: String,

    /// S3 bucket name.
    #[arg(short, long)]
    bucket: String,

    /// S3 key name.
    #[arg(short, long)]
    key: String,
}

impl From<Args> for S3Event {
    fn from(args: Args) -> Self {
        let record = create_s3_event_record(&args.region, &args.bucket, &args.key);
        let json = serde_json::json!({
            "Records": [record]
        });
        serde_json::from_value(json).expect("Failed to construct S3Event from JSON")
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    set_up_logger(APP_NAME, module_path!(), args.verbosity)?;
    debug!("Args: {args:?}");

    update(args.into()).await?;

    Ok(())
}
