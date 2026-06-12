# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

LambdUpdate is a Rust-based AWS Lambda function that automatically updates other Lambda functions when new code is
uploaded to an S3 bucket. It's deployed via Terraform and includes CI/CD automation through GitHub Actions.

## Architecture

### Core Components

- **`src/lib.rs`**: Main library containing the `update()` function that processes S3 events and updates Lambda
  functions
- **`src/lambda.rs`**: AWS Lambda runtime entry point that handles S3 events from Lambda runtime
- **`src/main.rs`**: CLI entry point for local testing/debugging with command-line arguments
- **`lambdupdate.tf`**: Terraform configuration defining AWS infrastructure including IAM roles, Lambda function, and S3
  event triggers

### Key Functionality

The system processes S3 events triggered when `.zip` files are uploaded to a code bucket. Function names are determined
either from:

1. Object metadata key `function.names` (comma-separated list for shared code)
2. Object key name (strips `.zip` extension)

The core update flow (the `update()` function in `src/lib.rs`):

1. Extract AWS region from S3 event records
2. Create AWS SDK clients for S3 and Lambda
3. For each S3 record, determine target function names and collect deduplicated update tasks
4. Concurrently update all target Lambda functions using `aws_sdk_lambda::Client::update_function_code()`, retrying
   with exponential backoff on `ResourceConflictException` (up to 3 attempts)

## Development Commands

### Build and Test

- `cargo build` - Build the project
- `cargo fmt` - Format the source code
- `cargo test` - Run all tests
- `cargo check` - Check for compilation errors without building
- `cargo clippy -- -D warnings` - Run Rust linter for code quality checks

## Deployment

### Terraform Setup

Terraform state uses per-region workspaces named `lambdupdate_<region>`. Source the appropriate env script to set
`TF_VAR_aws_region` (the only required variable) and select the workspace:

```bash
. env-us_east_1  # or env-us_east_2
```

The AWS account ID and code bucket name are derived in `lambdupdate.tf` from the caller identity and region.

Deploy infrastructure:

```bash
terraform apply
```

### CI/CD

GitHub Actions automatically:

1. Builds and tests on ARM64 runners
2. Packages Lambda binary as `bootstrap`
3. Deploys to multiple AWS regions (us-east-1, us-east-2)

## Dependencies

Key external dependencies:

- **AWS SDK**: `aws-sdk-lambda`, `aws-sdk-s3` for AWS API interactions
- **Lambda Runtime**: `lambda_runtime`, `aws_lambda_events` for Lambda event handling
- **Utilities**: `jluszcz_rust_utils` for logging and Lambda initialization
- **CLI**: `clap` for command-line argument parsing

## Testing

Comprehensive test suite in `src/lib.rs` covers:

- S3 event deserialization
- Region extraction from event records
- Function name resolution from metadata/object keys
- Error handling for invalid inputs

Run specific test:

```bash
cargo test test_get_function_names_from_key
```
