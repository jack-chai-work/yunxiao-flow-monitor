use serde::Serialize;

/// 应用内统一错误类型。
///
/// Tauri 的 command 要求错误类型可序列化，这里手动实现 `Serialize`，
/// 把错误同时以「机器可读 code + 人类可读 message」的形式送到前端。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("尚未配置云效访问令牌或组织 ID，请先打开「设置」完成配置")]
    NotConfigured,

    #[error("云效接口返回错误（HTTP {status}）：{body}")]
    Http { status: u16, body: String },

    #[error("鉴权失败，请检查个人访问令牌是否有效或已过期（HTTP {status}）")]
    Unauthorized { status: u16 },

    #[error("没有访问该资源的权限，请确认令牌已勾选「流水线」相关权限（HTTP {status}）")]
    Forbidden { status: u16 },

    #[error("网络请求失败：{0}")]
    Transport(String),

    #[error("响应解析失败：{0}")]
    Parse(String),

    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// 机器可读错误码，前端可据此做差异化提示。
    pub fn code(&self) -> &'static str {
        match self {
            AppError::NotConfigured => "NOT_CONFIGURED",
            AppError::Http { .. } => "HTTP_ERROR",
            AppError::Unauthorized { .. } => "UNAUTHORIZED",
            AppError::Forbidden { .. } => "FORBIDDEN",
            AppError::Transport(_) => "TRANSPORT_ERROR",
            AppError::Parse(_) => "PARSE_ERROR",
            AppError::Other(_) => "ERROR",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// 从任意错误快速构造 `AppError::Other`。
pub fn other<E: std::fmt::Display>(e: E) -> AppError {
    AppError::Other(e.to_string())
}
