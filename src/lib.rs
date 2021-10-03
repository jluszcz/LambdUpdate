use anyhow::{anyhow, Result};
use aws_config::ConfigLoader;
use futures::future::try_join_all;
use lambda::Region;
use log::{debug, info, LevelFilter};
use s3::output::HeadObjectOutput;
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt::Display;

const FUNCTION_NAME_MD_KEY: &str = "function.names";

#[derive(Debug, Deserialize)]
pub struct Event {
    #[serde(alias = "Records")]
    pub records: Vec<Record>,
}

#[derive(Debug, Deserialize)]
pub struct Record {
    #[serde(alias = "awsRegion")]
    pub region: String,
    pub s3: S3,
}

#[derive(Debug, Deserialize)]
pub struct S3 {
    pub bucket: Bucket,
    pub object: Object,
}

impl From<(&str, &str)> for S3 {
    fn from(bucket_and_key: (&str, &str)) -> Self {
        let (bucket, key) = bucket_and_key;
        Self {
            bucket: bucket.into(),
            object: key.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Bucket {
    pub name: String,
}

impl From<&str> for Bucket {
    fn from(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Object {
    pub key: String,
}

impl From<&str> for Object {
    fn from(key: &str) -> Self {
        Self {
            key: key.to_string(),
        }
    }
}

pub fn set_up_logger(verbose: bool) -> Result<()> {
    let level = if verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    let _ = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] [{}] {}",
                chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(LevelFilter::Warn)
        .level_for("lambdupdate", level)
        .chain(std::io::stdout())
        .apply();

    Ok(())
}

fn get_region(records: &[Record]) -> Result<String> {
    let regions = records.iter().map(|r| &r.region).collect::<HashSet<_>>();

    if regions.len() == 1 {
        Ok(regions
            .into_iter()
            .find(|_| true)
            .cloned()
            .expect("regions has one element"))
    } else {
        Err(anyhow!("Multiple regions in event: {:?}", regions))
    }
}

async fn get_function_names_from_md(s3_client: &s3::Client, record: &Record) -> Option<String> {
    let bucket = &record.s3.bucket.name;
    let key = &record.s3.object.key;

    debug!("Head Object: {}:{}", bucket, key);
    let head_object_output = s3_client.head_object().bucket(bucket).key(key).send().await;
    get_function_names_from_head_object_output(head_object_output, bucket, key)
}

fn get_function_names_from_head_object_output<E>(
    head_object_output: Result<HeadObjectOutput, E>,
    bucket: &str,
    key: &str,
) -> Option<String> {
    if let Ok(head_object_output) = head_object_output {
        info!("Head Object Succeeded: {}:{}", bucket, key);

        let object_md = head_object_output.metadata;
        debug!("Object Metadata: {:?}", object_md);

        object_md
            .map(|m| m.get(FUNCTION_NAME_MD_KEY).cloned())
            .flatten()
    } else {
        info!("Head Object Failed: {}:{}", bucket, key);
        None
    }
}

fn get_function_names<S>(function_names_from_md: Option<S>, record: &Record) -> Result<String>
where
    S: Into<String> + Display,
{
    let function_names = match function_names_from_md {
        Some(function_names) => {
            debug!("Function names from object metadata: {}", function_names);
            function_names.into()
        }
        None => {
            let key = &record.s3.object.key;
            let function_name = key
                .strip_suffix(".zip")
                .ok_or_else(|| anyhow!("'.zip' not found in object key: {}", key))?;

            debug!("Function name from object key: {}", function_name);
            function_name.to_string()
        }
    };

    Ok(function_names)
}

async fn update_code(
    lambda_client: lambda::Client,
    function_name: String,
    bucket: String,
    key: String,
) -> Result<()> {
    debug!(
        "Update Function Code: {} <-- {}:{}",
        function_name, bucket, key
    );

    lambda_client
        .update_function_code()
        .function_name(&function_name)
        .s3_bucket(&bucket)
        .s3_key(&key)
        .send()
        .await?;

    info!(
        "Update Function Code Succeeded: {} <-- {}:{}",
        function_name, bucket, key
    );

    Ok(())
}

pub async fn update(event: Event) -> Result<()> {
    debug!("Event: {:?}", event);

    let aws_config = ConfigLoader::default()
        .region(Region::new(get_region(&event.records)?))
        .load()
        .await;

    let s3_client = s3::Client::new(&aws_config);
    let lambda_client = lambda::Client::new(&aws_config);

    let mut update_code_futures = Vec::with_capacity(event.records.len());

    for record in event.records {
        debug!("Record: {:?}", record);

        let function_names = get_function_names_from_md(&s3_client, &record).await;
        let function_names = get_function_names(function_names, &record)?;

        for function_name in function_names.split(',') {
            update_code_futures.push(tokio::spawn(update_code(
                lambda_client.clone(),
                function_name.to_string(),
                record.s3.bucket.name.clone(),
                record.s3.object.key.clone(),
            )));
        }
    }

    debug!("{} function(s) to update", update_code_futures.len());
    try_join_all(update_code_futures).await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use anyhow::Error;
    use super::*;

    const TEST_EVENT: &str = r#"
    {
        "Records": [
          {
            "eventVersion": "2.0",
            "eventSource": "aws:s3",
            "awsRegion": "us-west-2",
            "eventTime": "1970-01-01T00:00:00.000Z",
            "eventName": "ObjectCreated:Put",
            "userIdentity": {
              "principalId": "EXAMPLE"
            },
            "requestParameters": {
              "sourceIPAddress": "127.0.0.1"
            },
            "responseElements": {
              "x-amz-request-id": "EXAMPLE123456789",
              "x-amz-id-2": "EXAMPLE123/5678abcdefghijklambdaisawesome/mnopqrstuvwxyzABCDEFGH"
            },
            "s3": {
              "s3SchemaVersion": "1.0",
              "configurationId": "testConfigRule",
              "bucket": {
                "name": "my-s3-bucket",
                "ownerIdentity": {
                  "principalId": "EXAMPLE"
                },
                "arn": "arn:aws:s3:::example-bucket"
              },
              "object": {
                "key": "HappyFace.jpg",
                "size": 1024,
                "eTag": "0123456789abcdef0123456789abcdef",
                "sequencer": "0A1B2C3D4E5F678901"
              }
            }
          }
        ]
      }
    "#;

    impl Record {
        fn new(region: &str, bucket: &str, key: &str) -> Self {
            Self {
                region: region.to_string(),
                s3: (bucket, key).into(),
            }
        }
    }

    #[test]
    fn test_deserialize() -> Result<()> {
        let event: Event = serde_json::from_str(TEST_EVENT)?;

        assert_eq!(1, event.records.len());

        let record = &event.records[0];
        assert_eq!("us-west-2", record.region);
        assert_eq!("my-s3-bucket", record.s3.bucket.name);
        assert_eq!("HappyFace.jpg", record.s3.object.key);

        Ok(())
    }

    #[test]
    fn test_get_region() -> Result<()> {
        let mut records = vec![Record::new("us-east-1", "foo", "bar")];

        assert_eq!("us-east-1", get_region(&records)?);

        records.push(Record::new("us-east-1", "baz", "quux"));

        assert_eq!("us-east-1", get_region(&records)?);

        Ok(())
    }

    #[test]
    fn test_get_region_multiple() {
        let records = vec![
            Record::new("us-east-1", "foo", "bar"),
            Record::new("us-east-2", "baz", "quux"),
        ];

        let res = get_region(&records);
        assert!(res.is_err());
        if let Err(e) = res {
            assert!(e.to_string().contains("Multiple regions"));
        }
    }

    #[test]
    fn test_get_function_names_from_md() -> Result<()> {
        let function_names =
            get_function_names(Some("foo,bar"), &Record::new("us-east-1", "foo", "bar"))?;

        assert_eq!("foo,bar", function_names);

        Ok(())
    }

    #[test]
    fn test_get_function_names_from_key() -> Result<()> {
        let function_names =
            get_function_names(None::<&str>, &Record::new("us-east-1", "foo", "bar.zip"))?;

        assert_eq!("bar", function_names);

        Ok(())
    }

    #[test]
    fn test_get_function_names_from_unzipped_key() {
        let res = get_function_names(None::<&str>, &Record::new("us-east-1", "foo", "bar"));

        assert!(res.is_err());
        if let Err(e) = res {
            assert!(e.to_string().contains("'.zip' not found"));
        }
    }

    #[test]
    fn test_get_function_names_from_head_object_output() {
        let fn_names = "foo,bar";

        let output: Result<HeadObjectOutput, Error> = Ok(HeadObjectOutput::builder()
            .metadata(FUNCTION_NAME_MD_KEY, fn_names)
            .build());

        let fn_names_from_output =
            get_function_names_from_head_object_output(output, "bucket", "key");

        assert!(fn_names_from_output.is_some());
        assert_eq!(fn_names, fn_names_from_output.unwrap());
    }

    #[test]
    fn test_get_function_names_from_head_object_output_err() {
        let output: Result<HeadObjectOutput, Error> = Err(anyhow!("Error!"));

        let fn_names_from_output =
            get_function_names_from_head_object_output(output, "bucket", "key");

        assert!(fn_names_from_output.is_none());
    }

    #[test]
    fn test_get_function_names_from_head_object_output_no_metadata() {
        let output: Result<HeadObjectOutput, Error> = Ok(HeadObjectOutput::builder().build());

        let fn_names_from_output =
            get_function_names_from_head_object_output(output, "bucket", "key");

        assert!(fn_names_from_output.is_none());
    }

    #[test]
    fn test_get_function_names_from_head_object_output_no_function_names() {
        let output: Result<HeadObjectOutput, Error> = Ok(HeadObjectOutput::builder()
            .set_metadata(Some(HashMap::new()))
            .build());

        let fn_names_from_output =
            get_function_names_from_head_object_output(output, "bucket", "key");

        assert!(fn_names_from_output.is_none());
    }
}
