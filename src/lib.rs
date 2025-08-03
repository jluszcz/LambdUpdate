//! LambdUpdate library for automatically updating AWS Lambda functions from S3 events.
//!
//! This library processes S3 events triggered when ZIP files are uploaded to a code bucket,
//! extracts function names from object metadata or keys, and updates the corresponding Lambda functions.

use anyhow::{Result, anyhow};
use aws_config::ConfigLoader;
use aws_lambda_events::s3::{S3Event, S3EventRecord};
use aws_sdk_lambda::config::Region;
use aws_sdk_s3::operation::head_object::HeadObjectOutput;
use futures::future::try_join_all;
use log::{debug, info};
use std::collections::HashSet;
use std::fmt::Display;

pub const APP_NAME: &str = "lambdupdate";

const FUNCTION_NAME_MD_KEY: &str = "function.names";

/// Extracts the AWS region from S3 event records.
///
/// All records must be from the same region. Returns an error if no region is found
/// or if records contain multiple different regions.
///
/// # Arguments
/// * `records` - Slice of S3 event records to extract region from
///
/// # Returns
/// * `Result<String>` - The AWS region if exactly one unique region is found
///
/// # Errors
/// * Returns error if records contain 0 or multiple different regions
fn get_region(records: &[S3EventRecord]) -> Result<String> {
    let regions = records
        .iter()
        .filter_map(|r| r.aws_region.as_deref())
        .collect::<HashSet<_>>();

    if regions.len() == 1 {
        Ok(regions
            .into_iter()
            .next()
            .expect("regions has one element")
            .to_string())
    } else {
        Err(anyhow!("Invalid region count: {:?}", regions))
    }
}

async fn get_function_names_from_md(
    s3_client: &aws_sdk_s3::Client,
    bucket: &str,
    key: &str,
) -> Option<String> {
    debug!("Head Object: {bucket}:{key}");
    let head_object_output = s3_client.head_object().bucket(bucket).key(key).send().await;
    get_function_names_from_head_object_output(head_object_output, bucket, key)
}

fn get_function_names_from_head_object_output<E>(
    head_object_output: Result<HeadObjectOutput, E>,
    bucket: &str,
    key: &str,
) -> Option<String> {
    if let Ok(head_object_output) = head_object_output {
        info!("Head Object Succeeded: {bucket}:{key}");

        let object_md = head_object_output.metadata;
        debug!("Object Metadata: {object_md:?}");

        object_md.and_then(|m| m.get(FUNCTION_NAME_MD_KEY).cloned())
    } else {
        info!("Head Object Failed for {bucket}:{key} - will use object key for function name");
        None
    }
}

/// Determines function names from object metadata or object key.
///
/// First tries to get function names from object metadata using the key "function.names".
/// If not found, extracts the function name from the object key by stripping the ".zip" suffix.
///
/// # Arguments
/// * `function_names_from_md` - Optional function names from object metadata
/// * `key` - S3 object key to extract function name from if metadata is not available
///
/// # Returns
/// * `Result<String>` - Comma-separated function names or single function name
///
/// # Errors
/// * Returns error if no metadata is provided and the key doesn't end with ".zip"
fn get_function_names<S>(function_names_from_md: Option<S>, key: &str) -> Result<String>
where
    S: Into<String> + Display,
{
    let function_names = match function_names_from_md {
        Some(function_names) => {
            debug!("Function names from object metadata: {function_names}");
            function_names.into()
        }
        None => {
            let function_name = key
                .strip_suffix(".zip")
                .ok_or_else(|| anyhow!("'.zip' not found in object key: {key}"))?;

            debug!("Function name from object key: {function_name}");
            function_name.to_string()
        }
    };

    Ok(function_names)
}

