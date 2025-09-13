use anyhow::Result;
use aws_lambda_events::s3::{S3Bucket, S3Entity, S3Event, S3EventRecord, S3Object};
use clap::{Arg, ArgAction, Command};
use jluszcz_rust_utils::{Verbosity, set_up_logger};
use lambdupdate::{APP_NAME, update};
use log::debug;

#[derive(Debug)]
struct Args {
    verbosity: Verbosity,
    region: String,
    bucket: String,
    key: String,
}

fn parse_args() -> Args {
    let matches = Command::new("LambdUpdate")
        .version("0.1")
        .author("Jacob Luszcz")
        .arg(
            Arg::new("verbosity")
                .short('v')
                .action(ArgAction::Count)
                .help("Verbose mode (-v for debug, -vv for trace logging)."),
        )
        .arg(
            Arg::new("region")
                .short('r')
                .long("region")
                .required(true)
                .help("AWS region."),
        )
        .arg(
            Arg::new("bucket")
                .short('b')
                .long("bucket")
                .required(true)
                .help("S3 bucket name."),
        )
        .arg(
            Arg::new("key")
                .short('k')
                .long("key")
                .required(true)
                .help("S3 key name."),
        )
        .get_matches();

    let verbosity = matches.get_count("verbosity").into();

    let region = matches
        .get_one::<String>("region")
        .cloned()
        .expect("region argument is required");

    let bucket = matches
        .get_one::<String>("bucket")
        .cloned()
        .expect("bucket argument is required");

    let key = matches
        .get_one::<String>("key")
        .cloned()
        .expect("key argument is required");

    Args {
        verbosity,
        region,
        bucket,
        key,
    }
}

impl From<Args> for S3Event {
    fn from(args: Args) -> Self {
        S3Event {
            records: vec![S3EventRecord {
                aws_region: Some(args.region),
                s3: S3Entity {
                    bucket: S3Bucket {
                        name: Some(args.bucket),
                        ..Default::default()
                    },
                    object: S3Object {
                        key: Some(args.key),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            }],
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args();
    set_up_logger(APP_NAME, module_path!(), args.verbosity)?;
    debug!("Args: {args:?}");

    update(args.into()).await?;

    Ok(())
}
