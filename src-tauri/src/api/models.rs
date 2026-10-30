use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 云效接口返回的 JSON 结构在不同版本间存在字段增减，
/// 这里统一用「宽容解析」：字段缺失一律降级为 None / 空集合，绝不 panic。
/// 若你的组织返回了额外字段，直接往对应结构体上加 `Option<T>` 即可。

fn pick<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| v.get(*k)).filter(|x| !x.is_null())
}

fn as_i64(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn as_string(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        other => Some(other.to_string()),
    }
}

fn as_bool(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        Value::Number(n) => n.as_i64().map(|i| i != 0),
        _ => None,
    }
}

fn as_array(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

/// 取数组里的第一个元素；对象/空值原样返回，方便统一处理「数组 or 对象」两种响应。
fn first_entry(v: &Value) -> Value {
    match v {
        Value::Array(items) => items.first().cloned().unwrap_or(Value::Null),
        other => other.clone(),
    }
}

/// 云效的 commit 字段常是百分号编码的文本（中文提交信息会被转义），这里解码成可读文本。
fn decode_percent(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

/// 解析 `data.commit`。
///
/// 实际返回是一段 JSON 文本（对象或数组），例如：
/// `[{"commitMsg":"%E4%BF%AE...","commitId":"10127ea5...","commitAuthor":"..."}]`
/// 返回 `(commit_id, commit_message)`。
fn parse_commit_text(raw: &str) -> Option<(Option<String>, Option<String>)> {
    let trimmed = raw.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    let v: Value = serde_json::from_str(trimmed).ok()?;
    let first = first_entry(&v);
    let id = as_string(pick(&first, &["commitId", "sha", "id"]));
    let msg = as_string(pick(&first, &["commitMsg", "commitMessage", "message"]))
        .map(|m| decode_percent(&m));
    Some((id, msg))
}

// ---------------------------------------------------------------------------
// 流水线
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineSummary {
    pub pipeline_id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline_type: Option<String>,
    /// 最近一次运行；列表接口不返回时为 None，由轮询器补齐
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_run: Option<RunSummary>,
    /// 是否被用户标记为「关注」
    pub watched: bool,
}

impl PipelineSummary {
    pub fn from_value(v: &Value, watched: bool) -> Option<Self> {
        let pipeline_id = as_i64(pick(v, &["pipelineId", "id"]))?;
        Some(Self {
            pipeline_id,
            name: as_string(pick(v, &["name", "pipelineName"])).unwrap_or_else(|| format!("流水线 {pipeline_id}")),
            creator_account_id: as_string(pick(v, &["creatorAccountId", "creator"])),
            create_time: as_i64(pick(v, &["createTime", "gmtCreate"])),
            group_id: as_i64(pick(v, &["groupId"])),
            group_name: as_string(pick(v, &["groupName"])),
            env_name: as_string(pick(v, &["envName"])),
            pipeline_type: as_string(pick(v, &["pipelineType"])),
            latest_run: v
                .get("lastPipelineRun")
                .or_else(|| v.get("latestPipelineRun"))
                .and_then(RunSummary::from_value),
            watched,
        })
    }
}

// ---------------------------------------------------------------------------
// 运行实例
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<String>,
}

impl SourceInfo {
    pub fn from_value(v: &Value) -> Option<Self> {
        // data 字段是代码源信息载体，个别返回会把它平铺在外层
        let data = v.get("data").filter(|d| d.is_object()).unwrap_or(v);

        // commit 字段有两种形态：直接是 sha，或是一段带 commitId/commitMsg 的 JSON 文本
        let mut commit_id = None;
        let mut commit_message = None;
        if let Some(c) = pick(data, &["commit", "commitId", "revision"]) {
            let text = as_string(Some(c)).unwrap_or_default();
            match parse_commit_text(&text) {
                Some((id, msg)) => {
                    commit_id = id;
                    commit_message = msg;
                }
                None => commit_id = Some(decode_percent(&text)),
            }
        }
        if commit_message.is_none() {
            commit_message = as_string(pick(data, &["commitMsg", "commitMessage", "message"]))
                .map(|m| decode_percent(&m));
        }

        Some(Self {
            sign: as_string(pick(v, &["sign", "name"])),
            repo_type: as_string(pick(v, &["type", "repoType"])),
            repo: as_string(pick(data, &["repo", "repoUrl", "url"])),
            branch: as_string(pick(data, &["branch", "branchName"])),
            commit_id,
            commit_message,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionInfo {
    #[serde(rename = "type")]
    pub action_type: String,
    pub disable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub job_id: i64,
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    pub actions: Vec<ActionInfo>,
}

impl JobInfo {
    pub fn from_value(v: &Value) -> Option<Self> {
        let job_id = as_i64(pick(v, &["id", "jobId"]))?;
        let actions = as_array(v.get("actions"))
            .iter()
            .filter_map(|a| {
                Some(ActionInfo {
                    action_type: as_string(pick(a, &["type", "actionType"]))?,
                    disable: as_bool(pick(a, &["disable"])).unwrap_or(true),
                    params: a.get("params").cloned(),
                })
            })
            .collect();
        Some(Self {
            job_id,
            name: as_string(pick(v, &["name", "jobName"])).unwrap_or_else(|| format!("任务 {job_id}")),
            status: as_string(pick(v, &["status"])).unwrap_or_else(|| "UNKNOWN".into()),
            start_time: as_i64(pick(v, &["startTime"])),
            end_time: as_i64(pick(v, &["endTime"])),
            actions,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageInfo {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    pub jobs: Vec<JobInfo>,
}

impl StageInfo {
    pub fn from_value(v: &Value) -> Option<Self> {
        // 兼容两种形态：{name, stageInfo:{...}} 与直接平铺 {...}
        let info = v.get("stageInfo").filter(|s| s.is_object()).unwrap_or(v);
        let name = as_string(pick(v, &["name", "stageName"]))
            .or_else(|| as_string(pick(info, &["name", "stageName"])))
            .unwrap_or_else(|| "阶段".into());
        let jobs = as_array(info.get("jobs"))
            .iter()
            .filter_map(JobInfo::from_value)
            .collect();
        Some(Self {
            name,
            status: as_string(pick(info, &["status"])).unwrap_or_else(|| "UNKNOWN".into()),
            start_time: as_i64(pick(info, &["startTime"])),
            end_time: as_i64(pick(info, &["endTime"])),
            jobs,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub pipeline_id: i64,
    pub pipeline_run_id: i64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_mode: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<i64>,
    pub sources: Vec<SourceInfo>,
    pub stages: Vec<StageInfo>,
    /// 运行备注 / 触发信息，部分返回里存在
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl RunSummary {
    pub fn from_value(v: &Value) -> Option<Self> {
        let pipeline_run_id = as_i64(pick(v, &["pipelineRunId", "runId", "id"]))?;
        let stages = as_array(v.get("stages"))
            .iter()
            .filter_map(StageInfo::from_value)
            .collect();
        let sources = as_array(v.get("sources"))
            .iter()
            .filter_map(SourceInfo::from_value)
            .collect();
        Some(Self {
            pipeline_id: as_i64(pick(v, &["pipelineId"])).unwrap_or(0),
            pipeline_run_id,
            status: as_string(pick(v, &["status"])).unwrap_or_else(|| "UNKNOWN".into()),
            trigger_mode: as_i64(pick(v, &["triggerMode"])),
            creator_account_id: as_string(pick(v, &["creatorAccountId", "creator", "triggerBy"])),
            create_time: as_i64(pick(v, &["createTime"])),
            start_time: as_i64(pick(v, &["startTime", "executeTime"])),
            end_time: as_i64(pick(v, &["endTime", "finishTime"])),
            update_time: as_i64(pick(v, &["updateTime"])),
            sources,
            stages,
            comment: as_string(pick(v, &["comment", "description"])),
        })
    }

    /// 是否处于「未结束」状态，用于决定加速轮询与日志跟踪。
    pub fn is_active(&self) -> bool {
        matches!(
            self.status.to_ascii_uppercase().as_str(),
            "RUNNING" | "WAITING" | "INIT" | "QUEUED" | "PENDING" | "READY"
        )
    }
}

// ---------------------------------------------------------------------------
// 任务步骤 / 日志
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInfo {
    pub step_index: i64,
    pub step_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
    pub status: String,
    pub running: bool,
    pub finish: bool,
    pub support_debug: bool,
}

impl StepInfo {
    pub fn from_value(v: &Value) -> Option<Self> {
        let step_index = as_i64(pick(v, &["stepIndex", "nodeIndex"]))?;
        let status = as_string(pick(v, &["status"])).unwrap_or_else(|| "UNKNOWN".into());
        let lower = status.to_ascii_lowercase();
        // 实际响应里没有 running / finish 布尔字段，只能从 status 推断
        let running = as_bool(pick(v, &["running"])).unwrap_or_else(|| {
            matches!(
                lower.as_str(),
                "running" | "executing" | "in_progress" | "inprogress"
            )
        });
        let finish = as_bool(pick(v, &["finish"])).unwrap_or_else(|| {
            matches!(
                lower.as_str(),
                "success"
                    | "succeed"
                    | "fail"
                    | "failed"
                    | "failure"
                    | "error"
                    | "cancel"
                    | "canceled"
                    | "cancelled"
                    | "skip"
                    | "skipped"
                    | "timeout"
            )
        });
        Some(Self {
            step_index,
            step_name: as_string(pick(v, &["stepName", "nodeName", "name"]))
                .unwrap_or_else(|| format!("步骤 {step_index}")),
            node_name: as_string(pick(v, &["nodeName"])),
            status,
            running,
            finish,
            support_debug: as_bool(pick(v, &["supportDebug"])).unwrap_or(false),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSteps {
    pub job_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_name: Option<String>,
    pub steps: Vec<StepInfo>,
}

impl JobSteps {
    pub fn from_value(v: &Value, fallback_job_id: i64) -> Self {
        // 云效中心版实际返回的是数组（一次构建过程一个元素），步骤放在 buildProcessNodes 里；
        // 官方文档里的对象形态（steps 字段）也一并兼容。取第一个有步骤的元素。
        let entry = match v {
            Value::Array(items) => items
                .iter()
                .find(|e| {
                    !as_array(e.get("buildProcessNodes")).is_empty()
                        || !as_array(e.get("steps")).is_empty()
                })
                .or_else(|| items.first())
                .cloned()
                .unwrap_or(Value::Null),
            other => other.clone(),
        };

        let steps = as_array(
            entry
                .get("buildProcessNodes")
                .or_else(|| entry.get("steps")),
        )
        .iter()
        .filter_map(StepInfo::from_value)
        .collect();

        let build_id = as_i64(pick(&entry, &["buildId"]));
        let response_job_id = as_i64(pick(&entry, &["jobId"]));
        // 实际响应里的 jobId 就是 buildId，不是请求的「运行任务 ID」，不能直接采信
        let job_id = match response_job_id {
            Some(j) if Some(j) != build_id => j,
            _ => fallback_job_id,
        };

        Self {
            job_id,
            build_id,
            action_code: as_string(pick(&entry, &["actionCode"])),
            action_name: as_string(pick(&entry, &["actionName"])),
            steps,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogChunk {
    /// 本次拉取到的日志内容
    pub logs: String,
    /// 服务端给出的下次起始位置
    pub last: i64,
    /// 是否还有更新的日志
    pub more: bool,
}

impl LogChunk {
    pub fn from_value(v: &Value) -> Self {
        let mut logs = as_string(pick(v, &["logs", "log", "content"])).unwrap_or_default();
        // 步骤尚未产出日志、或 offset 越过日志末尾时，云效会返回一段
        // "[ERROR] Log real-time query exception ..." 的提示文本，
        // 它不是真实日志，过滤掉，避免日志面板出现误导性报错。
        if logs.contains("Log real-time query exception") {
            logs.clear();
        }
        Self {
            logs,
            last: as_i64(pick(v, &["last", "offset", "nextOffset"])).unwrap_or(0),
            more: as_bool(pick(v, &["more", "hasMore"])).unwrap_or(false),
        }
    }
}

// ---------------------------------------------------------------------------
// 轮询快照（推送给前端）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollSnapshot {
    pub pipelines: Vec<PipelineSummary>,
    pub fetched_at: i64,
    pub running_count: usize,
    pub failed_count: usize,
    pub success_count: usize,
    pub interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusChange {
    pub pipeline_id: i64,
    pub pipeline_name: String,
    pub pipeline_run_id: i64,
    pub from_status: String,
    pub to_status: String,
    pub at: i64,
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 云效中心版 GetPipelineJobSteps 的真实返回（节选）：
    /// 顶层是数组，步骤在 buildProcessNodes 里，且 jobId 等于 buildId。
    const STEPS_JSON: &str = r#"[
      {
        "buildId": 568288868,
        "jobId": 568288868,
        "actionCode": "EXECUTION_COMPONENT_BUILD",
        "actionName": "构建",
        "buildProcessNodes": [
          {"nodeName":"申请运行环境(25s)","nodeIndex":0,"status":"success","stepIndex":0,"stepName":"申请运行环境(25s)"},
          {"nodeName":"Java 构建(80s)","nodeIndex":1,"status":"running","stepIndex":1,"stepName":"Java 构建(80s)"},
          {"nodeName":"推送镜像(24s)","nodeIndex":2,"status":"ready","stepIndex":2,"stepName":"推送镜像(24s)"}
        ]
      }
    ]"#;

    #[test]
    fn parses_array_steps_response() {
        let v: Value = serde_json::from_str(STEPS_JSON).unwrap();
        let s = JobSteps::from_value(&v, 542144969);
        assert_eq!(s.job_id, 542144969, "响应里的 jobId 是 buildId，任务 ID 应保留请求值");
        assert_eq!(s.build_id, Some(568288868));
        assert_eq!(s.action_code.as_deref(), Some("EXECUTION_COMPONENT_BUILD"));
        assert_eq!(s.action_name.as_deref(), Some("构建"));
        assert_eq!(s.steps.len(), 3);
        assert_eq!(s.steps[1].step_name, "Java 构建(80s)");
        assert!(s.steps[1].running);
        assert!(!s.steps[2].running);
        assert!(!s.steps[2].finish, "ready 的步骤不算结束");
        assert!(s.steps[0].finish);
    }

    #[test]
    fn parses_documented_object_steps_response() {
        let v = serde_json::json!({
            "jobId": 7, "buildId": 9, "actionCode": "X", "actionName": "构建",
            "steps": [{"stepIndex": 0, "stepName": "执行命令", "status": "success", "finish": true, "running": false, "supportDebug": false}]
        });
        let s = JobSteps::from_value(&v, 42);
        assert_eq!(s.job_id, 7);
        assert_eq!(s.build_id, Some(9));
        assert_eq!(s.steps.len(), 1);
        assert!(s.steps[0].finish);
    }

    #[test]
    fn filters_placeholder_error_log() {
        let v = serde_json::json!({
            "last": -1, "more": false,
            "logs": "[executionStep begins at ...]\n[ERROR] Log real-time query exception. Please try again later or view the complete log through the log download link"
        });
        assert!(LogChunk::from_value(&v).logs.is_empty());
        let ok = serde_json::json!({"last": -1, "more": false, "logs": "[INFO] build ok"});
        assert_eq!(LogChunk::from_value(&ok).logs, "[INFO] build ok");
    }

    #[test]
    fn parses_commit_text_with_percent_encoding() {
        // 真实返回：commit 是一段 JSON 数组文本，提交信息做了百分号编码
        let v = serde_json::json!({
            "type": "codeup",
            "sign": "fky-mgr-sys_oe33",
            "data": {
                "branch": "master",
                "repo": "https://codeup.aliyun.com/xxx/fky-mgr-sys.git",
                "commit": r#"[{"commitMsg": "%E4%BF%AE%E5%A4%8D%E9%97%AE%E9%A2%98", "commitId": "10127ea54793ff57081ac4f1030e80cc790bd3c1", "commitAuthor": "chai%20%3Cchai_jack_work%40outlook.com%3E", "commitTime": "Sat Oct 24 11:30:24 2026 +0800"}]"#
            }
        });
        let s = SourceInfo::from_value(&v).unwrap();
        assert_eq!(
            s.commit_id.as_deref(),
            Some("10127ea54793ff57081ac4f1030e80cc790bd3c1")
        );
        assert_eq!(s.commit_message.as_deref(), Some("修复问题"));
        assert_eq!(s.branch.as_deref(), Some("master"));
    }
}
