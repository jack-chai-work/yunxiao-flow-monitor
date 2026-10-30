use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Client, Method, StatusCode};
use serde_json::Value;

/// 云效流水线（Flow）OpenAPI 客户端 —— 中心版。
///
/// 所有接口都走 `https://{domain}/oapi/v1/flow/organizations/{organizationId}/...`，
/// 鉴权统一使用请求头 `x-yunxiao-token: <个人访问令牌>`。
///
/// 接口清单（对应云效 OpenAPI 名称）：
/// | 方法 | 路径 | OpenAPI |
/// |---|---|---|
/// | GET  | /pipelines | ListPipelines |
/// | GET  | /pipelines/{id} | GetPipeline |
/// | GET  | /pipelines/{id}/runs | ListPipelineRuns |
/// | GET  | /pipelines/{id}/runs/latestPipelineRun | GetLatestPipelineRun |
/// | GET  | /pipelines/{id}/runs/{runId} | GetPipelineRun |
/// | POST | /pipelines/{id}/runs | CreatePipelineRun（运行流水线） |
/// | PUT  | /pipelines/{id}/runs/{runId} | UpdatePipelineRun（终止流水线运行） |
/// | GET  | /pipelines/{id}/pipelineRuns/{runId}/jobs/{jobId}/steps | GetPipelineJobSteps |
/// | GET  | .../jobs/{jobId}/step/log | GetPipelineJobStepLog |
/// | GET  | .../jobs/{jobId}/step/log/url | GetPipelineJobStepLogUrl |
/// | POST | .../jobs/{jobId}/start | ExecutePipelineJobRun（手动运行/重跑任务） |
/// | PUT  | .../jobs/{jobId}/stop | StopPipelineJobRun（终止任务） |
/// | PUT  | .../jobs/{jobId}/skip | SkipPipelineJobRun（跳过任务） |
#[derive(Clone)]
pub struct YunxiaoClient {
    http: Client,
    base_url: String,
    organization_id: String,
    token: String,
}

impl YunxiaoClient {
    /// 配置不完整时返回 None，调用方据此提示用户去设置页。
    pub fn from_config(http: Client, cfg: &AppConfig) -> Option<Self> {
        if !cfg.is_ready() {
            return None;
        }
        Some(Self {
            http,
            base_url: cfg.base_url.clone(),
            organization_id: cfg.organization_id.clone(),
            token: cfg.token.clone(),
        })
    }

    pub fn organization_id(&self) -> &str {
        &self.organization_id
    }

    /// 拼接中心版 flow 路径。
    fn flow(&self, tail: &str) -> String {
        format!(
            "{}/oapi/v1/flow/organizations/{}/{}",
            self.base_url,
            encode(&self.organization_id),
            tail.trim_start_matches('/')
        )
    }

    async fn request(
        &self,
        method: Method,
        url: String,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> AppResult<Value> {
        let mut req = self
            .http
            .request(method, url.as_str())
            .header("x-yunxiao-token", &self.token)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");

        if !query.is_empty() {
            let pairs: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
            req = req.query(&pairs);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await.map_err(|e| {
            AppError::Transport(format!(
                "{e}（接入点：{url}，请检查网络代理与 base_url 是否正确）"
            ))
        })?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(map_http_error(status, &text));
        }

        // 部分接口（如终止运行）直接返回裸 `true`，不是 JSON 对象
        if text.trim().is_empty() {
            return Ok(Value::Bool(true));
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => Ok(v),
            Err(e) => {
                // GetPipelineJobStepLogUrl 直接返回纯文本下载链接（Content-Type 却是 JSON），
                // 这种情况包成字符串返回，而不是当作解析失败。
                let trimmed = text.trim();
                if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                    Ok(Value::String(trimmed.to_string()))
                } else {
                    Err(AppError::Parse(format!(
                        "{e}；原始响应片段：{}",
                        truncate(&text, 300)
                    )))
                }
            }
        }
    }

    async fn get(&self, url: String, query: &[(&str, String)]) -> AppResult<Value> {
        self.request(Method::GET, url, query, None).await
    }

    async fn post(&self, url: String, body: Option<Value>) -> AppResult<Value> {
        self.request(Method::POST, url, &[], body).await
    }

    async fn put(&self, url: String, body: Option<Value>) -> AppResult<Value> {
        self.request(Method::PUT, url, &[], body).await
    }

    // -----------------------------------------------------------------------
    // 只读接口
    // -----------------------------------------------------------------------

    /// ListPipelines：获取流水线列表。返回原始 JSON 数组。
    pub async fn list_pipelines_raw(
        &self,
        page: u32,
        per_page: u32,
        name: Option<&str>,
        status_list: Option<&str>,
    ) -> AppResult<Value> {
        let mut q: Vec<(&str, String)> = vec![
            ("page", page.to_string()),
            ("perPage", per_page.clamp(1, 30).to_string()),
        ];
        if let Some(n) = name.filter(|s| !s.trim().is_empty()) {
            q.push(("pipelineName", n.trim().to_string()));
        }
        if let Some(s) = status_list.filter(|s| !s.trim().is_empty()) {
            q.push(("statusList", s.trim().to_string()));
        }
        self.get(self.flow("pipelines"), &q).await
    }

