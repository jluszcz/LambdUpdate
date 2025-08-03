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

The core update flow (`src/lib.rs:102-149`):

1. Extract AWS region from S3 event records
2. Create AWS SDK clients for S3 and Lambda
3. For each S3 record, determine target function names
4. Concurrently update all target Lambda functions using `aws_sdk_lambda::Client::update_function_code()`

## Development Commands

### Build and Test

- `cargo build` - Build the project
- `cargo fmt` - Format the source code
- `cargo test` - Run all tests
- `cargo check` - Check for compilation errors without building
- `cargo clippy -- -D warnings` - Run Rust linter for code quality checks

### Making Changes

After making any changes, run the build/test commands above and make sure they pass, correcting any errors.

When fixing test failures, you MUST fix the test rather than remove tests. When in doubt, ask.

Before committing code, run `pre-commit run` to verify that no pre-commit hooks will fail.

## Deployment

### Terraform Setup

Set required environment variables:

```bash
export TF_VAR_aws_region="us-east-1"
export TF_VAR_aws_acct_id="123412341234"
export TF_VAR_code_bucket="my-code-bucket"
```

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
cargo test test_get_function_names_from_md
```
