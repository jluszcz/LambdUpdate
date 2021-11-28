use anyhow::Result;
use clap::{App, Arg};
use lambdupdate::{set_up_logger, update, Event, Record};
use log::debug;

#[derive(Debug)]
struct Args {
    verbose: bool,
    region: String,
    bucket: String,
    key: String,
}

fn parse_args() -> Args {
    let matches = App::new("LambdUpdate")
        .version("0.1")
        .author("Jacob Luszcz")
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .help("Verbose mode. Outputs DEBUG and higher log messages."),
        )
        .arg(
            Arg::with_name("region")
                .short("r")
                .long("region")
                .required(true)
                .takes_value(true)
                .help("AWS region."),
        )
        .arg(
            Arg::with_name("bucket")
                .short("b")
                .long("bucket")
                .required(true)
                .takes_value(true)
                .help("S3 bucket name."),
        )
        .arg(
            Arg::with_name("key")
                .short("k")
                .long("key")
                .required(true)
                .takes_value(true)
                .help("S3 key name."),
        )
        .get_matches();

    let verbose = matches.is_present("verbose");
    let region = matches.value_of("region").map(|l| l.into()).unwrap();
    let bucket = matches.value_of("bucket").map(|l| l.into()).unwrap();
    let key = matches.value_of("key").map(|l| l.into()).unwrap();

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