    /// GetPipeline：获取流水线详情（含 YAML/流程配置）。
    pub async fn get_pipeline_raw(&self, pipeline_id: i64) -> AppResult<Value> {
        self.get(self.flow(&format!("pipelines/{pipeline_id}")), &[])
            .await
    }

    /// ListPipelineRuns：获取运行实例列表。
    pub async fn list_runs_raw(
        &self,
        pipeline_id: i64,
        page: u32,
        per_page: u32,
        status: Option<&str>,
        trigger_mode: Option<i64>,
    ) -> AppResult<Value> {
        let mut q: Vec<(&str, String)> = vec![
            ("page", page.to_string()),
            ("perPage", per_page.clamp(1, 30).to_string()),
        ];
        if let Some(s) = status.filter(|s| !s.trim().is_empty()) {
            q.push(("status", s.trim().to_string()));
        }
        if let Some(t) = trigger_mode {
            q.push(("triggerMode", t.to_string()));
        }
        self.get(
            self.flow(&format!("pipelines/{pipeline_id}/runs")),
            &q,
        )
        .await
    }

    /// GetPipelineRun：获取单次运行实例详情。
    pub async fn get_run_raw(&self, pipeline_id: i64, run_id: i64) -> AppResult<Value> {
        self.get(
            self.flow(&format!("pipelines/{pipeline_id}/runs/{run_id}")),
            &[],
        )
        .await
    }

    /// GetLatestPipelineRun：获取最近一次运行信息。
    pub async fn get_latest_run_raw(&self, pipeline_id: i64) -> AppResult<Value> {
        self.get(
            self.flow(&format!(
                "pipelines/{pipeline_id}/runs/latestPipelineRun"
            )),
            &[],
        )
        .await
    }

    /// GetPipelineJobSteps：获取任务步骤列表（拿到 stepIndex 与 buildId 才能查日志）。
    pub async fn get_job_steps_raw(
        &self,
        pipeline_id: i64,
        run_id: i64,
        job_id: i64,
    ) -> AppResult<Value> {
        self.get(
            self.flow(&format!(
                "pipelines/{pipeline_id}/pipelineRuns/{run_id}/jobs/{job_id}/steps"
            )),
            &[],
        )
        .await
    }

    /// GetPipelineJobStepLog：增量拉取步骤日志。
    pub async fn get_job_step_log_raw(
        &self,
        pipeline_id: i64,
        run_id: i64,
        job_id: i64,
        step_index: i64,
        build_id: i64,
        offset: i64,
        limit: i64,
    ) -> AppResult<Value> {
        let q = vec![
            ("stepIndex", step_index.to_string()),
            ("buildId", build_id.to_string()),
            ("offset", offset.max(0).to_string()),
            ("limit", limit.clamp(1, 100_000).to_string()),
        ];
        self.get(
            self.flow(&format!(
                "pipelines/{pipeline_id}/pipelineRuns/{run_id}/jobs/{job_id}/step/log"
            )),
            &q,
        )
        .await
    }

    /// GetPipelineJobStepLogUrl：获取日志下载地址。
    pub async fn get_job_step_log_url_raw(
        &self,
        pipeline_id: i64,
        run_id: i64,
        job_id: i64,
        step_index: i64,
        build_id: i64,
    ) -> AppResult<Value> {
        let q = vec![
            ("stepIndex", step_index.to_string()),
            ("buildId", build_id.to_string()),
        ];
        self.get(
            self.flow(&format!(
                "pipelines/{pipeline_id}/pipelineRuns/{run_id}/jobs/{job_id}/step/log/url"
            )),
            &q,
        )
        .await
    }

    // -----------------------------------------------------------------------
    // 写操作接口（需要令牌具备「流水线 → 流水线运行实例 / 运行任务」读写权限）
    // -----------------------------------------------------------------------

    /// CreatePipelineRun：运行流水线，返回新的 pipelineRunId。
    ///
    /// `params` 是云效要求的「JSON 字符串」，可包含 branchModeBranchs / envs /
    /// runningBranchs / runningTags / comment 等字段；为空时用无参默认配置触发。
    pub async fn start_run(&self, pipeline_id: i64, params: Option<Value>) -> AppResult<i64> {
        let params_string = match params {
            Some(Value::String(s)) if !s.trim().is_empty() => s,
            Some(v) if !v.is_null() => serde_json::to_string(&v).unwrap_or_else(|_| "{}".into()),
            _ => "{}".to_string(),
        };
        let body = serde_json::json!({ "params": params_string });
        let v = self
            .post(self.flow(&format!("pipelines/{pipeline_id}/runs")), Some(body))
            .await?;
        extract_run_id(&v).ok_or_else(|| {
            AppError::Parse(format!(
                "触发成功但未能解析出 pipelineRunId，原始响应：{}",
                truncate(&v.to_string(), 300)
            ))
        })
    }

