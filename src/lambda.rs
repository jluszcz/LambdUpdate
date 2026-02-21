use jluszcz_rust_utils::lambda;
use lambda_runtime::{LambdaEvent, service_fn};
use lambdupdate::{APP_NAME, update};
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    let func = service_fn(function);
    lambda_runtime::run(func).await?;
    Ok(())
}

async fn function(event: LambdaEvent<Value>) -> Result<Value, lambda_runtime::Error> {
    lambda::init(APP_NAME, module_path!(), false).await?;

    update(serde_json::from_value(event.payload)?).await?;

    Ok(json!({}))
}
