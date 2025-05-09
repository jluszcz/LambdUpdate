use jluszcz_rust_utils::lambda;
use lambda_runtime::{LambdaEvent, service_fn};
use lambdupdate::{APP_NAME, update};
use serde_json::{Value, json};
use std::error::Error;

type LambdaError = Box<dyn Error + Send + Sync + 'static>;

#[tokio::main]
async fn main() -> Result<(), LambdaError> {
    let func = service_fn(function);
    lambda_runtime::run(func).await?;
    Ok(())
}

async fn function(event: LambdaEvent<Value>) -> Result<Value, LambdaError> {
    lambda::init(APP_NAME, module_path!(), false).await?;

    update(serde_json::from_value(event.payload)?).await?;

    Ok(json!({}))
}
