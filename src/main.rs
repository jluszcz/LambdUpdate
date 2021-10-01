use anyhow::Result;
use clap::{App, Arg};
use lambdupdate::{set_up_logger, update, Bucket, Event, Object, Record, S3};

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
        let s3 = S3 {
            bucket: Bucket { name: args.bucket },
            object: Object { key: args.key },
        };

        Event {
            records: vec![Record {
                s3,
                region: args.region,
            }],
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args();
    set_up_logger(args.verbose)?;
    update(&args.into()).await?;

    Ok(())
}
