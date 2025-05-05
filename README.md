# LambdUpdate

[![Status Badge](https://github.com/jluszcz/LambdUpdate/actions/workflows/build-and-deploy.yml/badge.svg)](https://github.com/jluszcz/LambdUpdate/actions/workflows/build-and-deploy.yml)

LambdUpdate is a [Terraform](https://www.terraform.io) template and Rust Lambda which updates AWS
[Lambda](https://aws.amazon.com/lambda/) functions when S3 code is uploaded to a code bucket.

## Usage

- Set environment variables for Terraform

``` bash
export TF_VAR_aws_region="us-east-1"
export TF_VAR_aws_acct_id="123412341234"
export TF_VAR_code_bucket="my-code-bucket"
```

- Run Terraform apply: `terraform apply`

- Upload updated code to your S3 code bucket.
    - Include `function.names` with a comma-separated list of one or more function names in your code object's metadata, and
      LambdUpdate will update each of those functions. This is useful if you have multiple functions that share code.
    - If you do not include `function.names` object metadata, LambdUpdate will take the function name from the object's key,
      stripping the `.zip` extension.

``` bash
aws s3 cp --metadata 'function.names="lambdupdate-alt-1,lambdupdate-alt-2"' lambdupdate.zip s3://my-code-bucket/
# OR
aws s3 cp --metadata lambdupdate.zip s3://my-code-bucket/
```
