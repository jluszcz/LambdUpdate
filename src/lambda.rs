use lambda_runtime::{handler_fn, Context};
use lambdupdate::{set_up_logger, update};
use log::debug;
use serde_json::{json, Value};
use std::error::Error;

type LambdaError = Box<dyn Error + Send + Sync + 'static>;

#[tokio::main]
async fn main() -> Result<(), LambdaError> {
    let func = handler_fn(function);
    lambda_runtime::run(func).await?;
    Ok(())
}

async fn function(event: Value, _: Context) -> Result<Value, LambdaError> {
    set_up_logger(module_path!(), false)?;
    debug!("Processing event: {:?}", event);

    update(serde_json::from_value(event)?).await?;

    Ok(json!({}))
}
