use serde::Serialize;

pub(crate) const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopAction {
    Checkout,
    Restack,
    SubmitStack,
    OpenPr,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProgressEvent<'a> {
    pub(crate) schema_version: u32,
    pub(crate) request_id: &'a str,
    #[serde(rename = "type")]
    pub(crate) event_type: &'static str,
    pub(crate) phase: &'a str,
    pub(crate) message: &'a str,
}

#[derive(Debug, Serialize)]
pub(crate) struct TerminalEvent<'a, T: Serialize> {
    pub(crate) schema_version: u32,
    pub(crate) request_id: &'a str,
    #[serde(rename = "type")]
    pub(crate) event_type: &'static str,
    pub(crate) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<DesktopError>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DesktopError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) details: String,
    pub(crate) recovery: &'static str,
}

impl DesktopError {
    pub(crate) fn unsupported_schema(received: u32) -> Self {
        Self {
            code: "unsupported_schema",
            message: format!("Desktop schema {received} is not supported."),
            details: format!("This engine supports schema {SCHEMA_VERSION}."),
            recovery: "reinstall_app",
        }
    }

    pub(crate) fn operation(
        code: &'static str,
        message: impl Into<String>,
        details: impl Into<String>,
        recovery: &'static str,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details: details.into(),
            recovery,
        }
    }
}

impl<'a, T: Serialize> TerminalEvent<'a, T> {
    pub(crate) fn success(request_id: &'a str, data: T) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id,
            event_type: "result",
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub(crate) fn failure(request_id: &'a str, error: DesktopError) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id,
            event_type: "result",
            ok: false,
            data: None,
            error: Some(error),
        }
    }
}
