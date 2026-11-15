use serde::Serialize;

/// The gateway takes these four (`ingest-api/src/schema.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Debug,
    Info,
    Warn,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Debug => "debug",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        }
    }

    /// An HTTP status's: 5xx an error, 4xx a warning, the rest info.
    pub fn of_status(status: u16) -> Self {
        match status {
            500.. => Severity::Error,
            400.. => Severity::Warn,
            _ => Severity::Info,
        }
    }
}

/// Who an event is about: the app maps its own signed-in user to this.
#[derive(Debug, Clone, Default)]
pub struct Who {
    pub user_id: String,
    pub user_email: String,
    /// Merged into the event's attributes, e.g. `{"api_key_id": "…"}`.
    pub attributes: Option<serde_json::Value>,
}

impl Who {
    pub fn id(user_id: impl ToString) -> Self {
        Who { user_id: user_id.to_string(), ..Default::default() }
    }
    pub fn email(mut self, email: impl ToString) -> Self {
        self.user_email = email.to_string();
        self
    }
    pub fn attrs(mut self, attributes: serde_json::Value) -> Self {
        self.attributes = Some(attributes);
        self
    }
}

/// Longest message and attributes sent; the rest is cut (a batch must stay under the gateway's
/// 8 MiB).
pub const MAX_TEXT: usize = 16 * 1024;

/// One event, its fields named as the gateway's `events` columns. Empty fields are not sent, and
/// take the column's default.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Event {
    pub ts: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    pub category: String,
    pub event_type: String,
    pub severity: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub user_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub user_email: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub session_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub request_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub entity_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub entity_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error_code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub tokens_input: u32,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub tokens_output: u32,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub duration_ms: u32,
    #[serde(skip_serializing_if = "is_zero_u16")]
    pub http_status: u16,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub route: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub app_version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub server: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ip: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub user_agent: String,
    /// Free-form JSON, as a string (the gateway stores it so; read it with `JSONExtract`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub attributes: String,
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}
fn is_zero_u16(n: &u16) -> bool {
    *n == 0
}

impl Event {
    /// Now, from `source = "backend"`; `server` and `app_version` come from the [`crate::Config`]
    /// when it is sent, unless set here.
    pub fn new(category: impl ToString, event_type: impl ToString, severity: Severity) -> Self {
        Event {
            // DateTime64's own form: a space, no T or Z
            ts: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            source: "backend".to_string(),
            category: category.to_string(),
            event_type: event_type.to_string(),
            severity: severity.as_str(),
            ..Default::default()
        }
    }

    pub fn source(mut self, source: impl ToString) -> Self {
        self.source = source.to_string();
        self
    }
    /// The user, and what [`Who::attrs`] carries.
    pub fn who(mut self, who: &Who) -> Self {
        self.user_id = who.user_id.clone();
        self.user_email = who.user_email.clone();
        if let Some(extra) = &who.attributes {
            self = self.merge_attrs(extra.clone());
        }
        self
    }
    pub fn user_id(mut self, id: impl ToString) -> Self {
        self.user_id = id.to_string();
        self
    }
    pub fn user_email(mut self, email: impl ToString) -> Self {
        self.user_email = email.to_string();
        self
    }
    pub fn session(mut self, id: impl ToString) -> Self {
        self.session_id = id.to_string();
        self
    }
    pub fn request_id(mut self, id: impl ToString) -> Self {
        self.request_id = id.to_string();
        self
    }
    pub fn entity(mut self, entity_type: impl ToString, entity_id: impl ToString) -> Self {
        self.entity_type = entity_type.to_string();
        self.entity_id = entity_id.to_string();
        self
    }
    pub fn msg(mut self, message: impl ToString) -> Self {
        self.message = cap(message.to_string(), MAX_TEXT);
        self
    }
    pub fn error_code(mut self, code: impl ToString) -> Self {
        self.error_code = code.to_string();
        self
    }
    pub fn model(mut self, model: impl ToString) -> Self {
        self.model = model.to_string();
        self
    }
    pub fn tokens(mut self, input: u32, output: u32) -> Self {
        self.tokens_input = input;
        self.tokens_output = output;
        self
    }
    pub fn status(mut self, status: u16) -> Self {
        self.http_status = status;
        self
    }
    pub fn duration(mut self, ms: u32) -> Self {
        self.duration_ms = ms;
        self
    }
    /// An HTTP route or a named operation (`worker:email_send`). For HTTP, the pattern it matched,
    /// not the path: a path can carry a token.
    pub fn route(mut self, route: impl ToString) -> Self {
        self.route = route.to_string();
        self
    }
    pub fn ip(mut self, ip: impl ToString) -> Self {
        self.ip = ip.to_string();
        self
    }
    pub fn user_agent(mut self, ua: impl ToString) -> Self {
        self.user_agent = ua.to_string();
        self
    }
    pub fn app_version(mut self, version: impl ToString) -> Self {
        self.app_version = version.to_string();
        self
    }
    pub fn server(mut self, server: impl ToString) -> Self {
        self.server = server.to_string();
        self
    }
    /// Free-form JSON; replaces what was there.
    pub fn attrs(mut self, attributes: serde_json::Value) -> Self {
        self.attributes = cap(attributes.to_string(), MAX_TEXT);
        self
    }
    /// Adds an object's keys to the attributes there are.
    pub fn merge_attrs(self, extra: serde_json::Value) -> Self {
        let mut current: serde_json::Value =
            serde_json::from_str(&self.attributes).unwrap_or_else(|_| serde_json::json!({}));
        match (current.as_object_mut(), extra) {
            (Some(map), serde_json::Value::Object(more)) => {
                map.extend(more);
                self.attrs(current)
            }
            (_, other) => self.attrs(other),
        }
    }

    /// Queues it to be sent. Never blocks, never fails; nothing when telemetry is off.
    pub fn emit(self) {
        crate::sender::enqueue(self);
    }
}

/// Cut to at most `max` bytes, on a character's edge.
pub fn cap(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_fields_are_not_sent() {
        let e = Event::new("http", "GET", Severity::Info).status(200);
        let v = serde_json::to_value(&e).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["category", "event_type", "http_status", "severity", "source", "ts"]);
    }

    #[test]
    fn who_and_attributes_merge() {
        let who = Who::id("u1").email("a@b.c").attrs(serde_json::json!({ "api_key_id": "k" }));
        let e = Event::new("x", "y", Severity::Warn)
            .attrs(serde_json::json!({ "a": 1 }))
            .who(&who);
        assert_eq!(e.user_id, "u1");
        assert_eq!(e.user_email, "a@b.c");
        let attrs: serde_json::Value = serde_json::from_str(&e.attributes).unwrap();
        assert_eq!(attrs, serde_json::json!({ "a": 1, "api_key_id": "k" }));
    }

    #[test]
    fn cap_cuts_on_a_character_edge() {
        assert_eq!(cap("abc".into(), 5), "abc");
        assert_eq!(cap("àèì".into(), 3), "à");
    }

    #[test]
    fn severity_of_status() {
        assert_eq!(Severity::of_status(200), Severity::Info);
        assert_eq!(Severity::of_status(404), Severity::Warn);
        assert_eq!(Severity::of_status(503), Severity::Error);
    }
}
