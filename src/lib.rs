//! LambdUpdate library for automatically updating AWS Lambda functions from S3 events.
//!
//! This library processes S3 events triggered when ZIP files are uploaded to a code bucket,
//! extracts function names from object metadata or keys, and updates the corresponding Lambda functions.

use anyhow::{Context, Result, anyhow};
use aws_config::ConfigLoader;
use aws_lambda_events::s3::{S3Event, S3EventRecord};
use aws_sdk_lambda::config::Region;
use aws_sdk_lambda::operation::update_function_code::UpdateFunctionCodeError;
use chrono::Utc;
use futures::future::try_join_all;
use log::{debug, info, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

pub const APP_NAME: &str = "lambdupdate";

const FUNCTION_NAME_MD_KEY: &str = "function.names";

const MAX_UPDATE_ATTEMPTS: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 500;

/// Creates an S3EventRecord for testing or CLI usage.
///
/// This function constructs a minimal S3EventRecord with all required fields
/// using JSON deserialization, since S3Event structs are non-exhaustive in
/// aws_lambda_events 1.0+. The event time is set to the current UTC time.
///
/// # Arguments
/// * `region` - AWS region (e.g., "us-east-1")
/// * `bucket` - S3 bucket name
/// * `key` - S3 object key
///
/// # Returns
/// * `S3EventRecord` - A valid S3EventRecord with the specified parameters
///
/// # Panics
/// * Panics if JSON deserialization fails (should not happen with valid inputs)
pub fn create_s3_event_record(region: &str, bucket: &str, key: &str) -> S3EventRecord {
    let event_time = Utc::now().to_rfc3339();

    let json = serde_json::json!({
        "eventVersion": "2.0",
        "eventSource": "aws:s3",
        "awsRegion": region,
        "eventTime": event_time,
        "eventName": "ObjectCreated:Put",
        "userIdentity": {
            "principalId": "EXAMPLE"
        },
        "requestParameters": {
            "sourceIPAddress": "127.0.0.1"
        },
        "responseElements": {
            "x-amz-request-id": "EXAMPLE123456789",
            "x-amz-id-2": "EXAMPLE123/5678"
        },
        "s3": {
            "s3SchemaVersion": "1.0",
            "bucket": {
                "name": bucket
            },
            "object": {
                "key": key
            }
        }
    });

    serde_json::from_value(json).expect("Failed to construct S3EventRecord from JSON")
}

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

/// Fetches function names from the object's S3 metadata.
///
/// Returns `Ok(Some(_))` if the object's metadata contains the function-names key,
/// `Ok(None)` if the head succeeds but the key is absent, and `Err(_)` if HeadObject
/// itself fails — callers must treat that as a real error rather than silently
/// falling back, since it usually indicates misconfigured IAM or a missing object.
async fn get_function_names_from_md(
    s3_client: &aws_sdk_s3::Client,
    bucket: &str,
    key: &str,
) -> Result<Option<String>> {
    debug!("Head Object: {bucket}:{key}");
    let head_object_output = s3_client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .with_context(|| format!("HeadObject failed for {bucket}:{key}"))?;

    info!("Head Object Succeeded: {bucket}:{key}");
    debug!("Object Metadata: {:?}", head_object_output.metadata);

    Ok(extract_function_names_from_metadata(
        head_object_output.metadata,
    ))
}

fn extract_function_names_from_metadata(
    metadata: Option<HashMap<String, String>>,
) -> Option<String> {
    metadata.and_then(|m| m.get(FUNCTION_NAME_MD_KEY).cloned())
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
fn get_function_names(function_names_from_md: Option<String>, key: &str) -> Result<String> {
    match function_names_from_md {
        Some(function_names) => {
            debug!("Function names from object metadata: {function_names}");
            Ok(function_names)
        }
        None => {
            let function_name = key
                .strip_suffix(".zip")
                .ok_or_else(|| anyhow!("'.zip' not found in object key: {key}"))?;

            debug!("Function name from object key: {function_name}");
            Ok(function_name.to_string())
        }
    }
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

fn collect_update_tasks(
    function_names_md: Option<String>,
    bucket: Arc<str>,
    key: Arc<str>,
    update_tasks: &mut HashSet<(String, Arc<str>, Arc<str>)>,
) -> Result<()> {
    let function_names = get_function_names(function_names_md, &key)?;
    let processed_names = process_function_names(&function_names)?;

    for function_name in processed_names {
        update_tasks.insert((function_name, Arc::clone(&bucket), Arc::clone(&key)));
    }

    Ok(())
}

fn is_conflict(err: Option<&UpdateFunctionCodeError>) -> bool {
    matches!(
        err,
        Some(UpdateFunctionCodeError::ResourceConflictException(_))
    )
}

async fn update_code(
    lambda_client: aws_sdk_lambda::Client,
    function_name: String,
    bucket: Arc<str>,
    key: Arc<str>,
) -> Result<()> {
    debug!("Update Function Code: {function_name} <-- {bucket}:{key}");

    let mut attempt = 0u32;
    let final_err = loop {
        attempt += 1;

        let result = lambda_client
            .update_function_code()
            .function_name(&function_name)
            .s3_bucket(bucket.as_ref())
            .s3_key(key.as_ref())
            .send()
            .await;

        match result {
            Ok(_) => {
                info!("Update Function Code Succeeded: {function_name} <-- {bucket}:{key}");
                return Ok(());
            }
            Err(err) => {
                if !is_conflict(err.as_service_error()) || attempt >= MAX_UPDATE_ATTEMPTS {
                    break err;
                }

                let backoff_ms = INITIAL_BACKOFF_MS * (1u64 << (attempt - 1));
                warn!(
                    "ResourceConflictException updating {function_name} (attempt {attempt}/{MAX_UPDATE_ATTEMPTS}); retrying in {backoff_ms}ms"
                );
                sleep(Duration::from_millis(backoff_ms)).await;
            }
        }
    };

    Err(final_err).with_context(|| format!("UpdateFunctionCode failed for {function_name}"))
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

    let mut update_tasks: HashSet<(String, Arc<str>, Arc<str>)> = HashSet::new();

    for record in event.records {
        debug!("Record: {record:?}");

        let bucket: Arc<str> = Arc::from(
            record
                .s3
                .bucket
                .name
                .as_deref()
                .ok_or_else(|| anyhow!("Bucket not found in {record:?}"))?,
        );

        let key: Arc<str> = Arc::from(
            record
                .s3
                .object
                .key
                .as_deref()
                .ok_or_else(|| anyhow!("Key not found in {record:?}"))?,
        );

        let function_names_md = get_function_names_from_md(&s3_client, &bucket, &key).await?;
        collect_update_tasks(function_names_md, bucket, key, &mut update_tasks)?;
    }

    debug!("{} function(s) to update", update_tasks.len());

    let update_futures = update_tasks
        .into_iter()
        .map(|(function_name, bucket, key)| {
            update_code(lambda_client.clone(), function_name, bucket, key)
        });

    try_join_all(update_futures).await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use aws_lambda_events::s3::S3EventRecord;
    use std::collections::HashMap;

    const TEST_EVENT: &str = r#"{"Records":[{"eventVersion":"2.0","eventSource":"aws:s3","awsRegion":"us-west-2","eventTime":"1970-01-01T00:00:00.000Z","eventName":"ObjectCreated:Put","userIdentity":{"principalId":"EXAMPLE"},"requestParameters":{"sourceIPAddress":"127.0.0.1"},"responseElements":{"x-amz-request-id":"EXAMPLE123456789","x-amz-id-2":"EXAMPLE123/5678abcdefghijklambdaisawesome/mnopqrstuvwxyzABCDEFGH"},"s3":{"s3SchemaVersion":"1.0","configurationId":"testConfigRule","bucket":{"name":"my-s3-bucket","ownerIdentity":{"principalId":"EXAMPLE"},"arn":"arn:aws:s3:::example-bucket"},"object":{"key":"HappyFace.jpg","size":1024,"eTag":"0123456789abcdef0123456789abcdef","sequencer":"0A1B2C3D4E5F678901"}}}]}"#;

    fn test_record(region: &str, bucket: &str, key: &str) -> S3EventRecord {
        create_s3_event_record(region, bucket, key)
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
    fn test_get_function_names_when_md_provided() -> Result<()> {
        let function_names = get_function_names(Some("foo,bar".to_string()), "bar")?;

        assert_eq!("foo,bar", function_names);

        Ok(())
    }

    #[test]
    fn test_get_function_names_from_key() -> Result<()> {
        let function_names = get_function_names(None, "bar.zip")?;

        assert_eq!("bar", function_names);

        Ok(())
    }

    #[test]
    fn test_get_function_names_from_unzipped_key() {
        let res = get_function_names(None, "bar");

        assert!(res.is_err());
        if let Err(e) = res {
            assert!(e.to_string().contains("'.zip' not found"));
        }
    }

    #[test]
    fn test_extract_function_names_present() {
        let mut md = HashMap::new();
        md.insert(FUNCTION_NAME_MD_KEY.to_string(), "foo,bar".to_string());

        assert_eq!(
            Some("foo,bar".to_string()),
            extract_function_names_from_metadata(Some(md))
        );
    }

    #[test]
    fn test_extract_function_names_no_metadata() {
        assert_eq!(None, extract_function_names_from_metadata(None));
    }

    #[test]
    fn test_extract_function_names_key_absent() {
        assert_eq!(
            None,
            extract_function_names_from_metadata(Some(HashMap::new()))
        );
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

    #[test]
    fn test_is_conflict_true() {
        let err = UpdateFunctionCodeError::ResourceConflictException(
            aws_sdk_lambda::types::error::ResourceConflictException::builder()
                .message("update in progress")
                .build(),
        );
        assert!(is_conflict(Some(&err)));
    }

    #[test]
    fn test_is_conflict_false_other_variant() {
        let err = UpdateFunctionCodeError::TooManyRequestsException(
            aws_sdk_lambda::types::error::TooManyRequestsException::builder()
                .message("throttled")
                .build(),
        );
        assert!(!is_conflict(Some(&err)));
    }

    #[test]
    fn test_is_conflict_false_none() {
        assert!(!is_conflict(None));
    }

    #[test]
    fn test_collect_update_tasks_dedups_identical_records() -> Result<()> {
        let bucket: Arc<str> = Arc::from("b");
        let key: Arc<str> = Arc::from("func.zip");
        let mut tasks = HashSet::new();

        collect_update_tasks(None, Arc::clone(&bucket), Arc::clone(&key), &mut tasks)?;
        collect_update_tasks(None, bucket, key, &mut tasks)?;

        assert_eq!(1, tasks.len());
        Ok(())
    }

    #[test]
    fn test_collect_update_tasks_keeps_distinct_functions() -> Result<()> {
        let bucket: Arc<str> = Arc::from("b");
        let key: Arc<str> = Arc::from("shared.zip");
        let mut tasks = HashSet::new();

        collect_update_tasks(
            Some("a,b,c".to_string()),
            Arc::clone(&bucket),
            Arc::clone(&key),
            &mut tasks,
        )?;

        assert_eq!(3, tasks.len());
        Ok(())
    }

    #[test]
    fn test_collect_update_tasks_dedups_across_metadata_and_key() -> Result<()> {
        let bucket: Arc<str> = Arc::from("b");
        let key: Arc<str> = Arc::from("foo.zip");
        let mut tasks = HashSet::new();

        collect_update_tasks(
            Some("foo".to_string()),
            Arc::clone(&bucket),
            Arc::clone(&key),
            &mut tasks,
        )?;
        collect_update_tasks(None, bucket, key, &mut tasks)?;

        assert_eq!(1, tasks.len());
        Ok(())
    }

    #[test]
    fn test_collect_update_tasks_distinct_keys_not_deduped() -> Result<()> {
        let bucket: Arc<str> = Arc::from("b");
        let mut tasks = HashSet::new();

        collect_update_tasks(
            Some("foo".to_string()),
            Arc::clone(&bucket),
            Arc::from("v1.zip"),
            &mut tasks,
        )?;
        collect_update_tasks(
            Some("foo".to_string()),
            bucket,
            Arc::from("v2.zip"),
            &mut tasks,
        )?;

        assert_eq!(2, tasks.len());
        Ok(())
    }
}