/// Processes a comma-separated list of function names, trimming whitespace and filtering empty names.
///
/// # Arguments
/// * `function_names` - Comma-separated string of function names
///
/// # Returns
/// * `Result<Vec<String>>` - Vector of trimmed, non-empty function names
///
/// # Errors
/// * Returns error if no valid function names are found after processing
fn process_function_names(function_names: &str) -> Result<Vec<String>> {
    let processed_names: Vec<String> = function_names
        .split(',')
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();

    if processed_names.is_empty() {
        Err(anyhow!(
            "No valid function names found in: '{}' - check for empty or whitespace-only names",
            function_names
        ))
    } else {
        debug!(
            "Processed {} function name(s): {:?}",
            processed_names.len(),
            processed_names
        );
        Ok(processed_names)
    }
}

async fn update_code(
    lambda_client: aws_sdk_lambda::Client,
    function_name: String,
    bucket: String,
    key: String,
) -> Result<()> {
    debug!("Update Function Code: {function_name} <-- {bucket}:{key}");

    lambda_client
        .update_function_code()
        .function_name(&function_name)
        .s3_bucket(&bucket)
        .s3_key(&key)
        .send()
        .await?;

    info!("Update Function Code Succeeded: {function_name} <-- {bucket}:{key}");

    Ok(())
}

