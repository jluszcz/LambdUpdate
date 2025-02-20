use anyhow::Result;
use clap::{Arg, ArgAction, Command};
use lambdupdate::{Event, Record, set_up_logger, update};
use log::debug;

#[derive(Debug)]
struct Args {
    verbose: bool,
    region: String,
    bucket: String,
    key: String,
}

fn parse_args() -> Args {
    let matches = Command::new("LambdUpdate")
        .version("0.1")
        .author("Jacob Luszcz")
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::SetTrue)
                .help("Verbose mode. Outputs DEBUG and higher log messages."),
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

    let verbose = matches.get_flag("verbose");

    let region = matches
        .get_one::<String>("region")
        .map(|l| l.into())
        .unwrap();

    let bucket = matches
        .get_one::<String>("bucket")
        .map(|l| l.into())
        .unwrap();

    let key = matches.get_one::<String>("key").map(|l| l.into()).unwrap();

    Args {
        verbose,
        region,
        bucket,
        key,
    }
}

impl From<Args> for Event {
    fn from(args: Args) -> Self {
        Event {
            records: vec![Record {
                region: args.region,
                s3: (args.bucket.as_str(), args.key.as_str()).into(),
            }],
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args();
    set_up_logger(module_path!(), args.verbose)?;
    debug!("Args: {:?}", args);

    update(args.into()).await?;

    Ok(())
}
