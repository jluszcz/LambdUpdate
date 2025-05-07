use lambda_runtime::{LambdaEvent, service_fn};
use lambda_utils::{emit_rustc_metric, set_up_logger};
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
    set_up_logger(APP_NAME, module_path!(), false)?;
    emit_rustc_metric(APP_NAME).await;

    update(serde_json::from_value(event.payload)?).await?;

    Ok(json!({}))
}