/// Main function to process S3 events and update Lambda functions.
///
/// This function:
/// 1. Extracts the AWS region from event records
/// 2. Creates AWS SDK clients for S3 and Lambda
/// 3. For each S3 record, determines function names and updates Lambda functions concurrently
///
/// # Arguments
/// * `event` - S3 event containing records of uploaded objects
///
/// # Returns
/// * `Result<()>` - Success if all Lambda functions are updated successfully
///
/// # Errors
/// * Returns error if region extraction fails, AWS operations fail, or function name processing fails
pub async fn update(event: S3Event) -> Result<()> {
    debug!("Event: {event:?}");

    let aws_config = ConfigLoader::default()
        .region(Region::new(get_region(&event.records)?))
        .load()
        .await;

    let s3_client = aws_sdk_s3::Client::new(&aws_config);
    let lambda_client = aws_sdk_lambda::Client::new(&aws_config);

    let mut update_tasks = Vec::new();

    for record in event.records {
        debug!("Record: {record:?}");

        let bucket = record
            .s3
            .bucket
            .name
            .as_ref()
            .ok_or_else(|| anyhow!("Bucket not found in {record:?}"))?;

        let key = record
            .s3
            .object
            .key
            .as_ref()
            .ok_or_else(|| anyhow!("Key not found in {record:?}"))?;

        let function_names = get_function_names_from_md(&s3_client, bucket, key).await;
        let function_names = get_function_names(function_names, key)?;
        let processed_names = process_function_names(&function_names)?;

        for function_name in processed_names {
            update_tasks.push((function_name, bucket.clone(), key.clone()));
        }
    }

    let mut update_code_futures = Vec::with_capacity(update_tasks.len());
    for (function_name, bucket, key) in update_tasks {
        update_code_futures.push(tokio::spawn(update_code(
            lambda_client.clone(),
            function_name,
            bucket,
            key,
        )));
    }

    debug!("{} function(s) to update", update_code_futures.len());
    try_join_all(update_code_futures).await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::Error;
    use aws_lambda_events::s3::{S3Bucket, S3Entity, S3EventRecord, S3Object};
    use std::collections::HashMap;

    const TEST_EVENT: &str = r#"{"Records":[{"eventVersion":"2.0","eventSource":"aws:s3","awsRegion":"us-west-2","eventTime":"1970-01-01T00:00:00.000Z","eventName":"ObjectCreated:Put","userIdentity":{"principalId":"EXAMPLE"},"requestParameters":{"sourceIPAddress":"127.0.0.1"},"responseElements":{"x-amz-request-id":"EXAMPLE123456789","x-amz-id-2":"EXAMPLE123/5678abcdefghijklambdaisawesome/mnopqrstuvwxyzABCDEFGH"},"s3":{"s3SchemaVersion":"1.0","configurationId":"testConfigRule","bucket":{"name":"my-s3-bucket","ownerIdentity":{"principalId":"EXAMPLE"},"arn":"arn:aws:s3:::example-bucket"},"object":{"key":"HappyFace.jpg","size":1024,"eTag":"0123456789abcdef0123456789abcdef","sequencer":"0A1B2C3D4E5F678901"}}}]}"#;

    fn test_record(region: &str, bucket: &str, key: &str) -> S3EventRecord {
        S3EventRecord {
            aws_region: Some(region.to_string()),
            s3: S3Entity {
                bucket: S3Bucket {
                    name: Some(bucket.to_string()),
                    ..Default::default()
                },
                object: S3Object {
                    key: Some(key.to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_deserialize() -> Result<()> {
        let event: S3Event = serde_json::from_str(TEST_EVENT)?;

        assert_eq!(1, event.records.len());

        let record = &event.records[0];
        assert_eq!(
            "us-west-2",
            record.aws_region.as_ref().expect("region not found")
        );
        assert_eq!(
            "my-s3-bucket",
            record.s3.bucket.name.as_ref().expect("bucket not found")
        );
        assert_eq!(
            "HappyFace.jpg",
            record.s3.object.key.as_ref().expect("key not found")
        );

        Ok(())
    }

    #[test]
    fn test_get_region() -> Result<()> {
        let mut records = vec![test_record("us-east-1", "foo", "bar")];

        assert_eq!("us-east-1", get_region(&records)?);

        records.push(test_record("us-east-1", "baz", "quux"));

        assert_eq!("us-east-1", get_region(&records)?);

        Ok(())
    }

    #[test]
    fn test_get_region_none() {
        let records = Vec::new();

        let res = get_region(&records);
        assert!(res.is_err());
        if let Err(e) = res {
            assert!(e.to_string().contains("Invalid region count"));
        }
    }

    #[test]
    fn test_get_region_multiple() {
        let records = vec![
            test_record("us-east-1", "foo", "bar"),
            test_record("us-east-2", "baz", "quux"),
        ];

        let res = get_region(&records);
        assert!(res.is_err());
        if let Err(e) = res {
            assert!(e.to_string().contains("Invalid region count"));
        }
    }

    #[test]
    fn test_get_function_names_from_md() -> Result<()> {
        let function_names = get_function_names(Some("foo,bar"), "bar")?;

        assert_eq!("foo,bar", function_names);

        Ok(())
    }

    #[test]
    fn test_get_function_names_from_key() -> Result<()> {
        let function_names = get_function_names(None::<&str>, "bar.zip")?;

        assert_eq!("bar", function_names);

        Ok(())
    }

    #[test]
    fn test_get_function_names_from_unzipped_key() {
        let res = get_function_names(None::<&str>, "bar");

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
        if let Some(fn_names_from_output) = fn_names_from_output {
            assert_eq!(fn_names, fn_names_from_output);
        }
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

    #[test]
    fn test_process_function_names_single() -> Result<()> {
        let processed = process_function_names("my-function")?;
        assert_eq!(vec!["my-function"], processed);
        Ok(())
    }

    #[test]
    fn test_process_function_names_multiple() -> Result<()> {
        let processed = process_function_names("func1,func2,func3")?;
        assert_eq!(vec!["func1", "func2", "func3"], processed);
        Ok(())
    }

    #[test]
    fn test_process_function_names_with_whitespace() -> Result<()> {
        let processed = process_function_names(" func1 , func2 , func3 ")?;
        assert_eq!(vec!["func1", "func2", "func3"], processed);
        Ok(())
    }

    #[test]
    fn test_process_function_names_empty_names() {
        let result = process_function_names(",, ,");
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("No valid function names found"));
        }
    }

    #[test]
    fn test_process_function_names_mixed() -> Result<()> {
        let processed = process_function_names("func1,,func2, ,func3")?;
        assert_eq!(vec!["func1", "func2", "func3"], processed);
        Ok(())
    }
}
