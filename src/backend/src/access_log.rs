use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        access_log_service_server::AccessLogService, AnalyzeAccessLogRequest,
        AnalyzeAccessLogResponse, CountItem,
    },
};

// 低配主机防 OOM:去重容器(UV 集合 / Top 各 HashMap)的 distinct key 上限。
// 达上限后只累加已存在的 key,不再新增,unique/Top 即为近似(truncated=true)。
const DISTINCT_CAP: usize = 50_000;
// 单次最多扫描的行数,避免超大日志拖垮低配主机(按需多次/外部 logrotate 处理)。
const MAX_LINES: u64 = 5_000_000;
const DEFAULT_TOP_N: u32 = 10;
const MAX_TOP_N: u32 = 100;

#[derive(Clone, Debug, Default)]
pub struct AccessLogServiceImpl;

#[tonic::async_trait]
impl AccessLogService for AccessLogServiceImpl {
    async fn analyze_access_log(
        &self,
        request: Request<AnalyzeAccessLogRequest>,
    ) -> Result<GrpcResponse<AnalyzeAccessLogResponse>, Status> {
        let request = request.into_inner();
        let path = PathBuf::from(request.path.trim());
        if path.as_os_str().is_empty() || !path.is_absolute() {
            return Err(Status::invalid_argument("path 必须是访问日志的绝对路径"));
        }
        let top_n = match request.top_n {
            0 => DEFAULT_TOP_N,
            n => n.min(MAX_TOP_N),
        } as usize;

        // 解析是 CPU + 阻塞 IO,放 spawn_blocking;逐行读,不整档进内存。
        let response = tokio::task::spawn_blocking(move || analyze_file(&path, top_n))
            .await
            .map_err(|error| Status::internal(error.to_string()))?
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => Status::not_found("日志文件不存在"),
                std::io::ErrorKind::PermissionDenied => {
                    Status::permission_denied("无权读取该日志文件")
                }
                _ => Status::internal(error.to_string()),
            })?;
        Ok(GrpcResponse::new(response))
    }
}

#[derive(Default)]
struct Aggregate {
    total_requests: u64,
    total_bytes: u64,
    status_2xx: u64,
    status_3xx: u64,
    status_4xx: u64,
    status_5xx: u64,
    bot_requests: u64,
    parsed_lines: u64,
    skipped_lines: u64,
    truncated: bool,
    visitors: HashSet<String>,
    paths: HashMap<String, u64>,
    ips: HashMap<String, u64>,
    user_agents: HashMap<String, u64>,
}

fn analyze_file(path: &PathBuf, top_n: usize) -> std::io::Result<AnalyzeAccessLogResponse> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut agg = Aggregate::default();

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            // 二进制/非 UTF-8 行跳过,不让整个解析失败。
            Err(_) => {
                agg.skipped_lines += 1;
                continue;
            }
        };
        if agg.parsed_lines + agg.skipped_lines >= MAX_LINES {
            agg.truncated = true;
            break;
        }
        match parse_line(&line) {
            Some(entry) => apply_entry(&mut agg, entry),
            None => agg.skipped_lines += 1,
        }
    }

    Ok(AnalyzeAccessLogResponse {
        status: Some(ok_response(if agg.truncated {
            "ok(数据量较大,结果为近似/已截断)"
        } else {
            "ok"
        })),
        total_requests: agg.total_requests,
        unique_visitors: agg.visitors.len() as u64,
        total_bytes: agg.total_bytes,
        status_2xx: agg.status_2xx,
        status_3xx: agg.status_3xx,
        status_4xx: agg.status_4xx,
        status_5xx: agg.status_5xx,
        bot_requests: agg.bot_requests,
        parsed_lines: agg.parsed_lines,
        skipped_lines: agg.skipped_lines,
        top_paths: top_n_items(&agg.paths, top_n),
        top_ips: top_n_items(&agg.ips, top_n),
        top_user_agents: top_n_items(&agg.user_agents, top_n),
        truncated: agg.truncated,
    })
}

struct ParsedEntry {
    ip: String,
    path: String,
    status: u16,
    bytes: u64,
    user_agent: String,
}

/// 解析 combined 日志格式:
/// `IP - - [date] "METHOD /path proto" status bytes "referer" "user-agent"`
fn parse_line(line: &str) -> Option<ParsedEntry> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let ip = line.split_whitespace().next()?.to_owned();

    // 取第一对引号内的 request 行。
    let first_quote = line.find('"')?;
    let after_first = &line[first_quote + 1..];
    let request_end = after_first.find('"')?;
    let request = &after_first[..request_end];
    let path = request
        .split_whitespace()
        .nth(1)
        .unwrap_or("-")
        .split('?')
        .next()
        .unwrap_or("-")
        .to_owned();

    // request 之后是 `status bytes ...`。
    let after_request = after_first[request_end + 1..].trim_start();
    let mut tail = after_request.split_whitespace();
    let status: u16 = tail.next()?.parse().ok()?;
    let bytes: u64 = tail
        .next()
        .and_then(|token| token.parse().ok())
        .unwrap_or(0);

    // user-agent 是最后一对引号内的字段。
    let user_agent = line
        .rfind('"')
        .and_then(|close| line[..close].rfind('"').map(|open| (open, close)))
        .map(|(open, close)| line[open + 1..close].to_owned())
        .unwrap_or_default();

    Some(ParsedEntry {
        ip,
        path,
        status,
        bytes,
        user_agent,
    })
}

