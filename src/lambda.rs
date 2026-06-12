use aws_lambda_events::s3::S3Event;
use jluszcz_rust_utils::lambda;
use lambda_runtime::{LambdaEvent, service_fn};
use lambdupdate::{APP_NAME, update};

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda::init(APP_NAME, module_path!(), false).await?;

    let func = service_fn(function);
    lambda_runtime::run(func).await?;
    Ok(())
}

async fn function(event: LambdaEvent<S3Event>) -> Result<(), lambda_runtime::Error> {
    update(event.payload).await?;
    Ok(())
}
