use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Debug, Default)]
pub struct Metrics {
    requests: AtomicU64,
    tokens_issued: AtomicU64,
    token_issue_microseconds: AtomicU64,
    executions: AtomicU64,
    rejections: Mutex<BTreeMap<String, u64>>,
}

impl Metrics {
    pub fn request(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn token_issued(&self, elapsed_microseconds: u64) {
        self.tokens_issued.fetch_add(1, Ordering::Relaxed);
        self.token_issue_microseconds
            .fetch_add(elapsed_microseconds, Ordering::Relaxed);
    }

    pub fn execution(&self) {
        self.executions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn rejection(&self, reason: &str) {
        let mut rejections = self.rejections.lock().expect("metrics lock poisoned");
        *rejections.entry(sanitize_label(reason)).or_default() += 1;
    }

    pub fn render(&self, role: &str) -> String {
        let requests = self.requests.load(Ordering::Relaxed);
        let issued = self.tokens_issued.load(Ordering::Relaxed);
        let latency = self.token_issue_microseconds.load(Ordering::Relaxed);
        let executions = self.executions.load(Ordering::Relaxed);
        let rejections = self.rejections.lock().expect("metrics lock poisoned");
        let mut output = format!(
            "# HELP spacl_requests_total HTTP requests handled.\n\
             # TYPE spacl_requests_total counter\n\
             spacl_requests_total{{role=\"{role}\"}} {requests}\n\
             # HELP spacl_tokens_issued_total Action tokens issued.\n\
             # TYPE spacl_tokens_issued_total counter\n\
             spacl_tokens_issued_total{{role=\"{role}\"}} {issued}\n\
             # HELP spacl_token_issue_duration_microseconds_total Total token issue time.\n\
             # TYPE spacl_token_issue_duration_microseconds_total counter\n\
             spacl_token_issue_duration_microseconds_total{{role=\"{role}\"}} {latency}\n\
             # HELP spacl_executions_total Actions completed by a robot runtime.\n\
             # TYPE spacl_executions_total counter\n\
             spacl_executions_total{{role=\"{role}\"}} {executions}\n\
             # HELP spacl_rejections_total Requests rejected by reason.\n\
             # TYPE spacl_rejections_total counter\n"
        );
        for (reason, count) in rejections.iter() {
            output.push_str(&format!(
                "spacl_rejections_total{{role=\"{role}\",reason=\"{reason}\"}} {count}\n"
            ));
        }
        output
    }
}

fn sanitize_label(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Metrics;

    #[test]
    fn renders_counters_and_sanitized_rejection_labels() {
        let metrics = Metrics::default();
        metrics.request();
        metrics.token_issued(42);
        metrics.execution();
        metrics.rejection("SEQUENCE GAP");
        let rendered = metrics.render("robot");
        assert!(rendered.contains("spacl_requests_total{role=\"robot\"} 1"));
        assert!(rendered.contains("reason=\"SEQUENCE_GAP\"} 1"));
        assert!(
            rendered.contains("spacl_token_issue_duration_microseconds_total{role=\"robot\"} 42")
        );
    }
}