fn apply_entry(agg: &mut Aggregate, entry: ParsedEntry) {
    agg.parsed_lines += 1;
    agg.total_requests += 1;
    agg.total_bytes += entry.bytes;
    match entry.status {
        200..=299 => agg.status_2xx += 1,
        300..=399 => agg.status_3xx += 1,
        400..=499 => agg.status_4xx += 1,
        500..=599 => agg.status_5xx += 1,
        _ => {}
    }
    if is_bot(&entry.user_agent) {
        agg.bot_requests += 1;
    }
    insert_capped_set(&mut agg.visitors, entry.ip.clone(), &mut agg.truncated);
    bump_capped(&mut agg.ips, entry.ip, &mut agg.truncated);
    bump_capped(&mut agg.paths, entry.path, &mut agg.truncated);
    if !entry.user_agent.is_empty() {
        bump_capped(&mut agg.user_agents, entry.user_agent, &mut agg.truncated);
    }
}

fn is_bot(user_agent: &str) -> bool {
    let lower = user_agent.to_ascii_lowercase();
    ["bot", "spider", "crawl", "slurp", "bingpreview"]
        .iter()
        .any(|needle| lower.contains(needle))
}

/// 已存在则累加;不存在且未到上限则插入;到上限则丢弃新 key 并标记近似。
fn bump_capped(map: &mut HashMap<String, u64>, key: String, truncated: &mut bool) {
    if let Some(count) = map.get_mut(&key) {
        *count += 1;
    } else if map.len() < DISTINCT_CAP {
        map.insert(key, 1);
    } else {
        *truncated = true;
    }
}

fn insert_capped_set(set: &mut HashSet<String>, key: String, truncated: &mut bool) {
    if set.contains(&key) {
        return;
    }
    if set.len() < DISTINCT_CAP {
        set.insert(key);
    } else {
        *truncated = true;
    }
}

fn top_n_items(map: &HashMap<String, u64>, top_n: usize) -> Vec<CountItem> {
    let mut items: Vec<(&String, &u64)> = map.iter().collect();
    // 计数降序,计数相同按 key 升序保证确定性。
    items.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    items
        .into_iter()
        .take(top_n)
        .map(|(key, count)| CountItem {
            key: key.clone(),
            count: *count,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combined_log_line() {
        let line = r#"203.0.113.7 - - [10/Oct/2026:13:55:36 +0000] "GET /index.html?a=1 HTTP/1.1" 200 1024 "https://ref" "Mozilla/5.0""#;
        let entry = parse_line(line).expect("parse");
        assert_eq!(entry.ip, "203.0.113.7");
        assert_eq!(entry.path, "/index.html"); // 查询串被剥离
        assert_eq!(entry.status, 200);
        assert_eq!(entry.bytes, 1024);
        assert_eq!(entry.user_agent, "Mozilla/5.0");
    }

    #[test]
    fn handles_dash_bytes_and_bot_ua() {
        let line = r#"66.249.66.1 - - [10/Oct/2026:00:00:00 +0000] "GET /robots.txt HTTP/1.1" 404 - "-" "Googlebot/2.1""#;
        let entry = parse_line(line).expect("parse");
        assert_eq!(entry.bytes, 0); // "-" → 0
        assert_eq!(entry.status, 404);
        assert!(is_bot(&entry.user_agent));
    }

    #[test]
    fn rejects_garbage_line() {
        assert!(parse_line("not a log line").is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn top_n_sorts_by_count_then_key() {
        let mut map = HashMap::new();
        map.insert("/a".to_owned(), 5u64);
        map.insert("/b".to_owned(), 9u64);
        map.insert("/c".to_owned(), 5u64);
        let top = top_n_items(&map, 2);
        assert_eq!(top[0].key, "/b");
        assert_eq!(top[0].count, 9);
        // 计数并列时按 key 升序 → /a 在 /c 前。
        assert_eq!(top[1].key, "/a");
    }

    #[test]
    fn cap_marks_truncated_and_bounds_size() {
        let mut map = HashMap::new();
        let mut truncated = false;
        // 灌满到上限再多塞一个新 key,应被丢弃并置 truncated。
        for i in 0..DISTINCT_CAP {
            bump_capped(&mut map, format!("k{i}"), &mut truncated);
        }
        assert!(!truncated);
        bump_capped(&mut map, "overflow".to_owned(), &mut truncated);
        assert!(truncated);
        assert_eq!(map.len(), DISTINCT_CAP);
        // 已存在的 key 仍可累加。
        bump_capped(&mut map, "k0".to_owned(), &mut truncated);
        assert_eq!(map.get("k0"), Some(&2));
    }
}
