use anyhow::{anyhow, Result};
use aws_config::ConfigLoader;
use lambda::Region;
use log::{debug, info, LevelFilter};
use serde::Deserialize;
use std::collections::HashMap;

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

#[derive(Debug, Deserialize)]
pub struct Bucket {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Object {
    pub key: String,
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
        .level(level)
        .level_for("hyper", LevelFilter::Warn)
        .level_for("rustls", LevelFilter::Warn)
        .level_for("smithy_http_tower", LevelFilter::Warn)
        .level_for("tracing", LevelFilter::Warn)
        .level_for("reqwest", LevelFilter::Warn)
        .level_for("html5ever", LevelFilter::Warn)
        .level_for("aws_config", LevelFilter::Warn)
        .chain(std::io::stdout())
        .apply();

    Ok(())
}

type Clients = (lambda::Client, s3::Client);

async fn clients<'a, 'b>(cache: &'a mut HashMap<String, Clients>, region: &'b str) -> &'a Clients {
    if !cache.contains_key(region) {
        let aws_config = ConfigLoader::default()
            .region(Region::new(region.to_string()))
            .load()
            .await;

        cache.insert(
            region.to_string(),
            (
                lambda::Client::new(&aws_config),
                s3::Client::new(&aws_config),
            ),
        );
    }

    cache.get(region).unwrap()
}

pub async fn update(event: &Event) -> Result<()> {
    let mut client_by_region = HashMap::new();

    for record in event.records.iter() {
        debug!("Processing {:?}", record);

        let (lambda_client, s3_client) = clients(&mut client_by_region, &record.region).await;

        let bucket = &record.s3.bucket.name;
        let key = &record.s3.object.key;

        let head_object_output = s3_client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await?;

        let object_md = head_object_output
            .metadata
            .ok_or_else(|| anyhow!("{}:{} has no metadata", bucket, key))?;
        debug!("Object Metadata: {:?}", object_md);

        let function_names = object_md
            .get("function.names")
            .ok_or_else(|| anyhow!("{}:{} metadata is missing function.names", bucket, key))?;

        for function_name in function_names.split(',') {
            lambda_client
                .update_function_code()
                .function_name(function_name)
                .s3_bucket(bucket)
                .s3_key(key)
                .send()
                .await?;

            info!("Updated {} with {}:{}", function_name, bucket, key);
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
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
}
