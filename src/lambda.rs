use lambda_runtime::{service_fn, LambdaEvent};
use lambdupdate::{set_up_logger, update};
use log::debug;
use serde_json::{json, Value};
use std::error::Error;

type LambdaError = Box<dyn Error + Send + Sync + 'static>;

#[tokio::main]
async fn main() -> Result<(), LambdaError> {
    let func = service_fn(function);
    lambda_runtime::run(func).await?;
    Ok(())
}

async fn function(event: LambdaEvent<Value>) -> Result<Value, LambdaError> {
    set_up_logger(module_path!(), false)?;
    debug!("Processing event: {:?}", event);

    update(serde_json::from_value(event.payload)?).await?;

    Ok(json!({}))
}
