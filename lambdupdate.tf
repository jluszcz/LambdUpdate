terraform {
  backend "s3" {}
}

# Sourced from environment variables named TF_VAR_${VAR_NAME}
variable "aws_acct_id" {}

variable "aws_region" {}

variable "code_bucket" {}

provider "aws" {
  region = var.aws_region
}

data "aws_s3_bucket" "code_bucket" {
  bucket = var.code_bucket
}

resource "aws_cloudwatch_log_group" "lambdupdate" {
  name              = "/aws/lambda/lambdupdate"
  retention_in_days = "7"
}

data "aws_iam_policy_document" "lambda_assume_role" {
  statement {
    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }
    actions = ["sts:AssumeRole"]
  }
}

resource "aws_iam_role" "lambdupdate" {
  name               = "lambdupdate.lambda.${var.aws_region}"
  assume_role_policy = data.aws_iam_policy_document.lambda_assume_role.json
}

data "aws_iam_policy_document" "cw_logs" {
  statement {
    actions   = ["logs:CreateLogGroup", "logs:CreateLogStream", "logs:PutLogEvents", "logs:Describe*"]
    resources = ["arn:aws:logs:${var.aws_region}:${var.aws_acct_id}:*"]
  }
}

resource "aws_iam_policy" "cw_logs" {
  name   = "lambdupdate.cw_logs.${var.aws_region}"
  policy = data.aws_iam_policy_document.cw_logs.json
}

resource "aws_iam_role_policy_attachment" "cw_logs" {
  role       = aws_iam_role.lambdupdate.name
  policy_arn = aws_iam_policy.cw_logs.arn
}

data "aws_iam_policy_document" "lambda" {
  statement {
    actions   = ["lambda:UpdateFunctionCode"]
    resources = ["*"]
  }
}

resource "aws_iam_policy" "lambda" {
  name   = "lambdupdate.lambda.${var.aws_region}"
  policy = data.aws_iam_policy_document.lambda.json
}

resource "aws_iam_role_policy_attachment" "lambda" {
  role       = aws_iam_role.lambdupdate.name
  policy_arn = aws_iam_policy.lambda.arn
}

data "aws_iam_policy_document" "s3" {
  statement {
    actions   = ["s3:GetObject"]
    resources = ["${data.aws_s3_bucket.code_bucket.arn}/*"]
  }
}

resource "aws_iam_policy" "s3" {
  name   = "lambdupdate.s3.${var.code_bucket}"
  policy = data.aws_iam_policy_document.s3.json
}

resource "aws_iam_role_policy_attachment" "s3" {
  role       = aws_iam_role.lambdupdate.name
  policy_arn = aws_iam_policy.s3.arn
}

resource "aws_s3_bucket_notification" "notification" {
  bucket = data.aws_s3_bucket.code_bucket.id

  lambda_function {
    lambda_function_arn = aws_lambda_function.lambdupdate.arn
    events              = ["s3:ObjectCreated:*"]
    filter_suffix       = ".zip"
  }
}

resource "aws_lambda_permission" "allow_bucket" {
  statement_id  = "lambdupdate-allow-exec-from-s3"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.lambdupdate.arn
  principal     = "s3.amazonaws.com"
  source_arn    = data.aws_s3_bucket.code_bucket.arn
}

resource "aws_lambda_function" "lambdupdate" {
  function_name = "lambdupdate"
  s3_bucket     = var.code_bucket
  s3_key        = "lambdupdate.zip"
  role          = aws_iam_role.lambdupdate.arn
  architectures = ["arm64"]
  runtime       = "provided.al2023"
  handler       = "ignored"
  publish       = "false"
  description   = "Update Lambdas from code in ${var.code_bucket}"
  timeout       = 5
  memory_size   = 128
}

resource "aws_iam_openid_connect_provider" "github" {
  url = "https://token.actions.githubusercontent.com"

  client_id_list = ["sts.amazonaws.com"]

  thumbprint_list = ["6938fd4d98bab03faadb97b34396831e3780aea1"]
}

data "aws_iam_policy_document" "github" {
  statement {
    actions = ["s3:PutObject"]
    resources = ["${data.aws_s3_bucket.code_bucket.arn}/lambdupdate.zip"]
  }
}

resource "aws_iam_policy" "github" {
  name   = "lambdupdate.github.${var.aws_region}"
  policy = data.aws_iam_policy_document.github.json
}

resource "aws_iam_role" "github" {
  name = "lambdupdate.github.${var.aws_region}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17",
    Statement = [
      {
        Effect = "Allow",
        Principal = {
          Federated = aws_iam_openid_connect_provider.github.arn
        },
        Action = "sts:AssumeRoleWithWebIdentity",
        Condition = {
          StringEquals = {
            "token.actions.githubusercontent.com:aud" : "sts.amazonaws.com"
          }
          StringLike = {
            "token.actions.githubusercontent.com:sub" : "repo:jluszcz/LambdUpdate:*"
          },
        }
      }
    ]
  })
}

resource "aws_iam_role_policy_attachment" "github" {
  role       = aws_iam_role.github.name
  policy_arn = aws_iam_policy.github.arn
}