    /// UpdatePipelineRun：终止流水线运行。
    pub async fn stop_run(&self, pipeline_id: i64, run_id: i64) -> AppResult<bool> {
        let v = self
            .put(
                self.flow(&format!("pipelines/{pipeline_id}/runs/{run_id}")),
                None,
            )
            .await?;
        Ok(as_bool_value(&v))
    }

    /// ExecutePipelineJobRun：手动运行/重跑指定任务。
    pub async fn start_job(&self, pipeline_id: i64, run_id: i64, job_id: i64) -> AppResult<bool> {
        let v = self
            .post(
                self.flow(&format!(
                    "pipelines/{pipeline_id}/pipelineRuns/{run_id}/jobs/{job_id}/start"
                )),
                None,
            )
            .await?;
        Ok(as_bool_value(&v))
    }

    /// StopPipelineJobRun：终止指定任务。
    pub async fn stop_job(&self, pipeline_id: i64, run_id: i64, job_id: i64) -> AppResult<bool> {
        let v = self
            .put(
                self.flow(&format!(
                    "pipelines/{pipeline_id}/pipelineRuns/{run_id}/jobs/{job_id}/stop"
                )),
                None,
            )
            .await?;
        Ok(as_bool_value(&v))
    }

    /// SkipPipelineJobRun：跳过指定任务。
    pub async fn skip_job(&self, pipeline_id: i64, run_id: i64, job_id: i64) -> AppResult<bool> {
        let v = self
            .put(
                self.flow(&format!(
                    "pipelines/{pipeline_id}/pipelineRuns/{run_id}/jobs/{job_id}/skip"
                )),
                None,
            )
            .await?;
        Ok(as_bool_value(&v))
    }
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

fn encode(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

fn truncate(s: &str, max: usize) -> String {
    let one_line = s.replace('\n', " ").replace('\r', " ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let cut: String = one_line.chars().take(max).collect();
        format!("{cut}…")
    }
}

fn map_http_error(status: StatusCode, body: &str) -> AppError {
    let brief = truncate(body, 500);
    match status.as_u16() {
        401 => AppError::Unauthorized { status: 401 },
        403 => AppError::Forbidden { status: 403 },
        404 => AppError::Http {
            status: 404,
            body: if brief.is_empty() {
                "资源不存在，请检查 organizationId / pipelineId / pipelineRunId 是否正确".into()
            } else {
                brief
            },
        },
        429 => AppError::Http {
            status: 429,
            body: "触发接口限流，请调大轮询间隔后重试".into(),
        },
        _ => AppError::Http {
            status: status.as_u16(),
            body: if brief.is_empty() {
                status.to_string()
            } else {
                brief
            },
        },
    }
}

/// 兼容 `123` / `{"pipelineRunId":123}` / `{"result":123}` 等多种返回形态。
fn extract_run_id(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    for key in ["pipelineRunId", "runId", "id", "result", "data"] {
        if let Some(inner) = v.get(key) {
            if let Some(n) = inner.as_i64() {
                return Some(n);
            }
            if inner.is_object() {
                if let Some(n) = extract_run_id(inner) {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn as_bool_value(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Object(map) => {
            if let Some(Value::Bool(b)) = map.get("success") {
                return *b;
            }
            if let Some(Value::Bool(b)) = map.get("result") {
                return *b;
            }
            true
        }
        Value::String(s) => !matches!(s.as_str(), "false" | "0"),
        _ => true,
    }
}

/// 从任意响应里取出「业务数据数组」。
///
/// 云效不同接口的包装层不一致：有的直接返回数组，有的包在 `result` / `data` /
/// `pipelines` 里，这里统一兜住，避免前端各处重复判断。
pub fn extract_array(v: &Value) -> Vec<Value> {
    if let Value::Array(a) = v {
        return a.clone();
    }
    for key in [
        "pipelines",
        "pipelineRuns",
        "runs",
        "result",
        "results",
        "data",
        "list",
        "items",
    ] {
        if let Some(inner) = v.get(key) {
            if let Value::Array(a) = inner {
                return a.clone();
            }
            if inner.is_object() {
                let nested = extract_array(inner);
                if !nested.is_empty() {
                    return nested;
                }
            }
        }
    }
    Vec::new()
}

/// 从包装响应里取出单个业务对象（GetPipelineRun 等接口可能带 requestId 外层）。
pub fn extract_object(v: &Value) -> Value {
    if v.is_object() {
        for key in ["pipelineRun", "result", "data"] {
            if let Some(inner) = v.get(key) {
                if inner.is_object() {
                    return inner.clone();
                }
            }
        }
    }
    v.clone()
}
