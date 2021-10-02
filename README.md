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

- Upload updated code to your S3 code bucket.
    - If you have set `function.names` to a comma-separated list of one or more function names,
      then LambdUpdate will update those functions.
    - If you do not set `function.names`, LambdUpdate will take the function name from the object's
      key, stripping the `.zip` extension.

```
aws s3 cp --metadata 'function.names="lambdupdate-alt-1,lambdupdate-alt-2"' lambdupdate.zip s3://my-code-bucket/
# OR
aws s3 cp --metadata lambdupdate.zip s3://my-code-bucket/
```
