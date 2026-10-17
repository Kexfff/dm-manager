//! Thin helpers over a zbus connection: typed method calls and property maps.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use zbus::zvariant::{DynamicType, OwnedValue, Type, Value};
use zbus::{Connection, MatchRule, MessageStream};

const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

pub(crate) type Emit<E> = Arc<dyn Fn(E) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct Bus {
    pub conn: Connection,
    service: &'static str,
}

impl Bus {
    pub fn new(conn: Connection, service: &'static str) -> Self {
        Self { conn, service }
    }

    pub async fn call<B, R>(&self, path: &str, iface: &str, method: &str, body: &B) -> zbus::Result<R>
    where
        B: Serialize + DynamicType,
        R: DeserializeOwned + Type,
    {
        let reply = self.conn.call_method(Some(self.service), path, Some(iface), method, body).await?;
        reply.body().deserialize()
    }

    /// Calls a method that returns nothing.
    pub async fn invoke<B>(&self, path: &str, iface: &str, method: &str, body: &B) -> zbus::Result<()>
    where
        B: Serialize + DynamicType,
    {
        self.conn.call_method(Some(self.service), path, Some(iface), method, body).await.map(drop)
    }

    pub async fn props(&self, path: &str, iface: &str) -> zbus::Result<Props> {
        self.call(path, PROPERTIES, "GetAll", &(iface,)).await.map(Props)
    }

    pub async fn get(&self, path: &str, iface: &str, name: &str) -> zbus::Result<OwnedValue> {
        self.call(path, PROPERTIES, "Get", &(iface, name)).await
    }

    pub async fn set(&self, path: &str, iface: &str, name: &str, value: Value<'_>) -> zbus::Result<()> {
        self.invoke(path, PROPERTIES, "Set", &(iface, name, value)).await
    }

    /// Every signal from this service, plus its owner changing (the service restarting).
    pub async fn signals(&self) -> zbus::Result<futures_util::stream::Select<MessageStream, MessageStream>> {
        let own = MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(self.service)?.build();
        let owner = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .arg(0, self.service)?
            .build();
        let own = MessageStream::for_match_rule(own, &self.conn, Some(512)).await?;
        let owner = MessageStream::for_match_rule(owner, &self.conn, Some(8)).await?;
        Ok(futures_util::stream::select(own, owner))
    }
}

/// A property map from `GetAll` (or a section of NetworkManager settings).
#[derive(Debug, Default)]
pub(crate) struct Props(pub HashMap<String, OwnedValue>);

impl Props {
    fn value(&self, key: &str) -> Option<&Value<'static>> {
        self.0.get(key).map(|v| &**v)
    }

    pub fn str(&self, key: &str) -> Option<String> {
        match self.value(key)? {
            Value::Str(s) => Some(s.to_string()),
            Value::ObjectPath(p) => Some(p.to_string()),
            _ => None,
        }
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.value(key)? {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn u8(&self, key: &str) -> Option<u8> {
        match self.value(key)? {
            Value::U8(n) => Some(*n),
            _ => None,
        }
    }

    pub fn u32(&self, key: &str) -> Option<u32> {
        match self.value(key)? {
            Value::U32(n) => Some(*n),
            _ => None,
        }
    }

    pub fn i16(&self, key: &str) -> Option<i16> {
        match self.value(key)? {
            Value::I16(n) => Some(*n),
            _ => None,
        }
    }

    pub fn bytes(&self, key: &str) -> Option<Vec<u8>> {
        match self.value(key)? {
            Value::Array(a) => Some(
                a.iter()
                    .filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        }
    }
}

/// A short, human-readable message for a D-Bus error.
pub fn describe_error(e: &zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(name, msg, _) => match msg.as_deref() {
            Some(m) if !m.is_empty() => m.to_string(),
            _ => name.rsplit('.').next().unwrap_or(name.as_str()).to_string(),
        },
        zbus::Error::FDO(e) => match &**e {
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_) => {
                "The service isn't running".into()
            }
            other => other.to_string(),
        },
        other => other.to_string(),
    }
}
