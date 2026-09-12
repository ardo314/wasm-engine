//! Serves `registration` and `discovery` over the subjects `wasm-protocol`
//! derives for them.

use std::sync::Arc;

use futures::StreamExt;
use rmpv::Value;
use wasm_protocol::{ErrorCode, InterfaceId, Reply, Request, Subject, WireError};

use crate::types::{Provider, RegistryError};
use crate::{DISCOVERY_INTERFACE, REGISTRATION_INTERFACE, Registry, wire};

/// Answers registry calls until the connection drops.
pub async fn serve(client: async_nats::Client, registry: Registry) -> anyhow::Result<()> {
    let registration: InterfaceId = REGISTRATION_INTERFACE.parse()?;
    let discovery: InterfaceId = DISCOVERY_INTERFACE.parse()?;

    // Replicas of registryd share a queue group, so each call is answered once.
    let mut calls = futures::stream::select(
        client
            .queue_subscribe(
                Subject::interface_wildcard(&registration),
                Subject::queue_group(&registration),
            )
            .await?,
        client
            .queue_subscribe(
                Subject::interface_wildcard(&discovery),
                Subject::queue_group(&discovery),
            )
            .await?,
    );

    let registry = Arc::new(registry);
    while let Some(message) = calls.next().await {
        let client = client.clone();
        let registry = Arc::clone(&registry);
        tokio::spawn(async move { respond(&client, &registry, message).await });
    }
    Ok(())
}

async fn respond(client: &async_nats::Client, registry: &Registry, message: async_nats::Message) {
    let Some(reply_to) = message.reply.clone() else {
        return;
    };

    let (id, outcome) = match Request::decode(&message.payload) {
        Ok(request) => {
            let id = request.id.clone();
            (id, call(registry, &message.subject, &request).await)
        }
        Err(e) => (
            String::new(),
            Err(WireError::new(
                ErrorCode::BadRequest,
                format!("undecodable request: {e}"),
            )),
        ),
    };

    let reply = match outcome {
        Ok(results) => Reply::ok(id, results),
        Err(error) => Reply::err(id, error),
    };
    if let Ok(bytes) = reply.encode() {
        let _ = client.publish(reply_to, bytes.into()).await;
    }
}

async fn call(
    registry: &Registry,
    subject: &str,
    request: &Request,
) -> Result<Vec<Value>, WireError> {
    let subject: Subject = subject
        .parse()
        .map_err(|e| bad_request(format!("undecodable subject: {e}")))?;
    if subject.interface().to_string() != request.iface || subject.function() != request.func {
        return Err(bad_request(
            "envelope does not describe the subject it arrived on",
        ));
    }

    let result = match (request.iface.as_str(), request.func.as_str()) {
        (REGISTRATION_INTERFACE, "register") => {
            let [provider] = args(request)?;
            outcome(register(registry, provider).await)
        }
        (REGISTRATION_INTERFACE, "deregister") => {
            let [id] = args(request)?;
            outcome(deregister(registry, id).await)
        }
        (REGISTRATION_INTERFACE, "heartbeat") => {
            let [id] = args(request)?;
            outcome(heartbeat(registry, id).await)
        }
        (DISCOVERY_INTERFACE, "resolve") => {
            let [name, version_req] = args(request)?;
            outcome(resolve(registry, name, version_req).await)
        }
        (DISCOVERY_INTERFACE, "list-interfaces") => {
            let [] = args(request)?;
            outcome(list_interfaces(registry).await)
        }
        (iface, func) => {
            return Err(WireError::new(
                ErrorCode::NotFound,
                format!("the registry does not serve `{iface}` `{func}`"),
            ));
        }
    };
    Ok(vec![result])
}

async fn register(registry: &Registry, provider: &Value) -> Result<Value, RegistryError> {
    registry
        .register(Provider::from_value(provider)?)
        .await
        .map(|()| Value::Nil)
}

async fn deregister(registry: &Registry, id: &Value) -> Result<Value, RegistryError> {
    registry
        .deregister(&wire::string(id, "id")?)
        .await
        .map(|()| Value::Nil)
}

async fn heartbeat(registry: &Registry, id: &Value) -> Result<Value, RegistryError> {
    registry
        .heartbeat(&wire::string(id, "id")?)
        .await
        .map(|()| Value::Nil)
}

async fn resolve(
    registry: &Registry,
    name: &Value,
    version_req: &Value,
) -> Result<Value, RegistryError> {
    let providers = registry
        .resolve(
            &wire::string(name, "name")?,
            &wire::string(version_req, "version-req")?,
        )
        .await?;
    Ok(Value::Array(
        providers.iter().map(Provider::to_value).collect(),
    ))
}

async fn list_interfaces(registry: &Registry) -> Result<Value, RegistryError> {
    let interfaces = registry.list_interfaces().await?;
    Ok(Value::Array(
        interfaces.iter().map(|i| i.to_value()).collect(),
    ))
}

/// A WIT `result` is a successful call whichever arm it carries; only
/// transport and dispatch failures become envelope errors.
fn outcome(result: Result<Value, RegistryError>) -> Value {
    match result {
        Ok(value) => wire::variant("ok", value),
        Err(error) => wire::variant("err", error.to_value()),
    }
}

fn args<const N: usize>(request: &Request) -> Result<&[Value; N], WireError> {
    request.args.as_slice().try_into().map_err(|_| {
        bad_request(format!(
            "`{}` takes {N} arguments, found {}",
            request.func,
            request.args.len()
        ))
    })
}

fn bad_request(message: impl Into<String>) -> WireError {
    WireError::new(ErrorCode::BadRequest, message)
}
