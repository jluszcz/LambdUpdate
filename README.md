# LambdUpdate

[![Build Status](https://app.travis-ci.com/jluszcz/LambdUpdate.svg?branch=main)](https://app.travis-ci.com/jluszcz/LambdUpdate)

LambdUpdate is a [Terraform](https://www.terraform.io) template and Rust program which updates AWS
[Lambda](https://aws.amazon.com/lambda/) functions when S3 code is uploaded to a code bucket.

## Usage

- Set environment variables for Terraform

```
export TF_VAR_aws_region="us-east-1"
export TF_VAR_aws_acct_id="123412341234"
export TF_VAR_code_bucket="my-code-bucket"
```

- Run Terraform apply: `terraform apply`

- Upload updated code to S3, with `function.names` metadata set to a comma-separated list of one or
  more function names to update with the uploaded code.
